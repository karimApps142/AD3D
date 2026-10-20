//! Squama-style shadow plate (design §2–§5, M0 subset): square grid, one
//! directional light, centre pivot, Lambert inverse, `Quality::Preview` (no optimiser).

pub mod export;
pub mod layout;
pub mod light;
pub mod optimise;
pub mod parts;
pub mod response;
pub mod solve;
pub mod tiles;
pub mod calibration;

use std::f64::consts::PI;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::cancel::CancelToken;
use crate::error::{Error, Result, Warning};
use crate::image::{CellBrightness, LinearLuma, cell_brightness, synthetic_target};
use crate::mesh::Mesh;
use crate::relief::{MeshStats, PieceSet, Tol, mesh_relief_stats, quantise};

pub use layout::{
    HexOrientation, Layout, LayoutKind, TileCut, build_partition, cell_e_max, e_max, facet_brightness, layout, layout_for,
    min_cell_mm,
};
use layout::{snap, to_um};
pub use light::{CellLight, Light};
pub use response::{Response, ResponseCurve, tilt_from_brightness};
pub use solve::{
    Limits, PivotRule, PlateOutput, PlateSpec, Quality, SolveOpts, SolveReport, Solved, generate_plate, generate_plate_cut,
    solve_facets, solve_facets_ex,
};

/// Generator parameters, millimetres and radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SquamaParams {
    pub width_mm: f64,
    /// When set, the plate height is snapped from this and the image is centre-cropped
    /// to fill; otherwise the height follows the image aspect.
    pub height_mm: Option<f64>,
    pub cell_mm: f64,
    /// Direction the light comes from, in the plate plane (LuBan Left = π).
    pub light_azimuth: f64,
    /// Grazing angle above the plate plane.
    pub light_elevation: f64,
    /// Largest facet tilt.
    pub max_tilt: f64,
    /// Minimum material behind any facet.
    pub base_mm: f64,
}

impl Default for SquamaParams {
    /// LuBan defaults: width 300, cell 6, light from the Left, elevation 5°,
    /// surface angle 10°, thickness 3.
    fn default() -> Self {
        SquamaParams {
            width_mm: 300.0,
            height_mm: None,
            cell_mm: 6.0,
            light_azimuth: PI,
            light_elevation: 5f64.to_radians(),
            max_tilt: 10f64.to_radians(),
            base_mm: 3.0,
        }
    }
}

/// The JS → Rust DTO (v1, camelCase, degrees). `image` is `"synthetic"` or a path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SquamaParamsDto {
    pub job_id: u32,
    pub width_mm: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_mm: Option<f64>,
    pub cell_mm: f64,
    pub light_azimuth_deg: f64,
    pub light_elevation_deg: f64,
    pub max_tilt_deg: f64,
    pub base_mm: f64,
    pub image: String,
}

impl SquamaParamsDto {
    pub fn params(&self) -> SquamaParams {
        SquamaParams {
            width_mm: self.width_mm,
            height_mm: self.height_mm,
            cell_mm: self.cell_mm,
            light_azimuth: self.light_azimuth_deg.to_radians(),
            light_elevation: self.light_elevation_deg.to_radians(),
            max_tilt: self.max_tilt_deg.to_radians(),
            base_mm: self.base_mm,
        }
    }
    pub fn is_synthetic(&self) -> bool {
        self.image == "synthetic"
    }
}

/// Generator options that are not user parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GenOptions {
    pub tol: Tol,
    /// Refuse reliefs below 3δ (§2). Tests disable it to reach cell 0.5 mm.
    pub relief_guard: bool,
}

impl Default for GenOptions {
    fn default() -> Self {
        GenOptions { tol: Tol::default(), relief_guard: true }
    }
}

pub(crate) fn check_params(p: &SquamaParams, opts: &GenOptions) -> Result<(SquamaParams, Vec<Warning>)> {
    let mut w = Vec::new();
    let mut p = *p;
    let finite = [p.width_mm, p.cell_mm, p.light_azimuth, p.light_elevation, p.max_tilt, p.base_mm]
        .iter()
        .all(|x| x.is_finite())
        && p.height_mm.is_none_or(|h| h.is_finite());
    if !finite {
        return Err(Error::InvalidParams("non-finite parameter".into()));
    }
    let min_cell = 10.0 * opts.tol.eps;
    if p.cell_mm < min_cell - 1e-12 {
        return Err(Error::InvalidParams(format!("cell {} mm below {min_cell} mm", p.cell_mm)));
    }
    if p.width_mm < p.cell_mm || p.width_mm > 2048.0 {
        return Err(Error::InvalidParams(format!("width {} mm outside [cell, 2048]", p.width_mm)));
    }
    if let Some(h) = p.height_mm {
        if h < p.cell_mm || h > 2048.0 {
            return Err(Error::InvalidParams(format!("height {h} mm outside [cell, 2048]")));
        }
    }
    let deg = |x: f64| x.to_degrees();
    if p.light_elevation < 0.0 || deg(p.light_elevation) > 45.0 + 1e-9 {
        return Err(Error::InvalidParams(format!("elevation {:.3}° outside [1°, 45°]", deg(p.light_elevation))));
    }
    if deg(p.light_elevation) < 1.0 {
        p.light_elevation = 1f64.to_radians();
        w.push(Warning { code: "elevation_raised", message: "light elevation raised to 1°".into() });
    }
    if deg(p.max_tilt) < 1.0 - 1e-9 || deg(p.max_tilt) > 20.0 + 1e-9 {
        return Err(Error::InvalidParams(format!("max tilt {:.3}° outside [1°, 20°]", deg(p.max_tilt))));
    }
    if p.base_mm < 0.4 - 1e-12 {
        return Err(Error::InvalidParams(format!("base {} mm below 0.4 mm", p.base_mm)));
    }
    let relief = 2.0 * e_max(p.cell_mm, p.light_azimuth) * p.max_tilt.tan();
    if relief < 3.0 * opts.tol.delta && opts.relief_guard {
        return Err(Error::InvalidParams(format!(
            "relief {relief:.3} mm below {:.2} mm; raise max tilt or cell size",
            3.0 * opts.tol.delta
        )));
    }
    if relief < 8.0 * opts.tol.delta {
        w.push(Warning {
            code: "low_relief",
            message: format!("relief {relief:.3} mm below printable detail; raise max tilt or cell size"),
        });
    }
    if p.cell_mm < 3.0 {
        w.push(Warning { code: "small_cell", message: "cells under 3 mm may not show (unconfirmed)".into() });
    }
    if deg(p.light_elevation) < 3.0 {
        w.push(Warning { code: "low_elevation", message: "light elevation below 3°".into() });
    }
    Ok((p, w))
}

/// Timings (ms) and counts from one run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct SquamaStats {
    pub sample_ms: f64,
    pub solve_ms: f64,
    pub quantise_ms: f64,
    pub mesh_ms: f64,
    pub total_ms: f64,
    pub mesh: MeshStats,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SquamaOutput {
    pub mesh: Mesh,
    pub layout: Layout,
    pub warnings: Vec<Warning>,
    pub stats: SquamaStats,
    /// Lowest allowed facet height, for `validate()` (`T − q/2`).
    pub min_facet_w: f64,
    /// Outline area for `validate()`.
    pub back_area: f64,
}

/// Where cell brightness comes from.
#[derive(Debug, Clone, Copy)]
pub enum ImageSource<'a> {
    /// The built-in test target (500×630 aspect).
    Synthetic,
    Luma(&'a LinearLuma),
}

/// Resolution of the synthetic target. Its 500:630 aspect gives 50×63 at the
/// LuBan defaults and 100×126 at cell 3.
pub const SYNTHETIC_PX: (u32, u32) = (500, 630);

/// Whole pipeline: layout → cell brightness → facets → heights → mesh.
pub fn run(p: &SquamaParams, src: ImageSource<'_>, opts: &GenOptions, cancel: &CancelToken) -> Result<SquamaOutput> {
    let t0 = Instant::now();
    let synth;
    let img = match src {
        ImageSource::Synthetic => {
            synth = synthetic_target(SYNTHETIC_PX.0, SYNTHETIC_PX.1);
            &synth
        }
        ImageSource::Luma(l) => l,
    };
    let (pc, _) = check_params(p, opts)?;
    let l = layout(&pc, (img.w, img.h))?;
    let cb = cell_brightness(img, l.nx, l.ny, l.crop)?;
    let sample_ms = t0.elapsed().as_secs_f64() * 1e3;
    cancel.check()?;
    let mut out = generate_ex(p, &cb, opts, cancel)?;
    out.stats.sample_ms = sample_ms;
    out.stats.total_ms = t0.elapsed().as_secs_f64() * 1e3;
    Ok(out)
}

/// Pure generator: `(params, cell brightness) → mesh`. The grid is `cb.nx × cb.ny`;
/// `cb.nx` must equal the snapped width in cells.
pub fn generate(p: &SquamaParams, cb: &CellBrightness, cancel: &CancelToken) -> Result<Mesh> {
    generate_ex(p, cb, &GenOptions::default(), cancel).map(|o| o.mesh)
}

/// [`generate`] with options, warnings and timings.
pub fn generate_ex(p: &SquamaParams, cb: &CellBrightness, opts: &GenOptions, cancel: &CancelToken) -> Result<SquamaOutput> {
    let t0 = Instant::now();
    let (p, warnings) = check_params(p, opts)?;
    let cell_um = to_um(p.cell_mm);
    let nx = snap(to_um(p.width_mm), cell_um);
    if cb.nx != nx || cb.ny == 0 || cb.g.len() != (cb.nx * cb.ny) as usize {
        return Err(Error::InvalidParams(format!(
            "brightness grid {}×{} does not match the snapped width ({nx} cells)",
            cb.nx, cb.ny
        )));
    }
    let cell = cell_um as f64 / 1000.0;
    let l = Layout {
        kind: LayoutKind::Square,
        w_mm: nx as f64 * cell,
        h_mm: cb.ny as f64 * cell,
        cell_mm: cell,
        nx,
        ny: cb.ny,
        crop: None,
    };
    if l.h_mm > 2048.0 + 1e-9 {
        return Err(Error::InvalidParams(format!("plate height {} mm above 2048 mm", l.h_mm)));
    }
    let tol = opts.tol;
    let facets = solve_facets(cb, &l, &p, &tol);
    let t1 = Instant::now();
    let part = build_partition(&l, &[])?;
    let heights = quantise(&part, &facets, p.base_mm, &tol)?;
    let t2 = Instant::now();
    cancel.check()?;
    let (mesh, mstats) = mesh_relief_stats(&part, PieceSet::All, &heights, &[], &tol, cancel)?;
    let t3 = Instant::now();
    let ms = |a: Instant, b: Instant| (b - a).as_secs_f64() * 1e3;
    Ok(SquamaOutput {
        mesh,
        layout: l,
        warnings,
        stats: SquamaStats {
            sample_ms: 0.0,
            solve_ms: ms(t0, t1),
            quantise_ms: ms(t1, t2),
            mesh_ms: ms(t2, t3),
            total_ms: ms(t0, t3),
            mesh: mstats,
        },
        min_facet_w: p.base_mm - tol.q / 2.0,
        back_area: l.w_mm * l.h_mm,
    })
}

/// `validate()` options matching a generated plate (plate space, given transform).
pub fn validate_opts(out: &SquamaOutput, transform: crate::orient::Xform) -> crate::mesh::validate::ValidateOpts {
    crate::mesh::validate::ValidateOpts {
        transform,
        min_facet_w: Some(out.min_facet_w),
        back_area: Some(out.back_area),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn luban_snapping() {
        let mut p = SquamaParams::default();
        p.width_mm = 200.0;
        assert_eq!(layout(&p, (1, 1)).unwrap().w_mm, 198.0);
        p.width_mm = 202.0;
        assert_eq!(layout(&p, (1, 1)).unwrap().w_mm, 204.0);
        p.width_mm = 300.0;
        let l = layout(&p, SYNTHETIC_PX).unwrap();
        assert_eq!((l.nx, l.ny), (50, 63));
        p.cell_mm = 3.0;
        let l = layout(&p, SYNTHETIC_PX).unwrap();
        assert_eq!((l.nx, l.ny), (100, 126));
    }

    #[test]
    fn decimal_tie_snaps_half_up() {
        // 100.8 / 3.2 = 31.5 exactly in decimal; f64 says 31.4999...
        let p = SquamaParams { width_mm: 100.8, cell_mm: 3.2, ..Default::default() };
        assert_eq!(layout(&p, (1, 1)).unwrap().nx, 32);
    }

    #[test]
    fn lambert_inverse_endpoints() {
        let (a, s) = (5f64.to_radians(), 10f64.to_radians());
        assert!((tilt_from_brightness(1.0, a, s) - s).abs() < 1e-12);
        assert!((tilt_from_brightness(0.0, a, s) + a).abs() < 1e-12); // E=0 → θ = −α
        assert_eq!(tilt_from_brightness(f64::NAN, a, s), tilt_from_brightness(0.0, a, s));
    }
}
