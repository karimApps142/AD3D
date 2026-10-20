//! Calibration tile (design §15.3, FEATURES PROJ-SQUAMA-CAL).
//!
//! A small Shadow Plate made of uniform patches, one tilt each, ramping from a
//! self-shadowed tilt (`−max(S, α + 2°)`) to `+S`: printed with the plate's own
//! settings and photographed under the owner's lamp, it gives a measured
//! tilt → brightness table ([`fit_response`]) that replaces Lambert.
//!
//! Layout (plate space, square cells, the plate's centre-pivot rule):
//! - patches of `across × along` cells ("along" = the axis closer to the light
//!   direction), laid out row-major from the bottom-left; the first (darkest) step
//!   sits in one corner and the last (brightest) step in the opposite corner, so
//!   they double as black/white fiducials;
//! - a one-cell border at θ = 0 around the patches, except on the light side,
//!   where a marker strip is bright (θ = max) with a dark notch (θ = min) over
//!   the third nearest the first patch: it shows which side faced the lamp and
//!   which way round the tile is;
//! - sized to fit `max_size_mm` (default 120 × 90): patches shrink to 3 × 3
//!   cells before the call fails.
//!
//! The generator is pure: [`calibration_facets`] returns the §15.3 triple
//! (partition, facets, key); [`calibration_tile`] meshes it with the same relief
//! mesher and quantiser as the plate.
//!
//! `squama::export::calibration_spec` builds the spec for an M1 job (point lights
//! use the light as seen from the plate centre). Not done yet: step numbers and
//! an UP arrow engraved on the back (§14.3 back regions exist now).

use std::f64::consts::PI;

use glam::DVec2;
use serde::{Deserialize, Serialize};

use super::{GenOptions, SquamaParams, e_max};
use crate::cancel::CancelToken;
use crate::error::{Error, Result, Warning};
use crate::image::LinearLuma;
use crate::mesh::Mesh;
use crate::mesh::validate::ValidateOpts;
use crate::orient::Xform;
use crate::relief::{Facet, Partition, PieceSet, Tol, mesh_relief, quantise, square_grid};

/// What to print. Millimetres and radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalibrationSpec {
    /// Number of tilt steps (2–64).
    pub steps: u32,
    /// Patch columns; 0 picks the most compact layout that fits.
    pub cols: u32,
    /// Patch size in cells across and along the light (shrunk to fit, minimum 3).
    pub patch_across: u32,
    pub patch_along: u32,
    pub cell_mm: f64,
    pub base_mm: f64,
    pub light_azimuth: f64,
    pub light_elevation: f64,
    /// The plate's surface angle S (sets the default ramp end).
    pub max_tilt: f64,
    /// Ramp ends; default `−max(S, α + 2°)` (clamped to −30°) and `+S`.
    pub tilt_min: Option<f64>,
    pub tilt_max: Option<f64>,
    /// Light-side marker strip.
    pub marker: bool,
    /// Largest tile, plate-space width × height.
    pub max_size_mm: [f64; 2],
}

impl Default for CalibrationSpec {
    fn default() -> Self {
        CalibrationSpec::from_params(&SquamaParams::default())
    }
}

impl CalibrationSpec {
    /// The tile for a plate: same cell, base, light and surface angle.
    pub fn from_params(p: &SquamaParams) -> Self {
        CalibrationSpec {
            steps: 12,
            cols: 0,
            patch_across: 4,
            patch_along: 6,
            cell_mm: p.cell_mm,
            base_mm: p.base_mm,
            light_azimuth: p.light_azimuth,
            light_elevation: p.light_elevation,
            max_tilt: p.max_tilt,
            tilt_min: None,
            tilt_max: None,
            marker: true,
            max_size_mm: [120.0, 90.0],
        }
    }

    /// The tilt ramp ends actually used.
    pub fn tilt_range(&self) -> (f64, f64) {
        let lo = self
            .tilt_min
            .unwrap_or_else(|| -(self.max_tilt.max(self.light_elevation + 2f64.to_radians())).min(30f64.to_radians()));
        let hi = self.tilt_max.unwrap_or(self.max_tilt);
        (lo, hi)
    }
}

/// Which tile edge faced the light.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
    Bottom,
    Top,
}

/// One patch on the tile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchKey {
    /// Ramp step, 0 = lowest tilt; `None` for a filler patch at θ = 0.
    pub step: Option<u32>,
    pub tilt_rad: f64,
    pub tilt_deg: f64,
    /// Lambert prediction relative to a flat (θ = 0) patch: `max(0, sin(α + θ)) / sin α`.
    pub lambert_rel: f64,
    /// The linear grey this tilt reproduces under the plate's Lambert mapping (§3), clamped to [0, 1].
    pub grey: f64,
    /// Cells `[i0, j0, i1, j1)`.
    pub cells: [u32; 4],
    /// Plate-space rectangle `[u0, v0, u1, v1]` (mm).
    pub rect_mm: [f64; 4],
    /// The rectangle inset by one cell on each side where possible: measure here.
    pub measure_mm: [f64; 4],
}

/// Everything needed to read a photo of the printed tile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationKey {
    pub w_mm: f64,
    pub h_mm: f64,
    pub cell_mm: f64,
    pub nx: u32,
    pub ny: u32,
    pub light_azimuth_deg: f64,
    pub light_elevation_deg: f64,
    pub light_side: Side,
    pub tilt_min_deg: f64,
    pub tilt_max_deg: f64,
    /// Ramp patches in step order, then fillers.
    pub patches: Vec<PatchKey>,
}

impl CalibrationKey {
    pub fn steps(&self) -> usize {
        self.patches.iter().filter(|p| p.step.is_some()).count()
    }
}

/// A meshed calibration tile, ready for `validate()` and export.
#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationTile {
    pub mesh: Mesh,
    pub key: CalibrationKey,
    /// Lowest allowed facet height for `validate()` (`T − q/2`).
    pub min_facet_w: f64,
    pub back_area: f64,
    pub warnings: Vec<Warning>,
}

impl CalibrationTile {
    /// `validate()` options for this tile under an export transform.
    pub fn validate_opts(&self, transform: Xform) -> ValidateOpts {
        ValidateOpts { transform, min_facet_w: Some(self.min_facet_w), back_area: Some(self.back_area), ..Default::default() }
    }
}

struct Plan {
    nx: u32,
    ny: u32,
    cols: u32,
    rows: u32,
    pw: u32,
    ph: u32,
    along_x: bool,
}

fn light_side(az: f64) -> (Side, bool) {
    let (c, s) = (az.cos(), az.sin());
    if c.abs() >= s.abs() {
        (if c < 0.0 { Side::Left } else { Side::Right }, true)
    } else {
        (if s > 0.0 { Side::Top } else { Side::Bottom }, false)
    }
}

fn plan(spec: &CalibrationSpec) -> Result<Plan> {
    let (_, along_x) = light_side(spec.light_azimuth);
    let cell = spec.cell_mm;
    let fits = |nx: u32, ny: u32| {
        nx as f64 * cell <= spec.max_size_mm[0] + 1e-9 && ny as f64 * cell <= spec.max_size_mm[1] + 1e-9
    };
    let n = spec.steps;
    let col_choices: Vec<u32> = if spec.cols > 0 { vec![spec.cols.min(n)] } else { (1..=n).collect() };
    // Largest patches first; at each size the squarest layout that fits.
    let mut sizes = Vec::new();
    for along in (3..=spec.patch_along.max(3)).rev() {
        for across in (3..=spec.patch_across.max(3)).rev() {
            sizes.push((along, across));
        }
    }
    sizes.sort_by(|a, b| (b.0 * b.1).cmp(&(a.0 * a.1)).then(b.1.cmp(&a.1)));
    for (along, across) in sizes {
        let (pw, ph) = if along_x { (along, across) } else { (across, along) };
        let mut best: Option<(f64, Plan)> = None;
        for &cols in &col_choices {
            let rows = n.div_ceil(cols);
            let (nx, ny) = (2 + cols * pw, 2 + rows * ph);
            if !fits(nx, ny) {
                continue;
            }
            let waste = (cols * rows - n) as f64;
            let score = (nx as f64 - ny as f64).abs() * cell + waste * 1e3;
            if best.as_ref().is_none_or(|(s, _)| score < *s) {
                best = Some((score, Plan { nx, ny, cols, rows, pw, ph, along_x }));
            }
        }
        if let Some((_, p)) = best {
            return Ok(p);
        }
    }
    Err(Error::InvalidParams(format!(
        "{} steps of at least 3×3 cells of {} mm do not fit in {} × {} mm; use fewer steps, smaller cells or a larger tile",
        n, cell, spec.max_size_mm[0], spec.max_size_mm[1]
    )))
}

fn check(spec: &CalibrationSpec, tol: &Tol) -> Result<(f64, f64)> {
    let deg = |x: f64| x.to_degrees();
    let finite = [spec.cell_mm, spec.base_mm, spec.light_azimuth, spec.light_elevation, spec.max_tilt]
        .iter()
        .chain(spec.max_size_mm.iter())
        .all(|x| x.is_finite());
    if !finite {
        return Err(Error::InvalidParams("non-finite calibration parameter".into()));
    }
    if !(2..=64).contains(&spec.steps) {
        return Err(Error::InvalidParams(format!("calibration steps {} outside [2, 64]", spec.steps)));
    }
    if spec.cell_mm < 10.0 * tol.eps - 1e-12 || spec.cell_mm > 20.0 {
        return Err(Error::InvalidParams(format!("calibration cell {} mm outside [{}, 20]", spec.cell_mm, 10.0 * tol.eps)));
    }
    if spec.base_mm < 0.4 - 1e-12 || spec.base_mm > 20.0 {
        return Err(Error::InvalidParams(format!("base {} mm outside [0.4, 20]", spec.base_mm)));
    }
    if deg(spec.light_elevation) < 1.0 - 1e-9 || deg(spec.light_elevation) > 45.0 + 1e-9 {
        return Err(Error::InvalidParams(format!("elevation {:.3}° outside [1°, 45°]", deg(spec.light_elevation))));
    }
    let (lo, hi) = spec.tilt_range();
    if !(lo.is_finite() && hi.is_finite()) || lo >= hi || deg(lo) < -30.0 - 1e-9 || deg(hi) > 30.0 + 1e-9 {
        return Err(Error::InvalidParams(format!(
            "tilt range {:.2}°..{:.2}° must be increasing and within ±30°",
            deg(lo),
            deg(hi)
        )));
    }
    Ok((lo, hi))
}

/// §15.3: partition, one facet per cell and the key. Pure; no meshing.
pub fn calibration_facets(spec: &CalibrationSpec, tol: &Tol) -> Result<(Partition, Vec<Facet>, CalibrationKey)> {
    let (lo, hi) = check(spec, tol)?;
    let pl = plan(spec)?;
    let (side, _) = light_side(spec.light_azimuth);
    let cell = (spec.cell_mm * 1000.0).round() / 1000.0;
    let n = spec.steps;
    let step_tilt = |k: u32| lo + (hi - lo) * k as f64 / (n - 1) as f64;
    let mut tilt = vec![0.0f64; (pl.nx * pl.ny) as usize];
    let at = |i: u32, j: u32| (j * pl.nx + i) as usize;
    let (a, s_max) = (spec.light_elevation, spec.max_tilt);
    let lambert_rel = |t: f64| (a + t).sin().max(0.0) / a.sin();
    let grey = |t: f64| {
        let (e0, e1) = ((a - s_max).sin().max(0.0), (a + s_max).sin());
        (((a + t).sin().max(0.0) - e0) / (e1 - e0)).clamp(0.0, 1.0)
    };
    let mut patches = Vec::with_capacity((pl.cols * pl.rows) as usize);
    let slots = pl.cols * pl.rows;
    for slot in 0..slots {
        let step = if slot + 1 == slots {
            Some(n - 1)
        } else if slot < n - 1 {
            Some(slot)
        } else {
            None
        };
        let t = step.map_or(0.0, step_tilt);
        let (c, r) = (slot % pl.cols, slot / pl.cols);
        let (i0, j0) = (1 + c * pl.pw, 1 + r * pl.ph);
        let (i1, j1) = (i0 + pl.pw, j0 + pl.ph);
        for j in j0..j1 {
            for i in i0..i1 {
                tilt[at(i, j)] = t;
            }
        }
        let rect = [i0 as f64 * cell, j0 as f64 * cell, i1 as f64 * cell, j1 as f64 * cell];
        let inset = |lo: u32, hi: u32| if hi - lo >= 3 { 1.0 } else { 0.0 };
        let (ix, iy) = (inset(i0, i1) * cell, inset(j0, j1) * cell);
        patches.push(PatchKey {
            step,
            tilt_rad: t,
            tilt_deg: t.to_degrees(),
            lambert_rel: lambert_rel(t),
            grey: grey(t),
            cells: [i0, j0, i1, j1],
            rect_mm: rect,
            measure_mm: [rect[0] + ix, rect[1] + iy, rect[2] - ix, rect[3] - iy],
        });
    }
    if spec.marker {
        // Border cells on the light side, counted from the end nearer patch 0 (bottom-left).
        let edge: Vec<(u32, u32)> = match side {
            Side::Left => (0..pl.ny).map(|j| (0, j)).collect(),
            Side::Right => (0..pl.ny).map(|j| (pl.nx - 1, j)).collect(),
            Side::Bottom => (0..pl.nx).map(|i| (i, 0)).collect(),
            Side::Top => (0..pl.nx).map(|i| (i, pl.ny - 1)).collect(),
        };
        let notch = (edge.len() as u32).div_ceil(3);
        for (t, &(i, j)) in edge.iter().enumerate() {
            tilt[at(i, j)] = if (t as u32) < notch { lo } else { hi };
        }
    }
    patches.sort_by_key(|p| p.step.unwrap_or(u32::MAX));
    let part = square_grid(pl.nx, pl.ny, cell, DVec2::ZERO);
    let dir = DVec2::new(spec.light_azimuth.cos(), spec.light_azimuth.sin());
    let tmax = lo.abs().max(hi.abs());
    let c = spec.base_mm + e_max(cell, spec.light_azimuth) * tmax.tan() + tol.q;
    let mut facets = Vec::with_capacity(tilt.len());
    for j in 0..pl.ny {
        for i in 0..pl.nx {
            let th = tilt[at(i, j)];
            let pivot = DVec2::new((i as f64 + 0.5) * cell, (j as f64 + 0.5) * cell);
            facets.push(Facet { pivot, h: c, grad: -th.tan() * dir });
        }
    }
    let key = CalibrationKey {
        w_mm: pl.nx as f64 * cell,
        h_mm: pl.ny as f64 * cell,
        cell_mm: cell,
        nx: pl.nx,
        ny: pl.ny,
        light_azimuth_deg: spec.light_azimuth.to_degrees().rem_euclid(360.0),
        light_elevation_deg: spec.light_elevation.to_degrees(),
        light_side: side,
        tilt_min_deg: lo.to_degrees(),
        tilt_max_deg: hi.to_degrees(),
        patches,
    };
    let _ = pl.along_x;
    Ok((part, facets, key))
}

/// Builds and meshes the calibration tile (plate space, back at `w = 0`).
pub fn calibration_tile(spec: &CalibrationSpec, opts: &GenOptions, cancel: &CancelToken) -> Result<CalibrationTile> {
    let tol = opts.tol;
    let (part, facets, key) = calibration_facets(spec, &tol)?;
    let (lo, hi) = (key.tilt_min_deg.to_radians(), key.tilt_max_deg.to_radians());
    let relief = 2.0 * e_max(key.cell_mm, spec.light_azimuth) * lo.abs().max(hi.abs()).tan();
    if relief < 3.0 * tol.delta && opts.relief_guard {
        return Err(Error::InvalidParams(format!("calibration relief {relief:.3} mm below {:.2} mm", 3.0 * tol.delta)));
    }
    let mut warnings = Vec::new();
    if relief < 8.0 * tol.delta {
        warnings.push(Warning { code: "low_relief", message: format!("relief {relief:.3} mm below printable detail") });
    }
    if lo > -spec.light_elevation {
        warnings.push(Warning {
            code: "no_shadow_patch",
            message: "lowest tilt does not self-shadow; the ambient level will not be measured".into(),
        });
    }
    cancel.check()?;
    let heights = quantise(&part, &facets, spec.base_mm, &tol)?;
    cancel.check()?;
    let mesh = mesh_relief(&part, PieceSet::All, &heights, &[], &tol, cancel)?;
    Ok(CalibrationTile {
        mesh,
        back_area: key.w_mm * key.h_mm,
        key,
        min_facet_w: spec.base_mm - tol.q / 2.0,
        warnings,
    })
}

/// Synthetic grey ramp (linear luminance), `steps` vertical bands from black on
/// the left to white on the right. Running it through the normal plate
/// pipeline gives the "mapping strip" that shows the tone mapping in print.
pub fn grey_ramp_target(w: u32, h: u32, steps: u32) -> LinearLuma {
    let steps = steps.max(2);
    let w = w.max(steps);
    let h = h.max(1);
    let mut px = Vec::with_capacity((w * h) as usize);
    for _ in 0..h {
        for x in 0..w {
            let k = (x as u64 * steps as u64 / w as u64) as u32;
            px.push(k as f32 / (steps - 1) as f32);
        }
    }
    LinearLuma { w, h, px }
}

/// A measured tilt → brightness table (design §15.2 `Response::Measured`);
/// `Response::from(&measured)` converts it, and `to_dto` gives the §12.1 JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeasuredResponse {
    /// Increasing tilts (radians).
    pub tilt_rad: Vec<f64>,
    /// Non-decreasing brightness relative to a flat (θ = 0) patch.
    pub brightness: Vec<f64>,
    /// Darkest level (self-shadowed patches see only room light).
    pub ambient: f64,
    /// Brightest / darkest.
    pub contrast: f64,
}

impl From<&MeasuredResponse> for super::Response {
    fn from(m: &MeasuredResponse) -> Self {
        super::Response::Measured { tilt_rad: m.tilt_rad.clone(), brightness: m.brightness.clone() }
    }
}

impl MeasuredResponse {
    /// §12.1 `response` DTO: `{ kind: 'measured', tiltDeg, brightness }`.
    pub fn to_dto(&self) -> serde_json::Value {
        serde_json::json!({
            "kind": "measured",
            "tiltDeg": self.tilt_rad.iter().map(|t| t.to_degrees()).collect::<Vec<_>>(),
            "brightness": self.brightness,
        })
    }

    /// Tilt that gives relative brightness `b` (clamped to the table; on a flat
    /// stretch the lowest tilt that reaches it).
    pub fn inverse(&self, b: f64) -> f64 {
        let (t, y) = (&self.tilt_rad, &self.brightness);
        if !b.is_finite() || b <= y[0] {
            return t[0];
        }
        for k in 1..t.len() {
            if b <= y[k] {
                let d = y[k] - y[k - 1];
                return if d <= 0.0 { t[k - 1] } else { t[k - 1] + (t[k] - t[k - 1]) * (b - y[k - 1]) / d };
            }
        }
        *t.last().unwrap()
    }
}

/// Pool-adjacent-violators: the non-decreasing least-squares fit of `y`.
pub fn isotonic(y: &[f64]) -> Vec<f64> {
    let mut blocks: Vec<(f64, usize)> = Vec::with_capacity(y.len());
    for &v in y {
        blocks.push((v, 1));
        while blocks.len() >= 2 {
            let (b, nb) = blocks[blocks.len() - 1];
            let (a, na) = blocks[blocks.len() - 2];
            if a <= b {
                break;
            }
            blocks.pop();
            let n = na + nb;
            *blocks.last_mut().unwrap() = ((a * na as f64 + b * nb as f64) / n as f64, n);
        }
    }
    blocks.into_iter().flat_map(|(v, n)| std::iter::repeat_n(v, n)).collect()
}

/// Largest dip a measurement may show before the response is called
/// non-monotonic, as a fraction of the measured range.
pub const MONOTONE_TOLERANCE: f64 = 0.05;

/// §15.3: fits a response from one brightness per ramp step (step order, any
/// linear unit, e.g. patch medians of a linear photo). Rejects data that are not
/// increasing within [`MONOTONE_TOLERANCE`] instead of fitting them.
pub fn fit_response(key: &CalibrationKey, measured: &[f64]) -> Result<MeasuredResponse> {
    let steps: Vec<&PatchKey> = key.patches.iter().filter(|p| p.step.is_some()).collect();
    if measured.len() != steps.len() {
        return Err(Error::InvalidParams(format!("expected {} measurements (one per step), got {}", steps.len(), measured.len())));
    }
    if measured.iter().any(|m| !m.is_finite() || *m < 0.0) {
        return Err(Error::InvalidParams("measurements must be finite and ≥ 0".into()));
    }
    let mut pts: Vec<(f64, f64)> = steps.iter().zip(measured).map(|(p, &m)| (p.tilt_rad, m)).collect();
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (lo, hi) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), p| (l.min(p.1), h.max(p.1)));
    if hi <= 0.0 || hi - lo <= 1e-9 * hi.max(1.0) {
        return Err(Error::InvalidParams("measurements show no contrast".into()));
    }
    let ys: Vec<f64> = pts.iter().map(|p| p.1).collect();
    let fit = isotonic(&ys);
    let worst = ys.iter().zip(&fit).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
    if worst > MONOTONE_TOLERANCE * (hi - lo) || fit[fit.len() - 1] <= fit[0] {
        return Err(Error::InvalidParams(format!(
            "measured brightness is not increasing with tilt (largest dip {:.1}% of the range); check the photo (exposure locked, tile the right way round, light side marked)",
            100.0 * worst / (hi - lo)
        )));
    }
    let ts: Vec<f64> = pts.iter().map(|p| p.0).collect();
    // Normalise to θ = 0 (interpolated); fall back to the brightest patch.
    let at0 = if ts[0] <= 0.0 && *ts.last().unwrap() >= 0.0 {
        let k = ts.iter().position(|&t| t >= 0.0).unwrap();
        if k == 0 || ts[k] == 0.0 { fit[k] } else { fit[k - 1] + (fit[k] - fit[k - 1]) * (0.0 - ts[k - 1]) / (ts[k] - ts[k - 1]) }
    } else {
        *fit.last().unwrap()
    };
    let norm = if at0 > 0.0 { at0 } else { *fit.last().unwrap() };
    let brightness: Vec<f64> = fit.iter().map(|v| v / norm).collect();
    let ambient = brightness[0];
    let top = *brightness.last().unwrap();
    let contrast = if ambient > 0.0 { top / ambient } else { f64::INFINITY };
    Ok(MeasuredResponse { tilt_rad: ts, brightness, ambient, contrast })
}

/// Lambert prediction for the key's patches (for tests and a "what to expect" readout).
pub fn lambert_prediction(key: &CalibrationKey) -> Vec<f64> {
    key.patches.iter().filter(|p| p.step.is_some()).map(|p| p.lambert_rel).collect()
}

/// Azimuth (radians) wrapped to `[0, 2π)`.
pub fn wrap_azimuth(a: f64) -> f64 {
    a.rem_euclid(2.0 * PI)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::validate::validate;

    #[test]
    fn default_tile_fits_and_validates() {
        let spec = CalibrationSpec::default();
        let t = calibration_tile(&spec, &GenOptions::default(), &CancelToken::new()).unwrap();
        assert!(t.key.w_mm <= 120.0 && t.key.h_mm <= 90.0, "{} × {}", t.key.w_mm, t.key.h_mm);
        assert_eq!(t.key.steps(), 12);
        assert_eq!(t.key.light_side, Side::Left);
        let rep = validate(&t.mesh, &t.validate_opts(Xform::IDENTITY));
        assert!(rep.ok, "{}", rep.summary());
        // Steps ramp monotonically from −max(S, α+2°) to +S.
        let tilts: Vec<f64> = t.key.patches.iter().filter_map(|p| p.step.map(|_| p.tilt_deg)).collect();
        assert!((tilts[0] + 10.0).abs() < 1e-9 && (tilts[11] - 10.0).abs() < 1e-9);
        assert!(tilts.windows(2).all(|w| w[1] > w[0]));
    }

    #[test]
    fn first_and_last_steps_are_in_opposite_corners() {
        let (_, _, key) = calibration_facets(&CalibrationSpec::default(), &Tol::default()).unwrap();
        let first = key.patches.iter().find(|p| p.step == Some(0)).unwrap();
        let last = key.patches.iter().find(|p| p.step == Some(key.steps() as u32 - 1)).unwrap();
        assert_eq!(&first.cells[..2], &[1, 1]);
        assert_eq!(&last.cells[2..], &[key.nx - 1, key.ny - 1]);
    }

    #[test]
    fn too_many_steps_is_refused() {
        let spec = CalibrationSpec { steps: 64, max_size_mm: [60.0, 60.0], ..Default::default() };
        assert!(calibration_tile(&spec, &GenOptions::default(), &CancelToken::new()).is_err());
    }

    #[test]
    fn fit_recovers_lambert_and_rejects_noise() {
        let (_, _, key) = calibration_facets(&CalibrationSpec::default(), &Tol::default()).unwrap();
        let ambient = 0.05;
        let m: Vec<f64> = lambert_prediction(&key).iter().map(|v| v + ambient).collect();
        let r = fit_response(&key, &m).unwrap();
        assert!(r.brightness.windows(2).all(|w| w[1] >= w[0]));
        assert!((r.brightness.iter().cloned().fold(0.0, f64::max) - r.brightness[11]).abs() < 1e-12);
        assert!(r.ambient > 0.0 && r.contrast > 1.0);
        // inverse() round-trips inside the increasing part.
        let t = r.tilt_rad[8];
        assert!((r.inverse(r.brightness[8]) - t).abs() < 1e-9);
        // A reversed table is rejected, not fitted.
        let rev: Vec<f64> = m.iter().rev().cloned().collect();
        assert!(fit_response(&key, &rev).is_err());
        assert!(fit_response(&key, &m[..5]).is_err());
        assert_eq!(r.to_dto()["kind"], "measured");
    }

    #[test]
    fn isotonic_pools_violators() {
        assert_eq!(isotonic(&[1.0, 3.0, 2.0, 4.0]), vec![1.0, 2.5, 2.5, 4.0]);
        assert_eq!(isotonic(&[3.0, 2.0, 1.0]), vec![2.0, 2.0, 2.0]);
    }

    #[test]
    fn grey_ramp_has_steps() {
        let g = grey_ramp_target(160, 10, 16);
        assert_eq!(g.px[0], 0.0);
        assert_eq!(g.px[159], 1.0);
    }
}
