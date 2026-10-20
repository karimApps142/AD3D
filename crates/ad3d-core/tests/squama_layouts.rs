//! M1 layouts, lights and responses (design §2.1, §3, §4, §6.2, §15, §17):
//! property tests over random square/hex layouts, any light azimuth/elevation,
//! point lights, Lambert/measured responses, pivot rules and relief caps, plus
//! hex edge cases and hex tiles with seams through cells. Every mesh must pass
//! all three `validate()` stages (raw, bit-exact weld, slicer merge) in plate
//! space and in a standing print orientation.

use std::f64::consts::TAU;

use ad3d_core::CancelToken;
use ad3d_core::error::Error;
use ad3d_core::mesh::validate::{ValidateOpts, validate};
use ad3d_core::orient::{Orientation, QuarterTurns, Xform, export_xform};
use ad3d_core::relief::{Facet, PieceSet, Tol, mesh_relief, quantise};
use ad3d_core::squama::{
    HexOrientation, Layout, LayoutKind, Light, Limits, PivotRule, PlateOutput, PlateSpec, Quality, Response, SolveOpts,
    SquamaParams, TileCut, build_partition, facet_brightness, generate_plate, layout_for,
};
use ad3d_core::image::synthetic_target;
use glam::{DVec2, DVec3};
use proptest::prelude::*;

const KINDS: [LayoutKind; 3] = [
    LayoutKind::Square,
    LayoutKind::Hex { orientation: HexOrientation::Pointy },
    LayoutKind::Hex { orientation: HexOrientation::Flat },
];

fn spec(l: Layout) -> PlateSpec {
    PlateSpec {
        layout: l,
        light: Light::default(),
        limits: Limits { max_tilt: 10f64.to_radians(), base_mm: 3.0, max_relief_mm: None },
        response: Response::default(),
        opts: SolveOpts { relief_guard: false, ..Default::default() },
        quality: Quality::Final,
        tol: Tol::default(),
    }
}

fn light_az(light: &Light, l: &Layout) -> f64 {
    match *light {
        Light::Directional { az, .. } => az,
        Light::Point { .. } => {
            let c = light.reference(l.w_mm, l.h_mm, 3.0).unwrap();
            c.a.y.atan2(c.a.x)
        }
    }
}

/// All three stages, plate space and standing (the given quarter turn).
fn check_plate(s: &PlateSpec, out: &PlateOutput, quarter: QuarterTurns) -> Result<(), String> {
    let r = validate(&out.mesh, &out.validate_opts(Xform::IDENTITY));
    if !r.ok {
        return Err(format!("plate-space validate failed:\n{}", r.summary()));
    }
    if r.slicer.as_ref().is_none_or(|st| st.merged != 0) || r.written.merged != 0 {
        return Err("a weld merged vertices".into());
    }
    let (x, _) = export_xform(Orientation::Standing(quarter), s.layout.w_mm, s.layout.h_mm, light_az(&s.light, &s.layout));
    let r2 = validate(&out.mesh, &out.validate_opts(x));
    if !r2.ok {
        return Err(format!("standing validate failed:\n{}", r2.summary()));
    }
    // §4 per-corner rule after quantisation and clustering.
    let tol = &s.tol;
    let t_q = tol.base_q(s.limits.base_mm);
    if let Some(&lo) = out.heights.corner.iter().min()
        && lo < t_q
    {
        return Err(format!("corner {lo} µm below T {t_q} µm"));
    }
    if let Some(rmax) = s.limits.max_relief_mm {
        let cap = t_q + (rmax / tol.q).floor() as i64;
        if out.heights.corner.iter().any(|&h| h > cap) {
            return Err(format!("corner above the relief cap {cap} µm"));
        }
    }
    let b = out.mesh.bounds().unwrap();
    if (b.max[0] as f64 - s.layout.w_mm).abs() > 1e-3 || (b.max[1] as f64 - s.layout.h_mm).abs() > 1e-3 {
        return Err(format!("bounds {b:?} vs {} × {}", s.layout.w_mm, s.layout.h_mm));
    }
    Ok(())
}

fn pattern(kind: usize, n: usize, noise: &[f32], l: &Layout) -> Vec<f32> {
    (0..n)
        .map(|k| {
            let c = l.facet_centre(k as u32);
            let ci = (c.x / l.cell_mm).round() as i64 + (c.y / l.cell_mm).round() as i64;
            match kind {
                0 => noise[k % noise.len()],
                1 => (noise[k % noise.len()] > 0.5) as u8 as f32,
                2 => (ci.rem_euclid(2)) as f32,
                3 => (k % 3) as f32 / 2.0, // hex 3-colouring-ish
                4 => 0.0,
                5 => 1.0,
                _ => 0.5 + 1e-4 * (noise[k % noise.len()] - 0.5),
            }
        })
        .collect()
}

fn measured_table(seed: &[f32]) -> Response {
    // Rising trend over ±60° with noise and a possible droop at the top.
    let n = 4 + (seed[0] * 5.0) as usize;
    let tilt_rad: Vec<f64> = (0..n).map(|k| (-60.0 + 120.0 * k as f64 / (n - 1) as f64).to_radians()).collect();
    let brightness: Vec<f64> =
        (0..n).map(|k| (k as f64 / (n - 1) as f64) * 0.8 + 0.1 + 0.15 * (seed[(k + 1) % seed.len()] as f64 - 0.5)).map(|b| b.max(0.0)).collect();
    Response::Measured { tilt_rad, brightness }
}

#[derive(Debug, Clone)]
struct Case {
    kind: LayoutKind,
    cell_um: i64,
    cols: u32,
    rows: u32,
    jitter: (f64, f64),
    light: Light,
    max_tilt_deg: f64,
    base: f64,
    resp: Response,
    pivot: PivotRule,
    relief: Option<f64>,
    pat: usize,
    noise: Vec<f32>,
    quarter: QuarterTurns,
}

fn case_strategy() -> impl Strategy<Value = Case> {
    let kind = prop::sample::select(KINDS.to_vec());
    let cell = prop::sample::select(vec![500i64, 700, 777, 1000, 2000, 3000, 4321, 6000, 10000]);
    let sizes = (1u32..13, 1u32..13, -0.49f64..0.49, -0.49f64..0.49);
    let light = (
        0usize..3,
        0.0f64..TAU,
        prop::sample::select(vec![1.0f64, 45.0, 5.0, 17.3]),
        1.0f64..45.0,
        (20.0f64..400.0, 5.0f64..150.0, 0.0f64..TAU),
    );
    let lim = (
        prop::sample::select(vec![1.0f64, 20.0, 10.0, 6.7]),
        prop::sample::select(vec![0.4f64, 3.0, 1.234]),
        prop::option::of(0.2f64..3.0),
    );
    let resp = (0usize..3, 0.0f64..0.5, prop::collection::vec(0f32..=1.0, 12));
    let rest = (
        prop::bool::ANY,
        0usize..7,
        prop::collection::vec(0f32..=1.0, 64),
        prop::sample::select(vec![QuarterTurns::Auto, QuarterTurns::Q0, QuarterTurns::Q1, QuarterTurns::Q2, QuarterTurns::Q3]),
    );
    (kind, cell, sizes, light, lim, resp, rest).prop_map(
        |(kind, cell_um, (cols, rows, jx, jy), (lk, az, el_fixed, el_rand, (dist, height, dir)), (s, t, r), (rk, amb, seed), (pf, pat, noise, quarter))| {
            let el = if lk == 0 { el_fixed } else { el_rand };
            let light = if lk == 2 {
                // Placed relative to the plate centre once the layout is known (see run_case).
                Light::Point { pos_mm: DVec3::new(dist * dir.cos(), dist * dir.sin(), height) }
            } else {
                Light::Directional { az, el: el.to_radians() }
            };
            let resp = match rk {
                0 => Response::Lambert { ambient: 0.0 },
                1 => Response::Lambert { ambient: amb },
                _ => measured_table(&seed),
            };
            Case {
                kind,
                cell_um: if matches!(kind, LayoutKind::Hex { .. }) { cell_um.max(700) } else { cell_um },
                cols,
                rows,
                jitter: (jx, jy),
                light,
                max_tilt_deg: s,
                base: t,
                resp,
                pivot: if pf { PivotRule::Common } else { PivotRule::PerFacet },
                relief: r,
                pat,
                noise,
                quarter,
            }
        },
    )
}

fn run_case(c: &Case) -> Result<(), String> {
    let cell = c.cell_um as f64 / 1000.0;
    let p = SquamaParams {
        width_mm: (c.cols as f64 + c.jitter.0) * cell,
        height_mm: Some(((c.rows as f64 + c.jitter.1) * cell).max(cell)),
        cell_mm: cell,
        ..Default::default()
    };
    let l = layout_for(&p, c.kind, (1, 1)).map_err(|e| e.to_string())?;
    let mut s = spec(l);
    s.light = match c.light {
        Light::Point { pos_mm } => Light::Point { pos_mm: pos_mm + DVec3::new(l.w_mm / 2.0, l.h_mm / 2.0, c.base) },
        d => d,
    };
    s.limits = Limits { max_tilt: c.max_tilt_deg.to_radians(), base_mm: c.base, max_relief_mm: c.relief };
    s.response = c.resp.clone();
    s.opts.pivot = c.pivot;
    let g = pattern(c.pat, l.facet_count() as usize, &c.noise, &l);
    let out = match generate_plate(&s, &g, &CancelToken::new()) {
        Ok(o) => o,
        // A lamp so close that some facets see it outside the calibrated tilt
        // range is a legitimate refusal, never a broken mesh.
        Err(Error::InvalidParams(m)) if m.contains("outside the measured response") => return Ok(()),
        Err(e) => return Err(format!("generate failed: {e} for {c:?} / {l:?}")),
    };
    check_plate(&s, &out, c.quarter).map_err(|e| format!("{e}\nfor {c:?} / {l:?}"))
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 200, .. ProptestConfig::default() })]

    #[test]
    fn random_layouts_lights_responses(c in case_strategy()) {
        if let Err(e) = run_case(&c) {
            return Err(TestCaseError::fail(e));
        }
    }

    /// §17: random per-facet `h_k` and `grad_k` within the §4 per-corner rule
    /// (standing in for the optimiser and point lights), hex and square, with and
    /// without tile cuts (each tile meshed and validated on its own).
    #[test]
    fn random_facets_and_tiles(
        kind in prop::sample::select(KINDS.to_vec()),
        cols in 2u32..12, rows in 1u32..10,
        cell_um in prop::sample::select(vec![700i64, 1000, 2500, 6000]),
        grads in prop::collection::vec((0.0f64..TAU, 0.0f64..0.364), 200),
        extra in prop::collection::vec(prop::sample::select(vec![0.0f64, 0.0, 0.03, 0.051, 0.5, 1.7]), 200),
        cut_u in prop::option::of(0.2f64..0.8), cut_v in prop::option::of(0.2f64..0.8),
    ) {
        let cell = cell_um as f64 / 1000.0;
        let p = SquamaParams { width_mm: cols as f64 * cell, height_mm: Some(rows as f64 * cell), cell_mm: cell, ..Default::default() };
        let l = layout_for(&p, kind, (1, 1)).unwrap();
        let tol = Tol::default();
        let base = 0.4;
        let offs = l.corner_offsets();
        let facets: Vec<Facet> = (0..l.facet_count()).map(|f| {
            let (dir, t) = grads[f as usize % grads.len()];
            let grad = t * DVec2::new(dir.cos(), dir.sin());
            let lo = offs.iter().map(|o| grad.dot(*o)).fold(f64::INFINITY, f64::min);
            Facet { pivot: l.facet_centre(f), h: base + tol.q - lo + extra[f as usize % extra.len()], grad }
        }).collect();
        // Cuts snapped to legal lines.
        let mut cuts = Vec::new();
        let lat = l.hex_lattice();
        let (u_unit, v_unit) = match lat {
            None => (l.cell_mm, l.cell_mm),
            Some(h) if !h.flat => (h.s_unit(), h.pitch()),
            Some(h) => (h.pitch(), h.s_unit()),
        };
        if let Some(f) = cut_u {
            let k = (f * l.w_mm / u_unit).round();
            if k >= 1.0 && k * u_unit < l.w_mm - 1e-6 { cuts.push(TileCut::U(k * u_unit)); }
        }
        if let Some(f) = cut_v {
            let k = (f * l.h_mm / v_unit).round();
            if k >= 1.0 && k * v_unit < l.h_mm - 1e-6 { cuts.push(TileCut::V(k * v_unit)); }
        }
        let part = build_partition(&l, &cuts).unwrap();
        let heights = quantise(&part, &facets, base, &tol).unwrap();
        // Tile rectangles.
        let mut us = vec![0.0, l.w_mm];
        let mut vs = vec![0.0, l.h_mm];
        for c in &cuts { match *c { TileCut::U(x) => us.insert(1, x), TileCut::V(y) => vs.insert(1, y) } }
        let mut total = 0usize;
        for iu in 0..us.len() - 1 {
            for iv in 0..vs.len() - 1 {
                let sub = part.pieces_in_rect(DVec2::new(us[iu], vs[iv]), DVec2::new(us[iu + 1], vs[iv + 1]));
                total += sub.len();
                let set = if cuts.is_empty() { PieceSet::All } else { PieceSet::Subset(&sub) };
                let mesh = mesh_relief(&part, set, &heights, &[], &tol, &CancelToken::new()).unwrap();
                let area = (us[iu + 1] - us[iu]) * (vs[iv + 1] - vs[iv]);
                let r = validate(&mesh, &ValidateOpts { min_facet_w: Some(base - tol.q / 2.0), back_area: Some(area), ..Default::default() });
                prop_assert!(r.ok, "tile ({iu},{iv}) of {l:?} cuts {cuts:?}:\n{}", r.summary());
            }
        }
        prop_assert_eq!(total, part.pieces.len());
    }
}

// ---------------------------------------------------------------------------
// Hex edge cases
// ---------------------------------------------------------------------------

fn hex_layout(o: HexOrientation, w: f64, h: f64, cell: f64) -> Layout {
    let p = SquamaParams { width_mm: w, height_mm: Some(h), cell_mm: cell, ..Default::default() };
    layout_for(&p, LayoutKind::Hex { orientation: o }, (1, 1)).unwrap()
}

#[test]
fn smallest_hex_plates_every_pattern_and_azimuth() {
    for o in [HexOrientation::Pointy, HexOrientation::Flat] {
        for &(w, h) in &[(1.0, 1.0), (9.0, 1.0), (1.0, 9.0), (6.0, 5.2), (12.0, 10.4)] {
            let l = hex_layout(o, w, h, 6.0);
            for az_deg in [0.0, 30.0, 60.0, 90.0, 120.0, 150.0, 180.0, 210.0, 270.0, 330.0, 17.0] {
                for pat in 0..7 {
                    let mut s = spec(l);
                    s.light = Light::Directional { az: f64::to_radians(az_deg), el: 5f64.to_radians() };
                    s.limits.max_tilt = 20f64.to_radians();
                    let noise: Vec<f32> = (0..16).map(|k| ((k * 7919) % 13) as f32 / 12.0).collect();
                    let g = pattern(pat, l.facet_count() as usize, &noise, &l);
                    let out = generate_plate(&s, &g, &CancelToken::new()).unwrap();
                    check_plate(&s, &out, QuarterTurns::Auto).unwrap_or_else(|e| panic!("{o:?} {w}x{h} az {az_deg} pat {pat}: {e}"));
                }
            }
        }
    }
}

#[test]
fn hex_minimum_cell_and_below() {
    let tol = Tol::default();
    for o in [HexOrientation::Pointy, HexOrientation::Flat] {
        let l = hex_layout(o, 12.0, 9.0, 0.7);
        let part = build_partition(&l, &[]).unwrap();
        part.check(&tol).unwrap();
        let mut s = spec(l);
        s.limits.max_tilt = 20f64.to_radians();
        let g: Vec<f32> = (0..l.facet_count()).map(|k| ((k * 31) % 5) as f32 / 4.0).collect();
        let out = generate_plate(&s, &g, &CancelToken::new()).unwrap();
        check_plate(&s, &out, QuarterTurns::Q0).unwrap();
        // Below the minimum: refused, never a broken mesh.
        let l = hex_layout(o, 12.0, 9.0, 0.69);
        assert!(matches!(generate_plate(&spec(l), &vec![0.5; l.facet_count() as usize], &CancelToken::new()), Err(Error::InvalidParams(_))));
    }
}

#[test]
fn large_hex_plate_f32_coordinates() {
    // Near the 2048 mm limit with big cells: f32 ulps are largest here.
    for o in [HexOrientation::Pointy, HexOrientation::Flat] {
        let l = hex_layout(o, 2040.0, 2000.0, 80.0);
        assert!(l.w_mm <= 2048.0 && l.h_mm <= 2048.0);
        let mut s = spec(l);
        s.light = Light::Directional { az: 0.7, el: 30f64.to_radians() };
        let g: Vec<f32> = (0..l.facet_count()).map(|k| (k % 2) as f32).collect();
        let out = generate_plate(&s, &g, &CancelToken::new()).unwrap();
        check_plate(&s, &out, QuarterTurns::Auto).unwrap();
    }
}

#[test]
fn hex_partition_shortest_edge_is_half_circumradius() {
    for o in [HexOrientation::Pointy, HexOrientation::Flat] {
        for &(w, h) in &[(30.0, 30.0), (33.0, 31.0), (100.0, 47.0)] {
            let l = hex_layout(o, w, h, 6.0);
            let part = build_partition(&l, &[]).unwrap();
            let mut shortest = f64::INFINITY;
            for pc in &part.pieces {
                let r = &part.rings[pc.ring.start as usize..pc.ring.end as usize];
                for k in 0..r.len() {
                    let (a, b) = (part.verts[r[k] as usize], part.verts[r[(k + 1) % r.len()] as usize]);
                    shortest = shortest.min((b - a).length());
                }
            }
            let r_half = 6.0 / 3f64.sqrt() / 2.0;
            assert!((shortest - r_half).abs() < 1e-9, "{o:?} {w}x{h}: shortest {shortest}");
            let area: f64 = (0..part.pieces.len()).map(|k| part.piece_area(k)).sum();
            assert!((area - l.w_mm * l.h_mm).abs() < 1e-6 * area);
        }
    }
}

#[test]
fn hex_tiles_with_seams_through_cells() {
    // Pointy: a vertical cut on an even half-cell line splits row-0 hexes in two;
    // a horizontal cut on a row line splits that row's hexes.
    let tol = Tol::default();
    for o in [HexOrientation::Pointy, HexOrientation::Flat] {
        let l = hex_layout(o, 60.0, 52.0, 6.0);
        let lat = l.hex_lattice().unwrap();
        let (cu, cv) = if o == HexOrientation::Pointy { (lat.s_unit() * 10.0, lat.pitch() * 5.0) } else { (lat.pitch() * 5.0, lat.s_unit() * 10.0) };
        let cuts = [TileCut::U(cu), TileCut::V(cv)];
        let mut s = spec(l);
        s.light = Light::Directional { az: 2.1, el: 8f64.to_radians() };
        let img = synthetic_target(300, 300);
        let g = facet_brightness(&img, &l).unwrap();
        let out = ad3d_core::squama::generate_plate_cut(&s, &g, &cuts, PieceSet::Subset(&[]), &CancelToken::new());
        assert!(out.is_err(), "empty subset is refused");
        let part = build_partition(&l, &cuts).unwrap();
        let solved = ad3d_core::squama::solve_facets_ex(&g, &l, &s.light, &s.limits, &s.response, &s.opts, Quality::Final, &tol, &CancelToken::new()).unwrap();
        let heights = quantise(&part, &solved.facets, 3.0, &tol).unwrap();
        // The whole cut partition cannot be meshed in one go (valence-4 seam
        // vertices), but every tile can.
        assert!(mesh_relief(&part, PieceSet::All, &heights, &[], &tol, &CancelToken::new()).is_err());
        let rects = [
            (DVec2::new(0.0, 0.0), DVec2::new(cu, cv)),
            (DVec2::new(cu, 0.0), DVec2::new(l.w_mm, cv)),
            (DVec2::new(0.0, cv), DVec2::new(cu, l.h_mm)),
            (DVec2::new(cu, cv), DVec2::new(l.w_mm, l.h_mm)),
        ];
        let mut seen = 0;
        for (lo, hi) in rects {
            let sub = part.pieces_in_rect(lo, hi);
            seen += sub.len();
            let tile = ad3d_core::squama::generate_plate_cut(&s, &g, &cuts, PieceSet::Subset(&sub), &CancelToken::new()).unwrap();
            let area = (hi.x - lo.x) * (hi.y - lo.y);
            assert!((tile.back_area - area).abs() < 1e-6 * area);
            let r = validate(&tile.mesh, &tile.validate_opts(Xform::IDENTITY));
            assert!(r.ok, "{o:?} tile {lo:?}: {}", r.summary());
            let b = tile.mesh.bounds().unwrap();
            assert!((b.min[0] as f64 - lo.x).abs() < 1e-3 && (b.max[0] as f64 - hi.x).abs() < 1e-3);
            assert!((b.min[1] as f64 - lo.y).abs() < 1e-3 && (b.max[1] as f64 - hi.y).abs() < 1e-3);
        }
        assert_eq!(seen, part.pieces.len());
        // A split hex keeps one plane: both pieces of a facet have equal heights at
        // their shared seam vertices (no step across the seam).
        let mut by_facet: std::collections::HashMap<u32, Vec<usize>> = Default::default();
        for (k, pc) in part.pieces.iter().enumerate() {
            by_facet.entry(pc.facet).or_default().push(k);
        }
        assert!(by_facet.values().any(|v| v.len() > 1));
        for pcs in by_facet.values().filter(|v| v.len() > 1) {
            let mut at: std::collections::HashMap<u32, i64> = Default::default();
            for &k in pcs {
                let r = part.pieces[k].ring.clone();
                for sidx in r.start..r.end {
                    let v = part.rings[sidx as usize];
                    let h = heights.corner[sidx as usize];
                    if let Some(&prev) = at.get(&v) {
                        assert_eq!(prev, h, "step across a seam inside one hex");
                    }
                    at.insert(v, h);
                }
            }
        }
    }
}

#[test]
fn point_light_and_measured_response_full_plate() {
    for kind in KINDS {
        let p = SquamaParams { width_mm: 90.0, height_mm: Some(70.0), cell_mm: 4.0, ..Default::default() };
        let l = layout_for(&p, kind, (1, 1)).unwrap();
        let img = synthetic_target(200, 160);
        let g = facet_brightness(&img, &l).unwrap();
        let mut s = spec(l);
        s.light = Light::Point { pos_mm: DVec3::new(-250.0, 35.0, 60.0) };
        s.response = Response::Measured {
            tilt_rad: [-30.0f64, -15.0, 0.0, 10.0, 20.0, 30.0].iter().map(|d| d.to_radians()).collect(),
            brightness: vec![0.02, 0.06, 0.2, 0.35, 0.55, 0.6],
        };
        let out = generate_plate(&s, &g, &CancelToken::new()).unwrap();
        assert!(out.solved.report.contrast > 0.0);
        check_plate(&s, &out, QuarterTurns::Auto).unwrap();
    }
}
