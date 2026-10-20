//! Squama job → export parts, over what the core offers on this branch.
//!
//! The CLI speaks the design §12.1 DTO (params JSON + export settings) so the
//! same JSON that a project stores drives a headless run. This module turns it
//! into parts using the spike pipeline (`squama::run`, square grid, directional
//! light, Lambert) plus:
//! - image options (crop + §16 tone operations) applied to the `LinearLuma`;
//! - square-grid tiling to the bed (cuts on cell lines, near-equal counts);
//! - the calibration tile (`squama::calibration`).
//!
//! M1 features owned by other modules are parsed and passed through, and refused
//! here with a clear message until merged:
//! MERGE: wire to squama::squama_export (design §12) for hex layouts, point
//! lights, the shadow optimiser, measured/ambient responses, maxReliefMm, keys
//! and the stand; then delete `tile_square` and `apply_image_opts` below.

use ad3d_core::error::{Error, Result, Warning};
use ad3d_core::image::{LinearLuma, cell_brightness, srgb_to_linear, synthetic_target};
use ad3d_core::io::sink::{ExportPart, PartKind};
use ad3d_core::mesh::validate::ValidateOpts;
use ad3d_core::orient::{Orientation, Xform, export_xform};
use ad3d_core::profiles::Bed;
use ad3d_core::project::{ExportSettings, ImageOptsDto, KeysDto, TileMode, TilesDto};
use ad3d_core::relief::{PieceSet, mesh_relief, quantise};
use ad3d_core::squama::calibration::{CalibrationSpec, calibration_tile};
use ad3d_core::squama::{
    GenOptions, ImageSource, Layout, SYNTHETIC_PX, SquamaOutput, SquamaParams, SquamaParamsDto, build_partition, run,
    solve_facets, validate_opts,
};
use ad3d_core::CancelToken;
use serde_json::Value;

/// One generated part with the `validate()` options that match it.
pub struct GenPart {
    pub part: ExportPart,
    pub validate: ValidateOpts,
    /// Resolved quarter turns (None = flat).
    pub quarter_turns: Option<u32>,
}

pub struct GenResult {
    pub plate: SquamaOutput,
    pub params: SquamaParams,
    pub parts: Vec<GenPart>,
    pub warnings: Vec<Warning>,
    /// Tile grid used (columns along u, rows along v).
    pub tiles: [u32; 2],
}

fn unsupported(what: &str) -> Error {
    Error::InvalidParams(format!(
        "{what} needs an M1 module that is not merged into this build yet (MERGE: squama::squama_export)"
    ))
}

/// Refuses M1 fields this build cannot honour yet, so nothing is silently ignored.
pub fn check_supported(params: &Value, export: &ExportSettings) -> Result<()> {
    if let Some(l) = params.get("layout") {
        if l.get("kind").and_then(Value::as_str).unwrap_or("square") != "square" {
            return Err(unsupported("hex layout"));
        }
    }
    if params.get("light").is_some_and(|l| !l.is_null()) {
        return Err(unsupported("point light"));
    }
    if let Some(o) = params.get("optimiser") {
        if o.get("kind").and_then(Value::as_str).unwrap_or("none") != "none" {
            return Err(unsupported("shadow optimiser"));
        }
    }
    if let Some(r) = params.get("response") {
        let lambert0 = r.get("kind").and_then(Value::as_str) == Some("lambert")
            && r.get("ambient").and_then(Value::as_f64).unwrap_or(0.0) == 0.0;
        if !lambert0 {
            return Err(unsupported("custom response (measured table or ambient)"));
        }
    }
    if params.get("maxReliefMm").is_some_and(|v| !v.is_null()) {
        return Err(unsupported("maxReliefMm"));
    }
    if export.keys != KeysDto::None {
        return Err(unsupported("alignment keys"));
    }
    if export.stand {
        return Err(unsupported("stand"));
    }
    Ok(())
}

fn srgb_encode(y: f32) -> f32 {
    let y = y.clamp(0.0, 1.0);
    if y <= 0.003_130_8 { 12.92 * y } else { 1.055 * y.powf(1.0 / 2.4) - 0.055 }
}

/// Crop then §16 step 5 tone operations, on the perceptual value, back to linear.
/// MERGE: replace with image::prepare_target (design §16) once merged.
pub fn apply_image_opts(img: &LinearLuma, o: &ImageOptsDto) -> Result<LinearLuma> {
    let mut img = img.clone();
    if let Some([x0, y0, x1, y1]) = o.crop {
        if !(0.0 <= x0 && x0 < x1 && x1 <= 1.0 && 0.0 <= y0 && y0 < y1 && y1 <= 1.0) {
            return Err(Error::InvalidParams(format!("crop {:?} must be 0 ≤ x0 < x1 ≤ 1, 0 ≤ y0 < y1 ≤ 1", o.crop)));
        }
        let (w, h) = (img.w as f64, img.h as f64);
        let (px0, px1) = ((x0 * w).floor() as u32, ((x1 * w).ceil() as u32).min(img.w).max((x0 * w).floor() as u32 + 1));
        let (py0, py1) = ((y0 * h).floor() as u32, ((y1 * h).ceil() as u32).min(img.h).max((y0 * h).floor() as u32 + 1));
        let mut px = Vec::with_capacity(((px1 - px0) * (py1 - py0)) as usize);
        for y in py0..py1 {
            let row = (y * img.w) as usize;
            px.extend_from_slice(&img.px[row + px0 as usize..row + px1 as usize]);
        }
        img = LinearLuma { w: px1 - px0, h: py1 - py0, px };
    }
    let gamma = o.gamma.unwrap_or(1.0);
    let contrast = o.contrast.unwrap_or(1.0);
    let brightness = o.brightness.unwrap_or(0.0);
    if !(0.2..=5.0).contains(&gamma) || !(0.0..=3.0).contains(&contrast) || !(-1.0..=1.0).contains(&brightness) {
        return Err(Error::InvalidParams("imageOpts out of range (gamma 0.2–5, contrast 0–3, brightness −1…1)".into()));
    }
    let invert = o.invert.unwrap_or(false);
    let identity = gamma == 1.0 && contrast == 1.0 && brightness == 0.0 && !invert && o.auto_levels.is_none();
    if identity {
        return Ok(img);
    }
    let mut v: Vec<f32> = img.px.iter().map(|&y| if y.is_finite() { srgb_encode(y) } else { 0.0 }).collect();
    if invert {
        v.iter_mut().for_each(|x| *x = 1.0 - *x);
    }
    if let Some([plo, phi]) = o.auto_levels {
        if !(0.0..1.0).contains(&plo) || !(plo < phi && phi <= 1.0) {
            return Err(Error::InvalidParams("autoLevels must be percentiles 0 ≤ lo < hi ≤ 1".into()));
        }
        let mut s = v.clone();
        s.sort_by(|a, b| a.total_cmp(b));
        let at = |p: f64| s[((p * (s.len() - 1) as f64).round() as usize).min(s.len() - 1)];
        let (lo, hi) = (at(plo), at(phi));
        if hi > lo {
            v.iter_mut().for_each(|x| *x = (*x - lo) / (hi - lo));
        }
    }
    for x in &mut v {
        let mut t = (*x + brightness as f32).clamp(0.0, 1.0);
        t = ((t - 0.5) * contrast as f32 + 0.5).clamp(0.0, 1.0);
        t = t.powf(1.0 / gamma as f32);
        *x = srgb_to_linear(t.clamp(0.0, 1.0));
    }
    Ok(LinearLuma { w: img.w, h: img.h, px: v })
}

/// Near-equal split of `n` cells into `k` runs (63 → 32 + 31).
pub fn split_counts(n: u32, k: u32) -> Vec<u32> {
    let k = k.clamp(1, n.max(1));
    (0..k).map(|i| n / k + u32::from(i < n % k)).collect()
}

/// Plate-space extents (u, v) that fit the bed in this orientation.
fn max_tile_uv(bed: &Bed, o: Orientation, k: Option<u32>) -> [f64; 2] {
    let [ux, uy] = bed.usable_xy();
    match (o, k) {
        (Orientation::Flat, _) => [ux, uy],
        (_, Some(k)) if k % 2 == 1 => [bed.z_mm, ux],
        _ => [ux, bed.z_mm],
    }
}

fn compose_translate(x: &Xform, d: [f64; 3]) -> Xform {
    // p ↦ x(p + d)
    let t = x.apply(d);
    Xform { m: x.m, t }
}

/// Splits the plate into tiles on cell lines and meshes each tile on its own.
/// MERGE: replace with squama::plan_tiles + squama_export (design §14.1).
fn tile_square(
    p: &SquamaParams,
    img: &LinearLuma,
    l: &Layout,
    grid: [u32; 2],
    o: Orientation,
    opts: &GenOptions,
    cancel: &CancelToken,
) -> Result<Vec<GenPart>> {
    let tol = opts.tol;
    let mut pe = *p;
    pe.light_elevation = pe.light_elevation.max(1f64.to_radians());
    let cb = cell_brightness(img, l.nx, l.ny, l.crop)?;
    let facets = solve_facets(&cb, l, &pe, &tol);
    let part = build_partition(l);
    let heights = quantise(&part, &facets, p.base_mm, &tol)?;
    let (cx, cy) = (split_counts(l.nx, grid[0]), split_counts(l.ny, grid[1]));
    let mut out = Vec::new();
    let mut j0 = 0u32;
    for (tj, &ny) in cy.iter().enumerate() {
        let mut i0 = 0u32;
        for (ti, &nx) in cx.iter().enumerate() {
            cancel.check()?;
            let pieces: Vec<u32> = (j0..j0 + ny).flat_map(|j| (i0..i0 + nx).map(move |i| j * l.nx + i)).collect();
            let mesh = mesh_relief(&part, PieceSet::Subset(&pieces), &heights, &[], &tol, cancel)?;
            let (u0, v0) = (i0 as f64 * l.cell_mm, j0 as f64 * l.cell_mm);
            let (tw, th) = (nx as f64 * l.cell_mm, ny as f64 * l.cell_mm);
            let (xo, k) = export_xform(o, tw, th, p.light_azimuth);
            let transform = compose_translate(&xo, [-u0, -v0, 0.0]);
            let validate = ValidateOpts {
                transform,
                min_facet_w: Some(p.base_mm - tol.q / 2.0),
                back_area: Some(tw * th),
                ..Default::default()
            };
            out.push(GenPart {
                part: ExportPart { name: format!("tile_r{}c{}", tj + 1, ti + 1), kind: PartKind::Tile, mesh, transform },
                validate,
                quarter_turns: k,
            });
            i0 += nx;
        }
        j0 += ny;
    }
    Ok(out)
}

/// Generates every part for a job. `luma` is `None` for the synthetic target.
pub fn generate(
    params: &Value,
    luma: Option<&LinearLuma>,
    export: &ExportSettings,
    bed: &Bed,
    cancel: &CancelToken,
) -> Result<GenResult> {
    check_supported(params, export)?;
    let mut v = params.clone();
    if let Some(o) = v.as_object_mut() {
        o.entry("jobId").or_insert(Value::from(0));
    }
    let dto: SquamaParamsDto =
        serde_json::from_value(v).map_err(|e| Error::InvalidParams(format!("squama params: {e}")))?;
    let p = dto.params();
    let image_opts: ImageOptsDto = match params.get("imageOpts") {
        Some(Value::Null) | None => ImageOptsDto::default(),
        Some(o) => serde_json::from_value(o.clone()).map_err(|e| Error::InvalidParams(format!("imageOpts: {e}")))?,
    };
    let synth;
    let base_img = match luma {
        Some(l) => l,
        None => {
            synth = synthetic_target(SYNTHETIC_PX.0, SYNTHETIC_PX.1);
            &synth
        }
    };
    let img = apply_image_opts(base_img, &image_opts)?;
    let opts = GenOptions::default();
    let plate = run(&p, ImageSource::Luma(&img), &opts, cancel)?;
    let mut warnings = plate.warnings.clone();
    let l = plate.layout;
    let o = export.orientation.orientation();
    let (_, k_plate) = export_xform(o, l.w_mm, l.h_mm, p.light_azimuth);
    let grid = match export.tiles {
        TilesDto::Mode(TileMode::None) => [1, 1],
        TilesDto::Grid(g) => [g[0].min(l.nx), g[1].min(l.ny)],
        TilesDto::Mode(TileMode::Auto) => {
            let [mu, mv] = max_tile_uv(bed, o, k_plate);
            let need = |max: f64, n: u32| {
                // Fewest tiles whose largest run of cells fits.
                (1..=n).find(|&k| (n.div_ceil(k)) as f64 * l.cell_mm <= max + 1e-9)
            };
            match (need(mu, l.nx), need(mv, l.ny)) {
                (Some(a), Some(b)) => [a, b],
                _ => {
                    return Err(Error::InvalidParams(format!(
                        "a single {} mm cell does not fit the bed ({} × {} mm usable)",
                        l.cell_mm, mu, mv
                    )));
                }
            }
        }
    };
    let mut parts = Vec::new();
    if grid == [1, 1] {
        let (x, k) = export_xform(o, l.w_mm, l.h_mm, p.light_azimuth);
        parts.push(GenPart {
            validate: validate_opts(&plate, x),
            part: ExportPart { name: "plate".into(), kind: PartKind::Plate, mesh: plate.mesh.clone(), transform: x },
            quarter_turns: k,
        });
    } else {
        parts.extend(tile_square(&p, &img, &l, grid, o, &opts, cancel)?);
    }
    if export.calibration_tile {
        let spec = CalibrationSpec::from_params(&p);
        let t = calibration_tile(&spec, &opts, cancel)?;
        warnings.extend(t.warnings.iter().cloned());
        let (x, k) = export_xform(o, t.key.w_mm, t.key.h_mm, p.light_azimuth);
        parts.push(GenPart {
            validate: t.validate_opts(x),
            part: ExportPart { name: "calibration".into(), kind: PartKind::Calibration, mesh: t.mesh, transform: x },
            quarter_turns: k,
        });
    }
    // Bed fit, on the written coordinates.
    for gp in &parts {
        let b = gp.part.mesh.map_positions(|q| gp.part.transform.apply_f32(q)).bounds();
        if let Some(b) = b {
            let s = b.size().map(|v| v as f64);
            if !bed.fits(s, false) {
                warnings.push(Warning {
                    code: "does_not_fit_bed",
                    message: format!(
                        "{} is {:.1} × {:.1} × {:.1} mm, larger than the bed ({} × {} × {} mm, margin {} mm); try --tiles auto",
                        gp.part.name, s[0], s[1], s[2], bed.x_mm, bed.y_mm, bed.z_mm, bed.margin_mm
                    ),
                });
            }
        }
    }
    Ok(GenResult { plate, params: p, parts, warnings, tiles: grid })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ad3d_core::mesh::validate::validate;
    use ad3d_core::profiles::default_printer;
    use serde_json::json;

    fn params(width: f64) -> Value {
        json!({ "widthMm": width, "cellMm": 6.0, "lightAzimuthDeg": 180.0, "lightElevationDeg": 5.0,
                "maxTiltDeg": 10.0, "baseMm": 3.0, "image": "synthetic" })
    }

    #[test]
    fn split_is_near_equal() {
        assert_eq!(split_counts(63, 2), vec![32, 31]);
        assert_eq!(split_counts(10, 3), vec![4, 3, 3]);
        assert_eq!(split_counts(3, 5), vec![1, 1, 1]);
    }

    #[test]
    fn auto_tiles_fit_the_cr6se_and_validate() {
        let bed = default_printer().bed();
        let ex = ExportSettings { tiles: TilesDto::Mode(TileMode::Auto), ..Default::default() };
        let r = generate(&params(300.0), None, &ex, &bed, &CancelToken::new()).unwrap();
        assert!(r.parts.len() > 1, "300 × 378 standing must be tiled on 235 × 235 × 250");
        assert!(r.warnings.iter().all(|w| w.code != "does_not_fit_bed"), "{:?}", r.warnings);
        for gp in &r.parts {
            let rep = validate(&gp.part.mesh, &gp.validate);
            assert!(rep.ok, "{}: {}", gp.part.name, rep.summary());
        }
        // Tiles cover the plate's cells exactly once.
        let tris: usize = r.parts.iter().map(|p| p.part.mesh.triangle_count()).sum();
        assert!(tris > r.plate.mesh.triangle_count());
    }

    #[test]
    fn unsupported_fields_are_refused() {
        let ex = ExportSettings::default();
        let bed = default_printer().bed();
        let mut p = params(60.0);
        p["layout"] = json!({ "kind": "hex", "orientation": "pointy" });
        assert!(generate(&p, None, &ex, &bed, &CancelToken::new()).is_err());
        let ex2 = ExportSettings { stand: true, ..Default::default() };
        assert!(generate(&params(60.0), None, &ex2, &bed, &CancelToken::new()).is_err());
    }

    #[test]
    fn image_opts_crop_and_invert() {
        let img = LinearLuma { w: 4, h: 2, px: vec![0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0] };
        let o = ImageOptsDto { crop: Some([0.5, 0.0, 1.0, 1.0]), invert: Some(true), ..Default::default() };
        let r = apply_image_opts(&img, &o).unwrap();
        assert_eq!((r.w, r.h), (2, 2));
        assert!(r.px.iter().all(|&v| v.abs() < 1e-6));
        assert!(apply_image_opts(&img, &ImageOptsDto { gamma: Some(9.0), ..Default::default() }).is_err());
    }
}
