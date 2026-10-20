//! Facet solve (§3, §4, §15): target brightness → one facet plane per cell, for
//! any layout, any light (directional at any azimuth/elevation, or a point light
//! with per-facet direction and `1/d²` falloff) and any response (Lambert with
//! ambient, or a measured table). Pivot heights honour the §4 per-corner rule
//! (every corner ≥ T) and the optional relief cap (every corner ≤ T + max_relief).
//!
//! Also the whole-plate pipeline for these layouts, [`generate_plate`]:
//! layout → partition → facets → quantise → mesh.

use std::time::Instant;

use glam::DVec2;
use serde::Serialize;

use super::layout::{Layout, LayoutKind, TileCut, build_partition, e_max, min_cell_mm};
use super::light::Light;
use super::response::{CellRange, Response, tilt_from_brightness};
use super::SquamaParams;
use crate::cancel::CancelToken;
use crate::error::{Error, Result, Warning};
use crate::image::CellBrightness;
use crate::mesh::Mesh;
use crate::relief::{Facet, Heights, MeshStats, Partition, PieceSet, Tol, mesh_relief_stats, quantise};

/// §12 quality. `Preview`: per-cell inverse, zero optimiser sweeps. `Final`: the
/// shadow-aware optimiser when enabled, plus the extra corner checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub enum Quality {
    #[default]
    Preview,
    Final,
}

/// §12 limits (radians, mm).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Limits {
    /// Largest facet tilt `S` (1°–20°).
    pub max_tilt: f64,
    /// Minimum material behind any facet `T` (≥ 0.4 mm).
    pub base_mm: f64,
    /// Every corner ≤ `T + max_relief`; lowers the effective tilt when needed.
    pub max_relief_mm: Option<f64>,
}

impl Limits {
    pub fn from_params(p: &SquamaParams) -> Self {
        Limits { max_tilt: p.max_tilt, base_mm: p.base_mm, max_relief_mm: None }
    }
}

/// How each facet's height `h` at its pivot is chosen (§4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub enum PivotRule {
    /// Every facet pivots at the same `c = T + e_max·tan S + q`, `e_max` the
    /// largest `|a_k · (corner − pivot)|` over every facet's own light direction.
    /// Independent of the image, so neighbours never step while a slider drags.
    #[default]
    Common,
    /// Each facet as low as the per-corner rule allows: its lowest full-cell
    /// corner sits at `T + q`. Less material, but neighbouring pivots differ.
    PerFacet,
}

/// Solver options that are not user parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolveOpts {
    pub pivot: PivotRule,
    /// Refuse reliefs below 3δ (§2). Tests disable it to reach tiny cells.
    pub relief_guard: bool,
}

impl Default for SolveOpts {
    fn default() -> Self {
        SolveOpts { pivot: PivotRule::Common, relief_guard: true }
    }
}

/// What the solve achieved.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
pub struct SolveReport {
    /// Common brightness range every facet can reach (response units).
    pub b_lo: f64,
    pub b_hi: f64,
    /// `(b_hi − b_lo) / b_hi`: 0 = no contrast, 1 = black to full.
    pub contrast: f64,
    /// `true` when no common range existed and each facet used its own range.
    pub per_facet_range: bool,
    /// Tilt cap actually used (radians), after the relief cap.
    pub max_tilt_used: f64,
    /// Largest `|a_k · (corner − pivot)|` over all facets (mm).
    pub e_max_mm: f64,
    /// Pivot height for `PivotRule::Common` (mm), else the largest `h_k`.
    pub pivot_h_mm: f64,
    /// Highest full-cell corner over all facets, before quantisation (mm).
    pub max_corner_mm: f64,
}

/// The solve's output.
#[derive(Debug, Clone, PartialEq)]
pub struct Solved {
    pub facets: Vec<Facet>,
    pub warnings: Vec<Warning>,
    pub report: SolveReport,
}

fn check_limits(lim: &Limits) -> Result<()> {
    let s = lim.max_tilt.to_degrees();
    if !(lim.max_tilt.is_finite() && lim.base_mm.is_finite()) {
        return Err(Error::InvalidParams("non-finite limit".into()));
    }
    if !(1.0 - 1e-9..=20.0 + 1e-9).contains(&s) {
        return Err(Error::InvalidParams(format!("max tilt {s:.3}° outside [1°, 20°]")));
    }
    if lim.base_mm < 0.4 - 1e-12 {
        return Err(Error::InvalidParams(format!("base {} mm below 0.4 mm", lim.base_mm)));
    }
    if let Some(r) = lim.max_relief_mm
        && !(r.is_finite() && r > 0.0)
    {
        return Err(Error::InvalidParams(format!("max relief {r} mm must be positive")));
    }
    Ok(())
}

/// §3/§4/§15: per-facet target brightness `g` (linear, `[0, 1]`, one per facet of
/// `l`, ordered as [`Layout::facet_centre`]) → facet planes.
///
/// The target maps linearly into the brightness range every facet can reach
/// (`B_lo = max_k B_lo,k`, `B_hi = min_k B_hi,k`, §15.4), then each facet's tilt is
/// its response's inverse; `grad_k = −tan θ_k · a_k`. With one directional light,
/// `Lambert { ambient: 0 }` and a square layout this equals the M0 §3 inverse.
///
/// The §12 `solve_facets(t, p, ..)` is `pipeline::solve_job`, which passes
/// `Target::g` here and runs the optimiser on the partition at `Quality::Final`;
/// pivots and full-cell corners come from the layout, so this takes the layout.
#[allow(clippy::too_many_arguments)]
pub fn solve_facets_ex(
    g: &[f32],
    l: &Layout,
    light: &Light,
    lim: &Limits,
    resp: &Response,
    opts: &SolveOpts,
    quality: Quality,
    tol: &Tol,
    cancel: &CancelToken,
) -> Result<Solved> {
    check_limits(lim)?;
    let nf = l.facet_count() as usize;
    if g.len() != nf {
        return Err(Error::InvalidParams(format!("target has {} values for {nf} facets", g.len())));
    }
    if l.cell_mm < min_cell_mm(l.kind, tol) - 1e-12 {
        return Err(Error::InvalidParams(format!("cell {} mm below {:.4} mm", l.cell_mm, min_cell_mm(l.kind, tol))));
    }
    if !(l.w_mm <= 2048.0 + 1e-9 && l.h_mm <= 2048.0 + 1e-9) {
        return Err(Error::InvalidParams(format!("plate {:.3} × {:.3} mm above 2048 mm", l.w_mm, l.h_mm)));
    }
    let mut warnings = Vec::new();
    let centres: Vec<DVec2> = (0..nf as u32).map(|f| l.facet_centre(f)).collect();
    // A directional light is the same at every facet: evaluate it (and its range
    // and e_max) once. `lights` then has one entry; `at(k)` indexes either way.
    let uniform = matches!(light, Light::Directional { .. });
    let (lights, w) = light.cell_lights(if uniform { &centres[..1] } else { &centres }, lim.base_mm)?;
    let at = |k: usize| if uniform { 0 } else { k };
    warnings.extend(w);
    cancel.check()?;

    // e_max over every facet's own light direction (§4).
    let offs = l.corner_offsets();
    let e_dir = |a: DVec2| match l.kind {
        LayoutKind::Square => l.e_max_dir(a),
        LayoutKind::Hex { .. } => offs.iter().map(|o| a.dot(*o).abs()).fold(0.0, f64::max),
    };
    let e_all = match (light, l.kind) {
        // Same expression as M0 so square directional plates stay bit-identical.
        (Light::Directional { az, .. }, LayoutKind::Square) => e_max(l.cell_mm, *az),
        _ => lights.iter().map(|c| e_dir(c.a)).fold(0.0, f64::max),
    };

    // Relief cap → effective tilt (§4, §15.1). Clustering may raise a corner by τ.
    let mut s_eff = lim.max_tilt;
    if let Some(r) = lim.max_relief_mm {
        let room = r - (tol.tau_q as f64 + 2.0) * tol.q;
        if room <= 0.0 {
            return Err(Error::InvalidParams(format!("max relief {r} mm leaves no room for any tilt")));
        }
        let cap = (room / (2.0 * e_all)).atan();
        if cap < s_eff {
            s_eff = cap;
            warnings.push(Warning {
                code: "max_tilt_reduced",
                message: format!("max tilt reduced to {:.2}° by the {r} mm relief cap", cap.to_degrees()),
            });
        }
    }
    let relief = 2.0 * e_all * s_eff.tan();
    if relief < 3.0 * tol.delta && opts.relief_guard {
        return Err(Error::InvalidParams(format!(
            "relief {relief:.3} mm below {:.2} mm; raise max tilt, max relief or cell size",
            3.0 * tol.delta
        )));
    }
    if relief < 8.0 * tol.delta {
        warnings.push(Warning {
            code: "low_relief",
            message: format!("relief {relief:.3} mm below printable detail; raise max tilt or cell size"),
        });
    }

    // Response ranges.
    let ref_el = light.reference(l.w_mm, l.h_mm, lim.base_mm)?.el;
    let curve = resp.prepare(ref_el)?;
    let mut ranges: Vec<CellRange> = Vec::with_capacity(lights.len());
    for (k, cl) in lights.iter().enumerate() {
        ranges.push(curve.cell_range(cl, s_eff).ok_or_else(|| {
            Error::InvalidParams(format!(
                "facet {k}: light elevation {:.1}° is outside the measured response",
                cl.el.to_degrees()
            ))
        })?);
    }
    let b_lo = ranges.iter().map(|r| r.b_lo).fold(f64::NEG_INFINITY, f64::max);
    let b_hi = ranges.iter().map(|r| r.b_hi).fold(f64::INFINITY, f64::min);
    let per_facet_range = !(b_hi > b_lo);
    if per_facet_range {
        warnings.push(Warning {
            code: "no_common_range",
            message: "light falloff is larger than the facets' brightness range: move the lamp farther away".into(),
        });
    }
    let contrast = if per_facet_range || b_hi <= 0.0 { 0.0 } else { (b_hi - b_lo) / b_hi };
    if !per_facet_range && contrast < 0.3 && matches!(light, Light::Point { .. }) {
        warnings.push(Warning {
            code: "low_contrast",
            message: format!("achievable contrast {:.0} %: move the lamp farther away", contrast * 100.0),
        });
    }

    // Tilts.
    let directional_m0 = matches!(light, Light::Directional { .. })
        && matches!(resp, Response::Lambert { ambient } if *ambient == 0.0)
        && s_eff == lim.max_tilt;
    let mut theta = Vec::with_capacity(nf);
    if directional_m0 {
        // [`tilt_from_brightness`] with its two sines hoisted (same operations,
        // so bit-identical to the M0 path).
        let el = lights[0].el;
        let e_min = (el - s_eff).sin().max(0.0);
        let e_maxv = (el + s_eff).sin();
        theta.extend(g.iter().map(|&gk| {
            let gk = gk as f64;
            let gk = if gk.is_finite() { gk.clamp(0.0, 1.0) } else { 0.0 };
            let e = e_min + gk * (e_maxv - e_min);
            (e.clamp(-1.0, 1.0).asin() - el).clamp(-s_eff, s_eff)
        }));
    } else {
        for (k, &gk) in g.iter().enumerate() {
            let gk = gk as f64;
            let gk = if gk.is_finite() { gk.clamp(0.0, 1.0) } else { 0.0 };
            let (cl, r) = (&lights[at(k)], &ranges[at(k)]);
            let (lo, hi) = if per_facet_range { (r.b_lo, r.b_hi) } else { (b_lo, b_hi) };
            theta.push(curve.inverse(lo + gk * (hi - lo), cl, r));
        }
    }
    if quality == Quality::Final {
        // The shadow-aware optimiser (§15.1) refines these facets in
        // `pipeline::solve_job`, which owns the OptimiserOpts.
        cancel.check()?;
    }

    // Planes and pivot heights (§4 per-corner rule).
    let mut facets = Vec::with_capacity(nf);
    let c_common = lim.base_mm + e_all * s_eff.tan() + tol.q;
    let mut max_corner = f64::NEG_INFINITY;
    let mut max_h = f64::NEG_INFINITY;
    for k in 0..nf {
        let grad = -theta[k].tan() * lights[at(k)].a;
        let (lo_off, hi_off) = offs.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), o| {
            let d = grad.dot(*o);
            (a.min(d), b.max(d))
        });
        let h = match opts.pivot {
            PivotRule::Common => c_common,
            PivotRule::PerFacet => lim.base_mm + tol.q - lo_off,
        };
        max_corner = max_corner.max(h + hi_off);
        max_h = max_h.max(h);
        if quality == Quality::Final && h + lo_off < lim.base_mm + 0.5 * tol.q {
            return Err(Error::Internal(format!("facet {k} corner below base")));
        }
        facets.push(Facet { pivot: centres[k], h, grad });
    }
    if let Some(r) = lim.max_relief_mm
        && max_corner > lim.base_mm + r - (tol.tau_q as f64 + 1.0) * tol.q + 1e-9
    {
        return Err(Error::Internal(format!("corner {max_corner:.4} mm above the relief cap")));
    }
    Ok(Solved {
        facets,
        warnings,
        report: SolveReport {
            b_lo,
            b_hi,
            contrast,
            per_facet_range,
            max_tilt_used: s_eff,
            e_max_mm: e_all,
            pivot_h_mm: if opts.pivot == PivotRule::Common { c_common } else { max_h },
            max_corner_mm: max_corner,
        },
    })
}

/// §3–§4 (M0): one facet per square cell, one directional light, centre pivot
/// `h = T + e_max·tan S + q`.
pub fn solve_facets(cb: &CellBrightness, l: &Layout, p: &SquamaParams, tol: &Tol) -> Vec<Facet> {
    let a = DVec2::new(p.light_azimuth.cos(), p.light_azimuth.sin());
    let c = p.base_mm + e_max(l.cell_mm, p.light_azimuth) * p.max_tilt.tan() + tol.q;
    let mut out = Vec::with_capacity((l.nx * l.ny) as usize);
    for j in 0..l.ny {
        for i in 0..l.nx {
            let th = tilt_from_brightness(cb.at(i, j) as f64, p.light_elevation, p.max_tilt);
            let pivot = DVec2::new((i as f64 + 0.5) * l.cell_mm, (j as f64 + 0.5) * l.cell_mm);
            out.push(Facet { pivot, h: c, grad: -th.tan() * a });
        }
    }
    out
}

/// Everything [`generate_plate`] needs besides the target.
#[derive(Debug, Clone, PartialEq)]
pub struct PlateSpec {
    pub layout: Layout,
    pub light: Light,
    pub limits: Limits,
    pub response: Response,
    pub opts: SolveOpts,
    pub quality: Quality,
    pub tol: Tol,
}

/// Timings (ms).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct PlateTimings {
    pub partition_ms: f64,
    pub solve_ms: f64,
    pub quantise_ms: f64,
    pub mesh_ms: f64,
    pub total_ms: f64,
}

/// One whole plate.
#[derive(Debug, Clone, PartialEq)]
pub struct PlateOutput {
    pub mesh: Mesh,
    pub partition: Partition,
    pub heights: Heights,
    pub solved: Solved,
    pub mesh_stats: MeshStats,
    pub timings: PlateTimings,
    /// Lowest allowed facet height, for `validate()` (`T − q/2`).
    pub min_facet_w: f64,
    /// Outline area, for `validate()`.
    pub back_area: f64,
}

impl PlateOutput {
    /// `validate()` options matching this plate.
    pub fn validate_opts(&self, transform: crate::orient::Xform) -> crate::mesh::validate::ValidateOpts {
        crate::mesh::validate::ValidateOpts {
            transform,
            min_facet_w: Some(self.min_facet_w),
            back_area: Some(self.back_area),
            ..Default::default()
        }
    }
}

/// Pure generator for square and hex plates under any light and response:
/// `(spec, per-facet target) → mesh` (one part, the whole plate).
///
/// Tiled plates build the partition with cut lines ([`build_partition`]),
/// quantise once, and mesh each tile's `pieces_in_rect` subset; see the tests.
pub fn generate_plate(spec: &PlateSpec, g: &[f32], cancel: &CancelToken) -> Result<PlateOutput> {
    generate_plate_cut(spec, g, &[], PieceSet::All, cancel)
}

/// [`generate_plate`] with tile cut lines in the partition and a piece subset
/// (one tile) to mesh.
pub fn generate_plate_cut(
    spec: &PlateSpec,
    g: &[f32],
    cuts: &[TileCut],
    pieces: PieceSet<'_>,
    cancel: &CancelToken,
) -> Result<PlateOutput> {
    let t0 = Instant::now();
    let tol = &spec.tol;
    // Solve first: it checks the parameters (plate size, limits) before any
    // partition is allocated.
    let solved = solve_facets_ex(
        g,
        &spec.layout,
        &spec.light,
        &spec.limits,
        &spec.response,
        &spec.opts,
        spec.quality,
        tol,
        cancel,
    )?;
    let t1 = Instant::now();
    let part = build_partition(&spec.layout, cuts)?;
    let t2 = Instant::now();
    let heights = quantise(&part, &solved.facets, spec.limits.base_mm, tol)?;
    if let Some(r) = spec.limits.max_relief_mm {
        let cap = tol.base_q(spec.limits.base_mm) + (r / tol.q).floor() as i64;
        if let Some(&hmax) = heights.corner.iter().max()
            && hmax > cap
        {
            return Err(Error::Internal(format!("quantised corner {hmax} µm above the relief cap {cap} µm")));
        }
    }
    let t3 = Instant::now();
    cancel.check()?;
    let (mesh, mesh_stats) = mesh_relief_stats(&part, pieces, &heights, &[], tol, cancel)?;
    let t4 = Instant::now();
    let ms = |a: Instant, b: Instant| (b - a).as_secs_f64() * 1e3;
    let back_area = match pieces {
        PieceSet::All => spec.layout.w_mm * spec.layout.h_mm,
        PieceSet::Subset(s) => {
            let mut v = s.to_vec();
            v.sort_unstable();
            v.dedup();
            v.iter().map(|&k| part.piece_area(k as usize)).sum()
        }
    };
    Ok(PlateOutput {
        mesh,
        partition: part,
        heights,
        solved,
        mesh_stats,
        timings: PlateTimings {
            solve_ms: ms(t0, t1),
            partition_ms: ms(t1, t2),
            quantise_ms: ms(t2, t3),
            mesh_ms: ms(t3, t4),
            total_ms: ms(t0, t4),
        },
        min_facet_w: spec.limits.base_mm - tol.q / 2.0,
        back_area,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::squama::layout::{HexOrientation, layout, layout_for};
    use glam::DVec3;

    fn spec(l: Layout) -> PlateSpec {
        PlateSpec {
            layout: l,
            light: Light::default(),
            limits: Limits { max_tilt: 10f64.to_radians(), base_mm: 3.0, max_relief_mm: None },
            response: Response::default(),
            opts: SolveOpts::default(),
            quality: Quality::Preview,
            tol: Tol::default(),
        }
    }

    #[test]
    fn matches_m0_on_square_directional() {
        let p = SquamaParams { width_mm: 60.0, ..Default::default() };
        let l = layout(&p, (5, 4)).unwrap();
        let g: Vec<f32> = (0..l.facet_count()).map(|k| (k % 7) as f32 / 6.0).collect();
        let cb = CellBrightness { nx: l.nx, ny: l.ny, g: g.clone() };
        let tol = Tol::default();
        let old = solve_facets(&cb, &l, &p, &tol);
        let s = spec(l);
        let new = solve_facets_ex(&g, &l, &s.light, &s.limits, &s.response, &s.opts, Quality::Preview, &tol, &CancelToken::new())
            .unwrap();
        assert_eq!(old, new.facets);
    }

    #[test]
    fn gradient_points_away_from_light_any_azimuth() {
        let p = SquamaParams { width_mm: 30.0, cell_mm: 6.0, height_mm: Some(30.0), ..Default::default() };
        let l = layout_for(&p, LayoutKind::Hex { orientation: HexOrientation::Pointy }, (1, 1)).unwrap();
        let tol = Tol::default();
        for az_deg in [0.0, 37.0, 90.0, 181.0, 300.0] {
            let az = f64::to_radians(az_deg);
            let light = Light::Directional { az, el: 10f64.to_radians() };
            let s = spec(l);
            let g = vec![1.0f32; l.facet_count() as usize];
            let out = solve_facets_ex(&g, &l, &light, &s.limits, &s.response, &s.opts, Quality::Final, &tol, &CancelToken::new())
                .unwrap();
            let a = DVec2::new(az.cos(), az.sin());
            for f in &out.facets {
                // Brightest facets tilt fully toward the light: surface rises away from it.
                assert!((f.grad + 10f64.to_radians().tan() * a).length() < 1e-12);
            }
        }
    }

    #[test]
    fn point_light_falloff_lowers_tilt_near_the_lamp() {
        let p = SquamaParams { width_mm: 120.0, cell_mm: 6.0, height_mm: Some(60.0), ..Default::default() };
        let l = layout(&p, (1, 1)).unwrap();
        let tol = Tol::default();
        let light = Light::Point { pos_mm: DVec3::new(-150.0, 30.0, 40.0) };
        let s = spec(l);
        let g = vec![0.5f32; l.facet_count() as usize];
        let out =
            solve_facets_ex(&g, &l, &light, &s.limits, &s.response, &s.opts, Quality::Preview, &tol, &CancelToken::new()).unwrap();
        assert!(!out.report.per_facet_range && out.report.contrast > 0.0);
        // Same target, one row: facets nearer the lamp get more light, so they tilt
        // less toward it (incidence α + θ smaller) to show the same brightness.
        let row = 5u32;
        let near = &out.facets[(row * l.nx) as usize];
        let far = &out.facets[(row * l.nx + l.nx - 1) as usize];
        let incidence = |f: &Facet, el: f64| el + f.grad.length().atan() * if f.grad.x > 0.0 { 1.0 } else { -1.0 };
        let (cl, _) = light.cell_lights(&[near.pivot, far.pivot], 3.0).unwrap();
        assert!(incidence(near, cl[0].el) < incidence(far, cl[1].el));
    }

    #[test]
    fn relief_cap_and_pivot_rules() {
        let p = SquamaParams { width_mm: 60.0, cell_mm: 6.0, height_mm: Some(36.0), ..Default::default() };
        let tol = Tol::default();
        for kind in [LayoutKind::Square, LayoutKind::Hex { orientation: HexOrientation::Flat }] {
            let l = layout_for(&p, kind, (1, 1)).unwrap();
            let g: Vec<f32> = (0..l.facet_count()).map(|k| ((k * 37) % 11) as f32 / 10.0).collect();
            for pivot in [PivotRule::Common, PivotRule::PerFacet] {
                let mut s = spec(l);
                s.limits.max_relief_mm = Some(0.5);
                s.opts.pivot = pivot;
                s.quality = Quality::Final;
                let out = generate_plate(&s, &g, &CancelToken::new()).unwrap();
                assert!(out.solved.warnings.iter().any(|w| w.code == "max_tilt_reduced"));
                let t_q = tol.base_q(3.0);
                assert!(out.heights.corner.iter().all(|&h| h >= t_q && h <= t_q + 500));
                let r = crate::mesh::validate::validate(&out.mesh, &out.validate_opts(crate::orient::Xform::IDENTITY));
                assert!(r.ok, "{}", r.summary());
            }
        }
        let l = layout(&p, (1, 1)).unwrap();
        let mut s = spec(l);
        s.limits.max_relief_mm = Some(0.04);
        assert!(generate_plate(&s, &vec![0.5; l.facet_count() as usize], &CancelToken::new()).is_err());
    }
}
