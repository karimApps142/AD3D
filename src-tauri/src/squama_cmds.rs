//! M1 Squama commands: live preview, image intake, the 2D target view and export.
//!
//! Every command that does CPU or file work is `async` and moves it to
//! `tauri::async_runtime::spawn_blocking`, so the UI thread is never blocked.
//!
//! Command contract (mirrors `src/engine/tauriEngine.ts`; DTOs follow
//! docs/design/squama-mesher.md §12.1):
//! - `generate_squama { params }`            → raw `AD3M` payload (latest-wins, cancellable;
//!   flags bit 0 = final quality, §12.2)
//! - `squama_job_info { jobId }`             → `JobInfo | null` (layout + warnings of a recent job)
//! - `squama_target { params }`              → raw `AD3T` grid: cell-averaged target (§16)
//! - `squama_image_open { path }`            → `ImageInfo` (decodes and caches the photo)
//! - `squama_image_thumbnail { path, maxPx, rotate, mirror }` → raw grey thumbnail
//! - `export_squama { export, onEvent }`     → `ExportResult`; progress over a `Channel`
//! - `cancel_export`                         → `bool` (true when an export was running)
//! - `reveal_in_finder { path }`             → `()`
//!
//! The M1 core API (§12 `prepare_target`, `solve_facets` with optimiser and point light,
//! `plan_tiles`, `squama_export`, 3MF) is being built in parallel. Until it merges, this file
//! carries small, clearly marked stand-ins (`// MERGE:`) built from the spike core.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use ad3d_core::image::{CellBrightness, LinearLuma, cell_brightness, synthetic_target};
use ad3d_core::io::stl::write_stl;
use ad3d_core::mesh::validate::validate;
use ad3d_core::orient::{Orientation, QuarterTurns, export_xform};
use ad3d_core::squama::{self, GenOptions, SYNTHETIC_PX, SquamaParams, SquamaParamsDto};
use ad3d_core::{CancelToken, Error as CoreError, payload};
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::{Channel, Response};

// ---------------------------------------------------------------------------------------------
// DTO (§12.1). v1 fields are required; every M1 field is optional with a default, so v1 payloads
// (the spike harness) stay valid.
// ---------------------------------------------------------------------------------------------

/// Cache key of the built-in synthetic target (the DTO's `image: "synthetic"`).
pub const SYNTHETIC_KEY: &str = "synthetic";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QualityDto {
    #[default]
    Preview,
    Final,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HexOrientation {
    Pointy,
    Flat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum LayoutDto {
    Square,
    Hex { orientation: HexOrientation },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum LightDto {
    Point {
        #[serde(rename = "posMm")]
        pos_mm: [f64; 3],
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ResponseDto {
    Lambert {
        ambient: f64,
    },
    Measured {
        #[serde(rename = "tiltDeg")]
        tilt_deg: Vec<f64>,
        brightness: Vec<f64>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum OptimiserDto {
    None,
    Shadow {
        #[serde(default, rename = "maxSweeps", skip_serializing_if = "Option::is_none")]
        max_sweeps: Option<u32>,
    },
}

/// `imageOpts` (§12.1, §16). `rotate` and `mirror` are AD3D additions (FEATURES PROJ-SQUAMA
/// `rotate_deg`, `mirror`), applied in the app crate like the EXIF orientation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ImageOptsDto {
    /// Normalised `x0, y0, x1, y1` in the rotated/mirrored image, y down.
    pub crop: Option<[f64; 4]>,
    pub gamma: Option<f64>,
    pub contrast: Option<f64>,
    pub brightness: Option<f64>,
    pub invert: Option<bool>,
    /// Percentiles `(lo, hi)`; `null` or absent = off.
    pub auto_levels: Option<[f64; 2]>,
    /// Clockwise quarter turns in degrees: 0, 90, 180 or 270.
    pub rotate: Option<u32>,
    /// Horizontal flip, applied after `rotate`.
    pub mirror: Option<bool>,
}

/// The M1 request (§12.1 `SquamaParams`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SquamaParamsM1 {
    #[serde(default)]
    pub job_id: u32,
    #[serde(default)]
    pub quality: QualityDto,
    pub width_mm: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_mm: Option<f64>,
    pub cell_mm: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<LayoutDto>,
    pub light_azimuth_deg: f64,
    pub light_elevation_deg: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<LightDto>,
    pub max_tilt_deg: f64,
    pub base_mm: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_relief_mm: Option<f64>,
    pub image: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_opts: Option<ImageOptsDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<ResponseDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optimiser: Option<OptimiserDto>,
}

/// A warning for the UI. `code` is stable; `message` is for people.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WarningDto {
    pub code: String,
    pub message: String,
}

impl WarningDto {
    fn new(code: &str, message: impl Into<String>) -> Self {
        WarningDto { code: code.to_owned(), message: message.into() }
    }
}

impl SquamaParamsM1 {
    /// The spike (v1) DTO the current core understands.
    // MERGE: wire to the core's extended `squama::SquamaParamsDto` once it carries the M1 fields.
    pub fn to_v1(&self) -> SquamaParamsDto {
        SquamaParamsDto {
            job_id: self.job_id,
            width_mm: self.width_mm,
            height_mm: self.height_mm,
            cell_mm: self.cell_mm,
            light_azimuth_deg: self.light_azimuth_deg,
            light_elevation_deg: self.light_elevation_deg,
            max_tilt_deg: self.max_tilt_deg,
            base_mm: self.base_mm,
            image: self.image.clone(),
        }
    }

    pub fn core_params(&self) -> SquamaParams {
        self.to_v1().params()
    }

    pub fn is_synthetic(&self) -> bool {
        self.image == SYNTHETIC_KEY
    }

    pub fn is_final(&self) -> bool {
        self.quality == QualityDto::Final
    }

    pub fn image_opts(&self) -> ImageOptsDto {
        self.image_opts.clone().unwrap_or_default()
    }

    /// M1 options the merged core does not honour yet, reported so the UI can say so inline
    /// instead of silently showing something else.
    // MERGE: drop each entry as ad3d-core::squama gains hex layouts (§6.2), point lights
    // (§15.4), the shadow optimiser (§15.1) and measured responses (§15.2).
    pub fn unsupported_warnings(&self) -> Vec<WarningDto> {
        let mut w = Vec::new();
        if let Some(LayoutDto::Hex { .. }) = self.layout {
            w.push(WarningDto::new("hex_pending", "Hex cells are not in this build yet; showing square cells."));
        }
        if let Some(LightDto::Point { .. }) = self.light {
            w.push(WarningDto::new(
                "point_light_pending",
                "Point-lamp solve is not in this build yet; using a directional light at the same azimuth and elevation.",
            ));
        }
        if let Some(OptimiserDto::Shadow { .. }) = self.optimiser {
            if self.is_final() {
                w.push(WarningDto::new("optimiser_pending", "The shadow-aware optimiser is not in this build yet; using the Lambert inverse."));
            }
        }
        if let Some(ResponseDto::Measured { tilt_deg, brightness }) = &self.response {
            if tilt_deg.len() != brightness.len() || tilt_deg.len() < 2 {
                w.push(WarningDto::new("measured_response_invalid", "The measured response needs at least two (tilt, brightness) pairs."));
            }
            w.push(WarningDto::new("measured_response_pending", "Measured responses are not in this build yet; using Lambert."));
        }
        w
    }
}

// ---------------------------------------------------------------------------------------------
// Image pipeline stand-in (§16 steps 1–5). The app crate owns decode → LinearLuma; the core will
// own tone operations in `prepare_target`.
// ---------------------------------------------------------------------------------------------

/// Longest side of the cached working copy (§16 step 4).
pub const WORKING_MAX_PX: u32 = 2048;

/// Area-downsamples by the smallest integer factor that brings the long side to ≤ `max_px`.
pub fn downsample(img: &LinearLuma, max_px: u32) -> LinearLuma {
    let long = img.w.max(img.h);
    if long <= max_px || max_px == 0 {
        return img.clone();
    }
    let f = long.div_ceil(max_px);
    let (w, h) = (img.w.div_ceil(f), img.h.div_ceil(f));
    let mut px = vec![0f32; (w * h) as usize];
    for y in 0..h {
        let (y0, y1) = (y * f, ((y + 1) * f).min(img.h));
        for x in 0..w {
            let (x0, x1) = (x * f, ((x + 1) * f).min(img.w));
            let mut s = 0f64;
            for yy in y0..y1 {
                let row = &img.px[(yy * img.w) as usize..];
                for xx in x0..x1 {
                    s += row[xx as usize] as f64;
                }
            }
            px[(y * w + x) as usize] = (s / ((y1 - y0) * (x1 - x0)) as f64) as f32;
        }
    }
    LinearLuma { w, h, px }
}

/// Rotates clockwise by `rotate_deg` (0/90/180/270; other values round to the nearest quarter)
/// and then mirrors left↔right.
pub fn orient_image(img: &LinearLuma, rotate_deg: u32, mirror: bool) -> LinearLuma {
    let q = ((rotate_deg as f64 / 90.0).round() as u32) % 4;
    let (w, h) = (img.w, img.h);
    let (nw, nh) = if q % 2 == 1 { (h, w) } else { (w, h) };
    if q == 0 && !mirror {
        return img.clone();
    }
    let mut px = vec![0f32; img.px.len()];
    for y in 0..nh {
        for x in 0..nw {
            let xs = if mirror { nw - 1 - x } else { x };
            // Source pixel for destination (xs, y) under a clockwise rotation by q quarters.
            let (sx, sy) = match q {
                0 => (xs, y),
                1 => (y, h - 1 - xs),
                2 => (w - 1 - xs, h - 1 - y),
                _ => (w - 1 - y, xs),
            };
            px[(y * nw + x) as usize] = img.px[(sy * w + sx) as usize];
        }
    }
    LinearLuma { w: nw, h: nh, px }
}

/// Pixel rectangle of a normalised crop, at least 1×1 and inside the image.
pub fn crop_rect_px(w: u32, h: u32, c: [f64; 4]) -> Result<(u32, u32, u32, u32), String> {
    if !c.iter().all(|v| v.is_finite()) {
        return Err("crop has non-finite values".into());
    }
    let cl = |v: f64| v.clamp(0.0, 1.0);
    let (x0, x1) = (cl(c[0].min(c[2])), cl(c[0].max(c[2])));
    let (y0, y1) = (cl(c[1].min(c[3])), cl(c[1].max(c[3])));
    let px0 = ((x0 * w as f64).round() as u32).min(w - 1);
    let py0 = ((y0 * h as f64).round() as u32).min(h - 1);
    let px1 = ((x1 * w as f64).round() as u32).clamp(px0 + 1, w);
    let py1 = ((y1 * h as f64).round() as u32).clamp(py0 + 1, h);
    Ok((px0, py0, px1, py1))
}

pub fn crop_image(img: &LinearLuma, c: [f64; 4]) -> Result<LinearLuma, String> {
    let (x0, y0, x1, y1) = crop_rect_px(img.w, img.h, c)?;
    if (x0, y0, x1, y1) == (0, 0, img.w, img.h) {
        return Ok(img.clone());
    }
    let (w, h) = (x1 - x0, y1 - y0);
    let mut px = Vec::with_capacity((w * h) as usize);
    for y in y0..y1 {
        let row = (y * img.w) as usize;
        px.extend_from_slice(&img.px[row + x0 as usize..row + x1 as usize]);
    }
    Ok(LinearLuma { w, h, px })
}

#[inline]
pub fn srgb_encode(y: f64) -> f64 {
    let y = y.clamp(0.0, 1.0);
    if y <= 0.003_130_8 { 12.92 * y } else { 1.055 * y.powf(1.0 / 2.4) - 0.055 }
}

#[inline]
pub fn srgb_decode(v: f64) -> f64 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

/// Tone settings with the §16 defaults filled in and clamped to their domains.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tone {
    pub invert: bool,
    pub auto_levels: Option<[f64; 2]>,
    pub brightness: f64,
    pub contrast: f64,
    pub gamma: f64,
}

impl Tone {
    pub fn from_opts(o: &ImageOptsDto) -> Tone {
        let fin = |v: Option<f64>, d: f64| v.filter(|x| x.is_finite()).unwrap_or(d);
        Tone {
            invert: o.invert.unwrap_or(false),
            auto_levels: o.auto_levels.filter(|p| p.iter().all(|x| x.is_finite())).map(|[a, b]| {
                let (a, b) = (a.clamp(0.0, 1.0), b.clamp(0.0, 1.0));
                [a.min(b), a.max(b)]
            }),
            brightness: fin(o.brightness, 0.0).clamp(-1.0, 1.0),
            contrast: fin(o.contrast, 1.0).clamp(0.0, 3.0),
            gamma: fin(o.gamma, 1.0).clamp(0.2, 5.0),
        }
    }

    pub fn is_identity(&self) -> bool {
        !self.invert && self.auto_levels.is_none() && self.brightness == 0.0 && self.contrast == 1.0 && self.gamma == 1.0
    }

    /// The perceptual curve after invert and levels: brightness, contrast, gamma (§16 step 5).
    pub fn curve(&self, v: f64, lo: f64, hi: f64) -> f64 {
        let mut v = if self.invert { 1.0 - v } else { v };
        if hi - lo > 1e-6 {
            v = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
        }
        v += self.brightness;
        v = (v - 0.5) * self.contrast + 0.5;
        v = v.clamp(0.0, 1.0);
        v.powf(1.0 / self.gamma)
    }
}

const TONE_LUT: usize = 4096;

/// §16 step 5 on a linear image, through a 4096-entry LUT indexed by `sqrt(Y)` (dense in the
/// shadows, where sRGB is too) with linear interpolation.
// MERGE: wire to ad3d-core::squama::prepare_target (§16) once the core owns tone operations.
pub fn apply_tone(img: &LinearLuma, t: &Tone) -> LinearLuma {
    if t.is_identity() {
        return img.clone();
    }
    let (mut lo, mut hi) = (0.0, 1.0);
    if let Some([p0, p1]) = t.auto_levels {
        let mut hist = vec![0u32; TONE_LUT];
        for &y in &img.px {
            let mut v = srgb_encode(if y.is_finite() { y as f64 } else { 0.0 });
            if t.invert {
                v = 1.0 - v;
            }
            hist[((v * (TONE_LUT - 1) as f64).round() as usize).min(TONE_LUT - 1)] += 1;
        }
        let n = img.px.len().max(1) as f64;
        let pct = |p: f64| {
            let target = p * n;
            let mut acc = 0f64;
            for (k, &c) in hist.iter().enumerate() {
                acc += c as f64;
                if acc >= target && acc > 0.0 {
                    return k as f64 / (TONE_LUT - 1) as f64;
                }
            }
            1.0
        };
        let (a, b) = (pct(p0), pct(p1));
        if b - a > 1e-3 {
            lo = a;
            hi = b;
        }
    }
    let lut: Vec<f32> = (0..TONE_LUT)
        .map(|k| {
            let s = k as f64 / (TONE_LUT - 1) as f64;
            srgb_decode(t.curve(srgb_encode(s * s), lo, hi)) as f32
        })
        .collect();
    let scale = (TONE_LUT - 1) as f32;
    let px = img
        .px
        .iter()
        .map(|&y| {
            let y = if y.is_finite() { y.clamp(0.0, 1.0) } else { 0.0 };
            let f = y.sqrt() * scale;
            let i = (f as usize).min(TONE_LUT - 2);
            let fr = f - i as f32;
            lut[i] + (lut[i + 1] - lut[i]) * fr
        })
        .collect();
    LinearLuma { w: img.w, h: img.h, px }
}

/// sRGB-encoded 8-bit grey for display.
pub fn to_srgb8(px: &[f32]) -> Vec<u8> {
    let lut: Vec<u8> = (0..TONE_LUT)
        .map(|k| {
            let s = k as f64 / (TONE_LUT - 1) as f64;
            (srgb_encode(s * s) * 255.0).round() as u8
        })
        .collect();
    px.iter()
        .map(|&y| {
            let y = if y.is_finite() { y.clamp(0.0, 1.0) } else { 0.0 };
            lut[((y.sqrt() * (TONE_LUT - 1) as f32).round() as usize).min(TONE_LUT - 1)]
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Tiling stand-in (§14.1, square grid).
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrientationDto {
    #[serde(rename = "standing-auto")]
    StandingAuto,
    #[serde(rename = "standing-q0")]
    StandingQ0,
    #[serde(rename = "standing-q1")]
    StandingQ1,
    #[serde(rename = "standing-q2")]
    StandingQ2,
    #[serde(rename = "standing-q3")]
    StandingQ3,
    #[serde(rename = "flat")]
    Flat,
}

impl OrientationDto {
    pub fn core(self) -> Orientation {
        match self {
            OrientationDto::StandingAuto => Orientation::Standing(QuarterTurns::Auto),
            OrientationDto::StandingQ0 => Orientation::Standing(QuarterTurns::Q0),
            OrientationDto::StandingQ1 => Orientation::Standing(QuarterTurns::Q1),
            OrientationDto::StandingQ2 => Orientation::Standing(QuarterTurns::Q2),
            OrientationDto::StandingQ3 => Orientation::Standing(QuarterTurns::Q3),
            OrientationDto::Flat => Orientation::Flat,
        }
    }
}

/// Splits `n` cells into `parts` near-equal runs, larger runs first (63 → 32 + 31).
pub fn split_even(n: u32, parts: u32) -> Vec<u32> {
    let parts = parts.clamp(1, n.max(1));
    let (base, extra) = (n / parts, n % parts);
    (0..parts).map(|k| base + u32::from(k < extra)).collect()
}

/// Cell counts per tile column (left → right) and per tile row (bottom → top, plate `v`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TileGrid {
    pub cols: Vec<u32>,
    pub rows: Vec<u32>,
}

/// Largest plate-space tile extent `(u, v)` that fits the bed in the given orientation.
pub fn max_tile_extent(o: Orientation, bed: [f64; 3], margin: f64, light_azimuth: f64) -> (f64, f64) {
    let bx = bed[0] - 2.0 * margin;
    let by = bed[1] - 2.0 * margin;
    let bz = bed[2] - margin;
    match o {
        Orientation::Flat => (bx, by),
        Orientation::Standing(_) => {
            let (_, k) = export_xform(o, 1.0, 1.0, light_azimuth);
            // Standing parts put plate u' along X and v' up Z (§9); odd quarter turns swap u and v.
            if k.unwrap_or(0) % 2 == 0 { (bx, bz) } else { (bz, bx) }
        }
    }
}

// MERGE: wire to ad3d-core::squama::plan_tiles (§14.1) — it also handles hex seams and the stand.
pub fn plan_tiles_square(nx: u32, ny: u32, cell: f64, max_uv: (f64, f64)) -> Result<TileGrid, String> {
    let per = |m: f64| ((m + 1e-9) / cell).floor();
    let (mu, mv) = (per(max_uv.0), per(max_uv.1));
    if mu < 1.0 || mv < 1.0 {
        return Err(format!("a single {cell} mm cell does not fit the bed in this orientation"));
    }
    let (mu, mv) = (mu as u32, mv as u32);
    Ok(TileGrid { cols: split_even(nx, nx.div_ceil(mu)), rows: split_even(ny, ny.div_ceil(mv)) })
}

/// Sub-grid of a brightness grid (rows counted from the bottom, like the grid itself).
pub fn sub_grid(cb: &CellBrightness, i0: u32, j0: u32, w: u32, h: u32) -> CellBrightness {
    let mut g = Vec::with_capacity((w * h) as usize);
    for j in j0..j0 + h {
        for i in i0..i0 + w {
            g.push(cb.at(i, j));
        }
    }
    CellBrightness { nx: w, ny: h, g }
}

/// Calibration grey ramp: 12 uniform patches, 4 cells across × 6 cells along the light, laid out
/// 4 × 3 (§15.3 example). Patch k has target brightness k / 11, bottom-left first.
// MERGE: wire to ad3d-core::squama::calibration_tile (§15.3) for fiducials and the orientation marker.
pub fn calibration_grid(light_azimuth: f64) -> CellBrightness {
    let along_u = light_azimuth.cos().abs() >= light_azimuth.sin().abs();
    let (pw, ph) = if along_u { (6, 4) } else { (4, 6) };
    let (cols, rows) = (4u32, 3u32);
    let (nx, ny) = (cols * pw, rows * ph);
    let mut g = Vec::with_capacity((nx * ny) as usize);
    for j in 0..ny {
        for i in 0..nx {
            let k = (j / ph) * cols + i / pw;
            g.push(k as f32 / 11.0);
        }
    }
    CellBrightness { nx, ny, g }
}

// ---------------------------------------------------------------------------------------------
// Export DTOs.
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportFormat {
    #[serde(rename = "stl")]
    Stl,
    #[serde(rename = "3mf")]
    ThreeMf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeysDto {
    #[default]
    None,
    Pegs,
}

/// §12.1 `SquamaExport`. `baseName` is an AD3D addition: the file stem the user typed in the
/// save dialog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SquamaExportDto {
    pub params: SquamaParamsM1,
    pub out_dir: String,
    #[serde(default)]
    pub base_name: Option<String>,
    pub formats: Vec<ExportFormat>,
    pub orientation: OrientationDto,
    #[serde(default)]
    pub bed_mm: Option<[f64; 3]>,
    #[serde(default)]
    pub margin_mm: Option<f64>,
    #[serde(default)]
    pub keys: Option<KeysDto>,
    #[serde(default)]
    pub stand: Option<bool>,
    #[serde(default)]
    pub calibration_tile: Option<bool>,
}

/// Progress events sent over the export `Channel`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ExportEvent {
    #[serde(rename_all = "camelCase")]
    Started { total_parts: u32 },
    #[serde(rename_all = "camelCase")]
    Progress { part: String, index: u32, total_parts: u32, stage: String, fraction: f64 },
    #[serde(rename_all = "camelCase")]
    PartWritten { part: String, path: String, triangles: u32 },
    Warning { code: String, message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartReport {
    pub name: String,
    pub files: Vec<String>,
    pub triangles: u32,
    /// Print-space size, mm.
    pub size_mm: [f64; 3],
    pub fits_bed: Option<bool>,
    pub validated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub out_dir: String,
    pub files: Vec<String>,
    pub parts: Vec<PartReport>,
    pub tiles: Option<TileGrid>,
    pub warnings: Vec<WarningDto>,
    pub elapsed_ms: f64,
}

// ---------------------------------------------------------------------------------------------
// Job info and caches.
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutInfo {
    pub kind: &'static str,
    pub nx: u32,
    pub ny: u32,
    pub w_mm: f64,
    pub h_mm: f64,
    pub cell_mm: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobInfo {
    pub job_id: u32,
    pub quality: QualityDto,
    pub layout: LayoutInfo,
    pub warnings: Vec<WarningDto>,
    pub gen_ms: f64,
    pub triangles: u32,
    /// Set when this job ran `validate()` (final quality in debug builds).
    pub validated: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageInfo {
    pub path: String,
    pub width_px: u32,
    pub height_px: u32,
    /// Size of the cached working copy (≤ 2048 on the long side).
    pub working_width_px: u32,
    pub working_height_px: u32,
}

const BASE_CACHE_MAX: usize = 4;
const PREPARED_CACHE_MAX: usize = 8;
const CELL_CACHE_MAX: usize = 32;
const JOB_INFO_MAX: usize = 16;

/// Payload `flags` word offset (§12.2) and bit 0 = final quality.
pub const PAYLOAD_FLAGS_OFFSET: usize = 52;
pub const FLAG_FINAL: u32 = 1;

/// `AD3T` target grid: magic, nx, ny, flags, then `nx·ny` f32 (row 0 at `v = 0`).
pub const TARGET_MAGIC: u32 = 0x5433_4441;

type CellKey = (String, u32, u32, Option<[u64; 4]>);

struct Base {
    img: Arc<LinearLuma>,
    orig: (u32, u32),
}

#[derive(Default)]
pub struct SquamaState {
    newest_job: AtomicU64,
    running: Mutex<Option<(u64, CancelToken)>>,
    base: Mutex<HashMap<String, Arc<Base>>>,
    prepared: Mutex<HashMap<String, Arc<LinearLuma>>>,
    cells: Mutex<HashMap<CellKey, Arc<CellBrightness>>>,
    jobs: Mutex<VecDeque<JobInfo>>,
    export: Mutex<Option<CancelToken>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn put<K: std::hash::Hash + Eq, V>(m: &Mutex<HashMap<K, V>>, max: usize, k: K, v: V) {
    let mut c = lock(m);
    if c.len() >= max {
        c.clear();
    }
    c.insert(k, v);
}

fn core_err(e: CoreError) -> String {
    e.to_string()
}

/// Key for everything that shapes the prepared image.
fn prepared_key(image: &str, o: &ImageOptsDto) -> String {
    let t = Tone::from_opts(o);
    let crop = o.crop.map(|c| c.map(f64::to_bits));
    format!(
        "{image}|r{}|m{}|c{crop:?}|i{}|a{:?}|b{}|k{}|g{}",
        o.rotate.unwrap_or(0) % 360,
        o.mirror.unwrap_or(false),
        t.invert,
        t.auto_levels.map(|p| p.map(f64::to_bits)),
        t.brightness.to_bits(),
        t.contrast.to_bits(),
        t.gamma.to_bits()
    )
}

impl SquamaState {
    fn base(&self, image: &str) -> Result<Arc<Base>, String> {
        if let Some(b) = lock(&self.base).get(image) {
            return Ok(b.clone());
        }
        // Decode outside the lock; two racing decodes of one path are harmless.
        let full = if image == SYNTHETIC_KEY {
            synthetic_target(SYNTHETIC_PX.0, SYNTHETIC_PX.1)
        } else {
            LinearLuma::load(Path::new(image)).map_err(core_err)?
        };
        let orig = (full.w, full.h);
        let img = Arc::new(downsample(&full, WORKING_MAX_PX));
        let b = Arc::new(Base { img, orig });
        put(&self.base, BASE_CACHE_MAX, image.to_owned(), b.clone());
        Ok(b)
    }

    /// Oriented, cropped and toned working image, plus its cache key.
    fn prepared(&self, p: &SquamaParamsM1) -> Result<(String, Arc<LinearLuma>), String> {
        let o = p.image_opts();
        let key = prepared_key(&p.image, &o);
        if let Some(img) = lock(&self.prepared).get(&key) {
            return Ok((key, img.clone()));
        }
        let base = self.base(&p.image)?;
        let mut img = orient_image(&base.img, o.rotate.unwrap_or(0), o.mirror.unwrap_or(false));
        if let Some(c) = o.crop {
            img = crop_image(&img, c)?;
        }
        let img = Arc::new(apply_tone(&img, &Tone::from_opts(&o)));
        put(&self.prepared, PREPARED_CACHE_MAX, key.clone(), img.clone());
        Ok((key, img))
    }

    fn cells(&self, key: &str, img: &LinearLuma, l: &squama::Layout) -> Result<Arc<CellBrightness>, String> {
        let ck: CellKey = (key.to_owned(), l.nx, l.ny, l.crop.map(|c| c.map(f64::to_bits)));
        if let Some(cb) = lock(&self.cells).get(&ck) {
            return Ok(cb.clone());
        }
        let cb = Arc::new(cell_brightness(img, l.nx, l.ny, l.crop).map_err(core_err)?);
        put(&self.cells, CELL_CACHE_MAX, ck, cb.clone());
        Ok(cb)
    }

    /// Layout and target grid for a request (cached per image settings and grid).
    pub fn target(&self, p: &SquamaParamsM1) -> Result<(squama::Layout, Arc<CellBrightness>), String> {
        let (key, img) = self.prepared(p)?;
        let l = squama::layout(&p.core_params(), (img.w, img.h)).map_err(core_err)?;
        let cb = self.cells(&key, &img, &l)?;
        Ok((l, cb))
    }

    pub fn image_info(&self, path: &str) -> Result<ImageInfo, String> {
        let b = self.base(path)?;
        Ok(ImageInfo {
            path: path.to_owned(),
            width_px: b.orig.0,
            height_px: b.orig.1,
            working_width_px: b.img.w,
            working_height_px: b.img.h,
        })
    }

    /// Grey thumbnail: `[w u32][h u32]` then `w·h` sRGB bytes, top row first.
    pub fn thumbnail(&self, path: &str, max_px: u32, rotate: u32, mirror: bool) -> Result<Vec<u8>, String> {
        let b = self.base(path)?;
        let small = downsample(&b.img, max_px.clamp(16, WORKING_MAX_PX));
        let img = orient_image(&small, rotate, mirror);
        let mut out = Vec::with_capacity(8 + img.px.len());
        out.extend_from_slice(&img.w.to_le_bytes());
        out.extend_from_slice(&img.h.to_le_bytes());
        out.extend_from_slice(&to_srgb8(&img.px));
        Ok(out)
    }

    /// One preview/final generation. Returns the payload and the job info.
    pub fn generate(&self, dto: &SquamaParamsM1, token: &CancelToken) -> Result<(Vec<u8>, JobInfo), String> {
        let t0 = Instant::now();
        let p = dto.core_params();
        let (l, cb) = self.target(dto)?;
        token.check().map_err(core_err)?;
        // MERGE: wire to ad3d-core::squama::squama_preview(p, img, quality, cancel) (§12), which adds
        // the optimiser at Quality::Final, hex layouts and point lights.
        let out = squama::generate_ex(&p, &cb, &GenOptions::default(), token).map_err(core_err)?;
        let mut warnings = dto.unsupported_warnings();
        warnings.extend(out.warnings.iter().map(|w| WarningDto::new(w.code, w.message.clone())));
        let validated = if dto.is_final() && cfg!(debug_assertions) {
            let rep = validate(&out.mesh, &squama::validate_opts(&out, ad3d_core::orient::Xform::IDENTITY));
            if !rep.ok {
                warnings.push(WarningDto::new("validate_failed", format!("validate(): {}", rep.failures.join("; "))));
            }
            Some(rep.ok)
        } else {
            None
        };
        let gen_ms = t0.elapsed().as_secs_f64() * 1e3;
        let mut buf = payload::encode(&out.mesh, dto.job_id, gen_ms as f32);
        if dto.is_final() {
            // MERGE: wire to the core's payload encoder once it takes flags (§12.2).
            set_payload_flags(&mut buf, FLAG_FINAL);
        }
        let info = JobInfo {
            job_id: dto.job_id,
            quality: dto.quality,
            layout: LayoutInfo { kind: "square", nx: l.nx, ny: l.ny, w_mm: l.w_mm, h_mm: l.h_mm, cell_mm: l.cell_mm },
            warnings,
            gen_ms,
            triangles: out.mesh.triangle_count() as u32,
            validated,
        };
        Ok((buf, info))
    }

    fn remember(&self, info: JobInfo) {
        let mut jobs = lock(&self.jobs);
        if jobs.len() >= JOB_INFO_MAX {
            jobs.pop_front();
        }
        jobs.push_back(info);
    }

    pub fn job_info(&self, job_id: u32) -> Option<JobInfo> {
        lock(&self.jobs).iter().rev().find(|j| j.job_id == job_id).cloned()
    }

    /// The whole export: plan, generate each part at final quality, validate, stream to disk.
    // MERGE: wire to ad3d-core::squama::squama_export (§12, §14) with a PartSink that writes
    // STL/3MF; keys, splints, stand and 3MF arrive with it.
    pub fn export(
        &self,
        e: &SquamaExportDto,
        cancel: &CancelToken,
        emit: &mut dyn FnMut(ExportEvent),
    ) -> Result<ExportResult, String> {
        let t0 = Instant::now();
        let mut params = e.params.clone();
        params.quality = QualityDto::Final;
        let p = params.core_params();
        let mut warnings: Vec<WarningDto> = Vec::new();
        if e.formats.is_empty() {
            return Err("choose at least one format (STL or 3MF)".into());
        }
        let out_dir = PathBuf::from(&e.out_dir);
        if !out_dir.is_dir() {
            return Err(format!("{} is not a folder", out_dir.display()));
        }
        let base = sanitize_base(e.base_name.as_deref().unwrap_or("shadow_plate"));
        if e.keys == Some(KeysDto::Pegs) && p.base_mm < 2.8 - 1e-9 {
            return Err(format!(
                "invalid parameters: alignment keys need thickness ≥ 2.8 mm (pocket floor 0.8 mm), got {} mm",
                p.base_mm
            ));
        }
        for w in params.unsupported_warnings() {
            push_warning(&mut warnings, w, emit);
        }
        if e.formats.contains(&ExportFormat::ThreeMf) {
            push_warning(
                &mut warnings,
                WarningDto::new("3mf_pending", "The 3MF writer is not in this build yet; wrote binary STL instead."),
                emit,
            );
        }
        if e.keys == Some(KeysDto::Pegs) {
            push_warning(
                &mut warnings,
                WarningDto::new("keys_pending", "Alignment keys are not in this build yet; tiles have no peg holes."),
                emit,
            );
        }
        if e.stand == Some(true) {
            push_warning(
                &mut warnings,
                WarningDto::new("stand_pending", "The stand is not in this build yet; no stand part was written."),
                emit,
            );
        }

        let (l, cb) = self.target(&params)?;
        let orient = e.orientation.core();
        let margin = e.margin_mm.unwrap_or(5.0).max(0.0);
        let tiles = match e.bed_mm {
            Some(bed) => {
                let max_uv = max_tile_extent(orient, bed, margin, p.light_azimuth);
                Some(plan_tiles_square(l.nx, l.ny, l.cell_mm, max_uv)?)
            }
            None => None,
        };
        let grid = tiles.clone().unwrap_or(TileGrid { cols: vec![l.nx], rows: vec![l.ny] });

        // Part list: (name, brightness grid).
        let mut jobs: Vec<(String, CellBrightness)> = Vec::new();
        if e.calibration_tile == Some(true) {
            jobs.push((format!("{base}_calibration"), calibration_grid(p.light_azimuth)));
        }
        let single = grid.cols.len() == 1 && grid.rows.len() == 1;
        let mut j0 = 0;
        for (rj, &rh) in grid.rows.iter().enumerate() {
            let mut i0 = 0;
            for (ci, &cw) in grid.cols.iter().enumerate() {
                // Rows are named from the top so "r1" is the top row as the plate hangs.
                let name = if single { base.clone() } else { format!("{base}_r{}c{}", grid.rows.len() - rj, ci + 1) };
                jobs.push((name, sub_grid(&cb, i0, j0, cw, rh)));
                i0 += cw;
            }
            j0 += rh;
        }
        let total = jobs.len() as u32;
        emit(ExportEvent::Started { total_parts: total });

        let mut parts = Vec::new();
        let mut files = Vec::new();
        for (idx, (name, g)) in jobs.into_iter().enumerate() {
            let idx = idx as u32;
            let progress = |stage: &str, f: f64, emit: &mut dyn FnMut(ExportEvent)| {
                emit(ExportEvent::Progress {
                    part: name.clone(),
                    index: idx,
                    total_parts: total,
                    stage: stage.to_owned(),
                    fraction: (idx as f64 + f) / total as f64,
                })
            };
            cancel.check().map_err(core_err)?;
            progress("generating", 0.0, emit);
            let mut tp = p;
            tp.width_mm = g.nx as f64 * l.cell_mm;
            tp.height_mm = None;
            let out = squama::generate_ex(&tp, &g, &GenOptions::default(), cancel).map_err(core_err)?;
            for w in &out.warnings {
                push_warning(&mut warnings, WarningDto::new(w.code, w.message.clone()), emit);
            }
            cancel.check().map_err(core_err)?;
            progress("validating", 0.4, emit);
            let (x, _) = export_xform(orient, out.layout.w_mm, out.layout.h_mm, p.light_azimuth);
            let rep = validate(&out.mesh, &squama::validate_opts(&out, x));
            if !rep.ok {
                return Err(format!("{name}: validate() failed, nothing written for it: {}", rep.failures.join("; ")));
            }
            cancel.check().map_err(core_err)?;
            progress("writing", 0.7, emit);
            let path = out_dir.join(format!("{name}.stl"));
            write_stl(&path, &out.mesh, &x).map_err(core_err)?;
            let path_s = path.display().to_string();
            emit(ExportEvent::PartWritten {
                part: name.clone(),
                path: path_s.clone(),
                triangles: out.mesh.triangle_count() as u32,
            });
            let size = print_size(&out.mesh, &x);
            let fits = e.bed_mm.map(|bed| fits_bed(size, bed, margin, orient));
            if fits == Some(false) {
                push_warning(
                    &mut warnings,
                    WarningDto::new(
                        "part_exceeds_bed",
                        format!("{name} is {:.0} × {:.0} × {:.0} mm and does not fit the bed", size[0], size[1], size[2]),
                    ),
                    emit,
                );
            }
            files.push(path_s.clone());
            parts.push(PartReport {
                name: name.clone(),
                files: vec![path_s],
                triangles: out.mesh.triangle_count() as u32,
                size_mm: size,
                fits_bed: fits,
                validated: true,
            });
            progress("done", 1.0, emit);
        }
        Ok(ExportResult {
            out_dir: out_dir.display().to_string(),
            files,
            parts,
            tiles,
            warnings,
            elapsed_ms: t0.elapsed().as_secs_f64() * 1e3,
        })
    }
}

/// Records and emits a warning once per code.
fn push_warning(ws: &mut Vec<WarningDto>, w: WarningDto, emit: &mut dyn FnMut(ExportEvent)) {
    if ws.iter().any(|x| x.code == w.code) {
        return;
    }
    emit(ExportEvent::Warning { code: w.code.clone(), message: w.message.clone() });
    ws.push(w);
}

pub fn set_payload_flags(buf: &mut [u8], flags: u32) {
    if buf.len() >= PAYLOAD_FLAGS_OFFSET + 4 {
        buf[PAYLOAD_FLAGS_OFFSET..PAYLOAD_FLAGS_OFFSET + 4].copy_from_slice(&flags.to_le_bytes());
    }
}

/// Keeps a file stem safe: no separators, no leading dots, at most 96 characters.
pub fn sanitize_base(s: &str) -> String {
    let stem = Path::new(s).file_stem().and_then(|x| x.to_str()).unwrap_or("");
    let clean: String = stem
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | ' ' | '.') { c } else { '_' })
        .collect::<String>()
        .trim_start_matches('.')
        .trim()
        .chars()
        .take(96)
        .collect();
    if clean.is_empty() { "shadow_plate".into() } else { clean }
}

fn print_size(m: &ad3d_core::Mesh, x: &ad3d_core::orient::Xform) -> [f64; 3] {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for v in 0..m.vertex_count() as u32 {
        let p = x.apply_f32(m.vertex(v));
        for k in 0..3 {
            lo[k] = lo[k].min(p[k] as f64);
            hi[k] = hi[k].max(p[k] as f64);
        }
    }
    [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]]
}

/// Footprint may be turned on the bed (the slicer can rotate about Z); height must fit Z.
pub fn fits_bed(size: [f64; 3], bed: [f64; 3], margin: f64, _o: Orientation) -> bool {
    let (bx, by, bz) = (bed[0] - 2.0 * margin, bed[1] - 2.0 * margin, bed[2] - margin);
    let eps = 1e-6;
    let flat_ok = (size[0] <= bx + eps && size[1] <= by + eps) || (size[0] <= by + eps && size[1] <= bx + eps);
    flat_ok && size[2] <= bz + eps
}

async fn blocking<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| format!("worker failed: {e}"))?
}

// ---------------------------------------------------------------------------------------------
// Commands.
// ---------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn generate_squama(params: SquamaParamsM1, state: State<'_, Arc<SquamaState>>) -> Result<Response, String> {
    let state: Arc<SquamaState> = state.inner().clone();
    let seq = state.newest_job.fetch_add(1, Ordering::SeqCst) + 1;
    let token = CancelToken::new();
    if let Some((_, old)) = lock(&state.running).replace((seq, token.clone())) {
        old.cancel();
    }
    blocking(move || {
        let result = if state.newest_job.load(Ordering::SeqCst) != seq {
            Err("cancelled".to_owned())
        } else {
            state.generate(&params, &token).map(|(buf, info)| {
                state.remember(info);
                Response::new(buf)
            })
        };
        let mut running = lock(&state.running);
        if running.as_ref().is_some_and(|(s, _)| *s == seq) {
            *running = None;
        }
        result
    })
    .await
}

#[tauri::command]
pub fn squama_job_info(job_id: u32, state: State<'_, Arc<SquamaState>>) -> Option<JobInfo> {
    state.job_info(job_id)
}

#[tauri::command]
pub async fn squama_target(params: SquamaParamsM1, state: State<'_, Arc<SquamaState>>) -> Result<Response, String> {
    let state: Arc<SquamaState> = state.inner().clone();
    blocking(move || {
        let (l, cb) = state.target(&params)?;
        Ok(Response::new(encode_target(&cb, l.nx, l.ny)))
    })
    .await
}

pub fn encode_target(cb: &CellBrightness, nx: u32, ny: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(16 + 4 * cb.g.len());
    for v in [TARGET_MAGIC, nx, ny, 0] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for g in &cb.g {
        out.extend_from_slice(&g.to_le_bytes());
    }
    out
}

#[tauri::command]
pub async fn squama_image_open(path: String, state: State<'_, Arc<SquamaState>>) -> Result<ImageInfo, String> {
    let state: Arc<SquamaState> = state.inner().clone();
    blocking(move || state.image_info(&path)).await
}

#[tauri::command]
pub async fn squama_image_thumbnail(
    path: String,
    max_px: u32,
    rotate: u32,
    mirror: bool,
    state: State<'_, Arc<SquamaState>>,
) -> Result<Response, String> {
    let state: Arc<SquamaState> = state.inner().clone();
    blocking(move || state.thumbnail(&path, max_px, rotate, mirror).map(Response::new)).await
}

#[tauri::command]
pub async fn export_squama(
    export: SquamaExportDto,
    on_event: Channel<ExportEvent>,
    state: State<'_, Arc<SquamaState>>,
) -> Result<ExportResult, String> {
    let state: Arc<SquamaState> = state.inner().clone();
    let token = CancelToken::new();
    {
        let mut slot = lock(&state.export);
        if slot.is_some() {
            return Err("an export is already running".into());
        }
        *slot = Some(token.clone());
    }
    let st = state.clone();
    let result = blocking(move || {
        let mut emit = |ev: ExportEvent| {
            let _ = on_event.send(ev);
        };
        st.export(&export, &token, &mut emit)
    })
    .await;
    *lock(&state.export) = None;
    result
}

#[tauri::command]
pub fn cancel_export(state: State<'_, Arc<SquamaState>>) -> bool {
    match lock(&state.export).as_ref() {
        Some(t) => {
            t.cancel();
            true
        }
        None => false,
    }
}

/// The command `reveal_in_finder` would run, or an error for a path that does not exist.
pub fn reveal_command(path: &Path) -> Result<std::process::Command, String> {
    if !path.exists() {
        return Err(format!("{} does not exist", path.display()));
    }
    #[cfg(target_os = "macos")]
    {
        let mut c = std::process::Command::new("/usr/bin/open");
        c.arg("-R").arg(path);
        Ok(c)
    }
    #[cfg(target_os = "windows")]
    {
        let mut c = std::process::Command::new("explorer");
        c.arg(format!("/select,{}", path.display()));
        Ok(c)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let dir = if path.is_dir() { path } else { path.parent().unwrap_or(path) };
        let mut c = std::process::Command::new("xdg-open");
        c.arg(dir);
        Ok(c)
    }
}

#[tauri::command]
pub async fn reveal_in_finder(path: String) -> Result<(), String> {
    blocking(move || {
        let mut c = reveal_command(Path::new(&path))?;
        c.spawn().map(|_| ()).map_err(|e| format!("could not reveal {path}: {e}"))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use ad3d_core::io::stl::read_stl;
    use ad3d_core::mesh::validate::ValidateOpts;

    fn v1_json() -> serde_json::Value {
        serde_json::json!({
            "jobId": 42, "widthMm": 300.0, "cellMm": 6.0, "lightAzimuthDeg": 180.0,
            "lightElevationDeg": 5.0, "maxTiltDeg": 10.0, "baseMm": 3.0, "image": "synthetic"
        })
    }

    fn dto() -> SquamaParamsM1 {
        serde_json::from_value(v1_json()).unwrap()
    }

    fn tmp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ad3d-squama-cmds-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn v1_payload_is_still_valid() {
        let d = dto();
        assert_eq!(d.quality, QualityDto::Preview);
        assert_eq!(d.layout, None);
        let v1 = d.to_v1();
        assert_eq!(v1.job_id, 42);
        assert!(v1.is_synthetic());
        assert!(d.unsupported_warnings().is_empty());
    }

    #[test]
    fn full_m1_payload_parses() {
        let mut j = v1_json();
        let extra = serde_json::json!({
            "quality": "final", "heightMm": 240.0,
            "layout": { "kind": "hex", "orientation": "pointy" },
            "light": { "kind": "point", "posMm": [-400.0, 150.0, 60.0] },
            "maxReliefMm": 1.5,
            "imageOpts": { "crop": [0.1, 0.2, 0.9, 0.8], "gamma": 1.2, "contrast": 1.1, "brightness": 0.05,
                           "invert": true, "autoLevels": [0.01, 0.99], "rotate": 90, "mirror": true },
            "response": { "kind": "measured", "tiltDeg": [-10.0, 0.0, 10.0], "brightness": [0.0, 0.4, 1.0] },
            "optimiser": { "kind": "shadow", "maxSweeps": 8 }
        });
        j.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let d: SquamaParamsM1 = serde_json::from_value(j).unwrap();
        assert!(d.is_final());
        assert_eq!(d.layout, Some(LayoutDto::Hex { orientation: HexOrientation::Pointy }));
        assert_eq!(d.light, Some(LightDto::Point { pos_mm: [-400.0, 150.0, 60.0] }));
        assert_eq!(d.optimiser, Some(OptimiserDto::Shadow { max_sweeps: Some(8) }));
        let o = d.image_opts();
        assert_eq!(o.rotate, Some(90));
        assert_eq!(o.auto_levels, Some([0.01, 0.99]));
        let codes: Vec<String> = d.unsupported_warnings().into_iter().map(|w| w.code).collect();
        for c in ["hex_pending", "point_light_pending", "optimiser_pending", "measured_response_pending"] {
            assert!(codes.iter().any(|x| x == c), "missing {c} in {codes:?}");
        }
        // Lambert response with autoLevels null is fine too.
        let mut j2 = v1_json();
        j2["response"] = serde_json::json!({ "kind": "lambert", "ambient": 0.1 });
        j2["imageOpts"] = serde_json::json!({ "autoLevels": null });
        j2["optimiser"] = serde_json::json!({ "kind": "none" });
        let d2: SquamaParamsM1 = serde_json::from_value(j2).unwrap();
        assert_eq!(d2.image_opts().auto_levels, None);
        assert!(d2.unsupported_warnings().is_empty());
    }

    #[test]
    fn bad_variants_are_rejected() {
        let mut j = v1_json();
        j["layout"] = serde_json::json!({ "kind": "triangle" });
        assert!(serde_json::from_value::<SquamaParamsM1>(j).is_err());
        let mut j = v1_json();
        j["quality"] = serde_json::json!("ultra");
        assert!(serde_json::from_value::<SquamaParamsM1>(j).is_err());
    }

    fn ramp(w: u32, h: u32) -> LinearLuma {
        LinearLuma { w, h, px: (0..w * h).map(|k| k as f32 / (w * h) as f32).collect() }
    }

    #[test]
    fn orientation_moves_pixels_like_the_ui() {
        // 3 wide × 2 high, values = index.
        let img = LinearLuma { w: 3, h: 2, px: vec![0., 1., 2., 3., 4., 5.] };
        let r = orient_image(&img, 90, false);
        assert_eq!((r.w, r.h), (2, 3));
        // Clockwise: the old bottom-left (3) becomes the new top-left.
        assert_eq!(r.px, vec![3., 0., 4., 1., 5., 2.]);
        let r180 = orient_image(&img, 180, false);
        assert_eq!(r180.px, vec![5., 4., 3., 2., 1., 0.]);
        let r270 = orient_image(&img, 270, false);
        assert_eq!(r270.px, vec![2., 5., 1., 4., 0., 3.]);
        let m = orient_image(&img, 0, true);
        assert_eq!(m.px, vec![2., 1., 0., 5., 4., 3.]);
        // Four quarter turns are the identity.
        let mut x = img.clone();
        for _ in 0..4 {
            x = orient_image(&x, 90, false);
        }
        assert_eq!(x, img);
    }

    #[test]
    fn crop_and_downsample() {
        let img = ramp(100, 50);
        let c = crop_image(&img, [0.1, 0.2, 0.5, 1.0]).unwrap();
        assert_eq!((c.w, c.h), (40, 40));
        assert_eq!(c.px[0], img.px[10 * 100 + 10]);
        // Degenerate crops still give one pixel.
        let c1 = crop_image(&img, [0.5, 0.5, 0.5, 0.5]).unwrap();
        assert_eq!((c1.w, c1.h), (1, 1));
        assert!(crop_image(&img, [f64::NAN, 0.0, 1.0, 1.0]).is_err());
        let big = ramp(5000, 3000);
        let d = downsample(&big, 2048);
        assert!(d.w <= 2048 && d.h <= 2048);
        let mean = |i: &LinearLuma| i.px.iter().map(|&v| v as f64).sum::<f64>() / i.px.len() as f64;
        assert!((mean(&big) - mean(&d)).abs() < 1e-3);
    }

    #[test]
    fn tone_curve_follows_section_16() {
        let img = ramp(64, 64);
        let id = Tone::from_opts(&ImageOptsDto::default());
        assert!(id.is_identity());
        assert_eq!(apply_tone(&img, &id), img);
        let inv = apply_tone(&img, &Tone::from_opts(&ImageOptsDto { invert: Some(true), ..Default::default() }));
        // Inversion happens on the perceptual value, so dark ↔ bright.
        assert!(inv.px[0] > 0.99 && inv.px[img.px.len() - 1] < 0.01);
        let srgb_mid = srgb_decode(0.5) as f32;
        let k = img.px.iter().position(|&v| v >= srgb_mid).unwrap();
        assert!((inv.px[k] - srgb_decode(0.5) as f32).abs() < 0.01);
        // Gamma > 1 brightens mid-tones and stays monotone.
        let g = apply_tone(&img, &Tone::from_opts(&ImageOptsDto { gamma: Some(2.0), ..Default::default() }));
        assert!(g.px[k] > img.px[k]);
        assert!(g.px.windows(2).all(|w| w[1] >= w[0] - 1e-6));
        // Auto-levels stretch a narrow image to the full range.
        let narrow = LinearLuma { w: 100, h: 1, px: (0..100).map(|k| srgb_decode(0.4 + 0.2 * k as f64 / 99.0) as f32).collect() };
        let al = apply_tone(&narrow, &Tone::from_opts(&ImageOptsDto { auto_levels: Some([0.0, 1.0]), ..Default::default() }));
        assert!(al.px[0] < 0.01 && al.px[99] > 0.98);
        // Out-of-domain values are clamped, not trusted.
        let t = Tone::from_opts(&ImageOptsDto { gamma: Some(100.0), contrast: Some(-1.0), brightness: Some(f64::NAN), ..Default::default() });
        assert_eq!((t.gamma, t.contrast, t.brightness), (5.0, 0.0, 0.0));
    }

    #[test]
    fn tiles_match_the_cr6_se_example() {
        // 300 × 378 plate at cell 6, light from the left, standing auto (k = 1: u → Z, v → X).
        let o = Orientation::Standing(QuarterTurns::Auto);
        let max_uv = max_tile_extent(o, [235.0, 235.0, 250.0], 5.0, std::f64::consts::PI);
        assert_eq!(max_uv, (245.0, 225.0));
        let g = plan_tiles_square(50, 63, 6.0, max_uv).unwrap();
        assert_eq!(g.cols, vec![25, 25]);
        assert_eq!(g.rows, vec![32, 31]);
        // Flat on the bed.
        let g = plan_tiles_square(50, 63, 6.0, max_tile_extent(Orientation::Flat, [235.0, 235.0, 250.0], 5.0, 0.0)).unwrap();
        assert_eq!((g.cols.len(), g.rows.len()), (2, 2));
        assert_eq!(split_even(63, 2), vec![32, 31]);
        assert_eq!(split_even(10, 3), vec![4, 3, 3]);
        assert!(plan_tiles_square(10, 10, 6.0, (5.0, 100.0)).is_err());
    }

    #[test]
    fn generate_marks_final_payloads_and_records_info() {
        let st = SquamaState::default();
        let mut d = dto();
        let (buf, info) = st.generate(&d, &CancelToken::new()).unwrap();
        assert_eq!(u32::from_le_bytes(buf[52..56].try_into().unwrap()), 0);
        assert_eq!((info.layout.nx, info.layout.ny), (50, 63));
        let (m, job, _, _) = payload::decode(&buf).unwrap();
        assert_eq!(job, 42);
        let b = m.bounds().unwrap();
        assert!((b.max[0] - 300.0).abs() < 1e-3 && (b.max[1] - 378.0).abs() < 1e-3);
        d.quality = QualityDto::Final;
        d.job_id = 43;
        let (buf, info) = st.generate(&d, &CancelToken::new()).unwrap();
        assert_eq!(u32::from_le_bytes(buf[52..56].try_into().unwrap()) & FLAG_FINAL, FLAG_FINAL);
        assert_eq!(info.validated, if cfg!(debug_assertions) { Some(true) } else { None });
        st.remember(info.clone());
        assert_eq!(st.job_info(43), Some(info));
        assert_eq!(st.job_info(7), None);
        // A cancelled token gives no output.
        let t = CancelToken::new();
        t.cancel();
        assert!(st.generate(&d, &t).is_err());
    }

    #[test]
    fn crop_and_rotation_change_the_grid() {
        let st = SquamaState::default();
        let mut d = dto();
        // Rotating the 500×630 synthetic target by 90° makes it landscape.
        d.image_opts = Some(ImageOptsDto { rotate: Some(90), ..Default::default() });
        let (l, cb) = st.target(&d).unwrap();
        assert_eq!((l.nx, l.ny), (50, 40));
        assert_eq!(cb.g.len(), 50 * 40);
        // A square crop gives a square grid.
        d.image_opts = Some(ImageOptsDto { crop: Some([0.0, 0.0, 1.0, 500.0 / 630.0]), ..Default::default() });
        let (l, _) = st.target(&d).unwrap();
        assert_eq!((l.nx, l.ny), (50, 50));
        let bytes = encode_target(&cb, 50, 40);
        assert_eq!(u32::from_le_bytes(bytes[0..4].try_into().unwrap()), TARGET_MAGIC);
        assert_eq!(bytes.len(), 16 + 4 * 50 * 40);
    }

    #[test]
    fn thumbnail_header_and_size() {
        let st = SquamaState::default();
        let t = st.thumbnail(SYNTHETIC_KEY, 128, 90, false).unwrap();
        let w = u32::from_le_bytes(t[0..4].try_into().unwrap());
        let h = u32::from_le_bytes(t[4..8].try_into().unwrap());
        assert!(w > h && w <= 128 && h <= 128, "{w}×{h}");
        assert_eq!(t.len(), 8 + (w * h) as usize);
        let info = st.image_info(SYNTHETIC_KEY).unwrap();
        assert_eq!((info.width_px, info.height_px), SYNTHETIC_PX);
        assert!(st.image_info("/no/such/photo.png").is_err());
    }

    fn export_dto(dir: &Path) -> SquamaExportDto {
        SquamaExportDto {
            params: dto(),
            out_dir: dir.display().to_string(),
            base_name: Some("plate.3mf".into()),
            formats: vec![ExportFormat::Stl, ExportFormat::ThreeMf],
            orientation: OrientationDto::StandingAuto,
            bed_mm: Some([235.0, 235.0, 250.0]),
            margin_mm: Some(5.0),
            keys: None,
            stand: Some(true),
            calibration_tile: Some(true),
        }
    }

    #[test]
    fn export_writes_valid_tiles_and_reports_progress() {
        let dir = tmp_dir("export");
        let st = SquamaState::default();
        let e = export_dto(&dir);
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["orientation"], "standing-auto");
        assert_eq!(json["formats"], serde_json::json!(["stl", "3mf"]));
        let mut events = Vec::new();
        let r = st.export(&e, &CancelToken::new(), &mut |ev| events.push(ev)).unwrap();
        // 2 × 2 tiles + calibration tile.
        assert_eq!(r.parts.len(), 5);
        assert_eq!(r.tiles, Some(TileGrid { cols: vec![25, 25], rows: vec![32, 31] }));
        assert!(r.files.iter().any(|f| f.ends_with("plate_r1c1.stl")));
        assert!(r.files.iter().any(|f| f.ends_with("plate_calibration.stl")));
        for c in ["3mf_pending", "stand_pending"] {
            assert!(r.warnings.iter().any(|w| w.code == c), "missing {c}");
        }
        assert!(matches!(events.first(), Some(ExportEvent::Warning { .. })));
        assert!(events.iter().any(|e| matches!(e, ExportEvent::Started { total_parts: 5 })));
        assert_eq!(events.iter().filter(|e| matches!(e, ExportEvent::PartWritten { .. })).count(), 5);
        for p in &r.parts {
            assert_eq!(p.fits_bed, Some(true), "{} {:?}", p.name, p.size_mm);
            let m = read_stl(Path::new(&p.files[0])).unwrap();
            let rep = validate(&m, &ValidateOpts { soup: true, ..Default::default() });
            assert!(rep.written.boundary_edges == 0 && rep.written.nonmanifold_edges == 0, "{}: {}", p.name, rep.summary());
            assert!(rep.written.inconsistent_winding_edges == 0 && rep.written.signed_volume > 0.0);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_refuses_bad_requests_and_honours_cancel() {
        let dir = tmp_dir("refuse");
        let st = SquamaState::default();
        let mut e = export_dto(&dir);
        e.formats.clear();
        assert!(st.export(&e, &CancelToken::new(), &mut |_| {}).is_err());
        let mut e = export_dto(&dir);
        e.keys = Some(KeysDto::Pegs);
        e.params.base_mm = 2.0;
        let err = st.export(&e, &CancelToken::new(), &mut |_| {}).unwrap_err();
        assert!(err.contains("2.8"), "{err}");
        let mut e = export_dto(&dir);
        e.out_dir = dir.join("missing").display().to_string();
        assert!(st.export(&e, &CancelToken::new(), &mut |_| {}).is_err());
        let t = CancelToken::new();
        t.cancel();
        let err = st.export(&export_dto(&dir), &t, &mut |_| {}).unwrap_err();
        assert_eq!(err, "cancelled");
        assert!(std::fs::read_dir(&dir).unwrap().next().is_none(), "cancelled export wrote files");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_and_reveal() {
        assert_eq!(sanitize_base("plate.3mf"), "plate");
        assert_eq!(sanitize_base("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_base(".hidden"), "shadow_plate");
        assert_eq!(sanitize_base("a/b:c*?.stl"), "b_c__");
        assert!(reveal_command(Path::new("/definitely/not/here")).is_err());
        assert!(reveal_command(&std::env::temp_dir()).is_ok());
    }

    #[test]
    fn calibration_ramp_spans_black_to_white() {
        let g = calibration_grid(std::f64::consts::PI);
        assert_eq!((g.nx, g.ny), (24, 12));
        assert_eq!(g.at(0, 0), 0.0);
        assert_eq!(g.at(23, 11), 1.0);
        let g = calibration_grid(std::f64::consts::FRAC_PI_2);
        assert_eq!((g.nx, g.ny), (16, 18));
    }

    #[test]
    fn bed_fit() {
        let o = Orientation::Flat;
        assert!(fits_bed([225.0, 10.0, 240.0], [235.0, 235.0, 250.0], 5.0, o));
        assert!(!fits_bed([226.0, 10.0, 240.0], [235.0, 235.0, 250.0], 5.0, o));
        assert!(!fits_bed([200.0, 10.0, 246.0], [235.0, 235.0, 250.0], 5.0, o));
    }
}
