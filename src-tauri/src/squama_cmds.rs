//! M1 Squama commands: live preview, image intake, the 2D target view, the tile
//! plan and export. Everything geometric is the core's M1 pipeline
//! (`ad3d_core::squama::{job, pipeline, export}`); this file owns decoding,
//! caching, cancellation and file output.
//!
//! Every command that does CPU or file work is `async` and moves it to
//! `tauri::async_runtime::spawn_blocking`, so the UI thread is never blocked.
//!
//! Command contract (mirrors `src/engine/tauriEngine.ts`; DTOs follow
//! docs/design/squama-mesher.md §12.1):
//! - `generate_squama { params }`            → raw `AD3M` payload (latest-wins, cancellable;
//!   flags bit 0 = final quality, bit 1 = face kinds follow, §12.2)
//! - `squama_job_info { jobId }`             → `JobInfo | null` (layout, warnings, scores)
//! - `squama_target { params }`              → raw `AD3T` grid: per-facet target (§16), plus the
//!   simulated appearance (§15.1) when a final plate with these settings was generated
//! - `squama_tile_plan { export }`           → `TilePlanDto` (what an export would tile)
//! - `squama_image_open { path }`            → `ImageInfo` (decodes and caches the photo)
//! - `squama_image_thumbnail { path, maxPx, rotate, mirror }` → raw grey thumbnail
//! - `export_squama { export, onEvent }`     → `ExportResult`; progress over a `Channel`
//! - `cancel_generate`                       → `bool` (aborts the plate in flight)
//! - `cancel_export`                         → `bool` (true when an export was running)
//! - `reveal_in_finder { path }`             → `()`

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use ad3d_core::image::working::{DEFAULT_MAX_SIDE, DRAG_MAX_SIDE};
use ad3d_core::image::{ImageOpts, LinearLuma, TargetPipeline, build_working};
use ad3d_core::image::target::build_sampler;
use ad3d_core::io::sink::ThreeMfSink;
use ad3d_core::io::stl::write_stl_cancellable;
use ad3d_core::io::threemf::Meta;
use ad3d_core::mesh::validate::validate;
use ad3d_core::orient::BedMotion;
use ad3d_core::relief::Partition;
use ad3d_core::squama::export::{
    ExportReport, IoSink, KeyKind, PartReport as CorePartReport, PartSink, SquamaExport, TileRequest, export_plate,
    parse_orientation, preview_tile_plan,
};
use ad3d_core::squama::job::{QualityDto, SquamaJob};
use ad3d_core::squama::parts::Part;
use ad3d_core::squama::pipeline::{facet_raster, job_layout, job_target, plate_from_target, synthetic_image};
use ad3d_core::squama::solve::Quality;
use ad3d_core::squama::tiles::Bed;
use ad3d_core::squama::{GenOptions, Layout, LayoutKind, SquamaParamsDto, build_partition};
use ad3d_core::{CancelToken, Error as CoreError, payload};
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::{Channel, Response};

/// Cache key of the built-in synthetic target (the DTO's `image: "synthetic"`).
pub const SYNTHETIC_KEY: &str = "synthetic";

/// The §12.1 request: the core's DTO (v1 fields required, M1 fields optional).
pub type SquamaParamsM1 = SquamaParamsDto;

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

impl From<&ad3d_core::error::Warning> for WarningDto {
    fn from(w: &ad3d_core::error::Warning) -> Self {
        WarningDto::new(w.code, w.message.clone())
    }
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
    None,
    Pegs,
    /// Pegs when the plate is tiled and the base leaves room (the default).
    #[default]
    Auto,
}

/// §12.1 `SquamaExport`. `baseName` (the file stem from the save dialog) and `bedMotion`
/// (the printer's moving-bed axis, default `y` for the CR-6 SE) are AD3D additions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SquamaExportDto {
    pub params: SquamaParamsM1,
    pub out_dir: String,
    #[serde(default)]
    pub base_name: Option<String>,
    pub formats: Vec<ExportFormat>,
    pub orientation: String,
    #[serde(default)]
    pub bed_mm: Option<[f64; 3]>,
    #[serde(default)]
    pub margin_mm: Option<f64>,
    #[serde(default)]
    pub bed_motion: Option<BedMotion>,
    #[serde(default)]
    pub keys: Option<KeysDto>,
    #[serde(default)]
    pub stand: Option<bool>,
    #[serde(default)]
    pub calibration_tile: Option<bool>,
}

impl SquamaExportDto {
    /// Core export options. Defaults: CR-6 SE bed (bed moves in Y), keys when tiled, no stand.
    /// Without `bedMm` the plate is exported in one piece (the UI's "tile to bed" off).
    pub fn options(&self) -> Result<SquamaExport, String> {
        let mut bed = Bed::CR6_SE;
        if let Some([x, y, z]) = self.bed_mm {
            (bed.x_mm, bed.y_mm, bed.z_mm) = (x, y, z);
        }
        if let Some(m) = self.margin_mm {
            bed.margin_mm = m.max(0.0);
        }
        if let Some(m) = self.bed_motion {
            bed.motion = m;
        }
        Ok(SquamaExport {
            orientation: parse_orientation(&self.orientation).map_err(core_err)?,
            bed,
            keys: match self.keys.unwrap_or_default() {
                KeysDto::None => KeyKind::None,
                KeysDto::Pegs => KeyKind::Pegs,
                KeysDto::Auto => KeyKind::Auto,
            },
            stand: self.stand.unwrap_or(false),
            calibration_tile: self.calibration_tile.unwrap_or(false),
            tiles: if self.bed_mm.is_some() { TileRequest::Auto } else { TileRequest::None },
            ..SquamaExport::default()
        })
    }
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
    /// Which bed (print job / 3MF file) the part is on, from 0.
    pub bed: u32,
    /// Largest downward-facing ledge (standing tiles, calibration tile).
    pub max_downward_ledge_mm: Option<f64>,
    /// Height / smallest footprint side; above 10 the part wants a brim.
    pub height_ratio: f64,
}

/// Cells (square) or lattice units (hex) per tile column and row, from the plate's
/// lower left.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TileGrid {
    pub cols: Vec<u32>,
    pub rows: Vec<u32>,
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
    /// Match score before → after the optimiser, when it ran.
    pub optimise: Option<ad3d_core::squama::export::ExportOptimise>,
    /// Beds (print jobs) the parts need; one 3MF per bed.
    pub beds: u32,
    /// Print advice for tall, thin standing parts (brim, perimeters, layers).
    pub brim: Option<String>,
}

/// What `squama_tile_plan` returns.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TilePlanDto {
    pub cols: Vec<u32>,
    pub rows: Vec<u32>,
    /// Tile boundaries in plate mm (`cols + 1` / `rows + 1` values).
    pub u_edges_mm: Vec<f64>,
    pub v_edges_mm: Vec<f64>,
    pub quarter_turns: Option<u32>,
    /// Print-space size (X, Y, Z) of every tile, row-major from the bottom row.
    pub print_mm: Vec<[f64; 3]>,
    /// Plate thickness the plan used.
    pub thickness_mm: f64,
    pub w_mm: f64,
    pub h_mm: f64,
}

// ---------------------------------------------------------------------------------------------
// Job info and caches.
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutInfo {
    pub kind: &'static str,
    /// `pointy` / `flat` for hex layouts.
    pub orientation: Option<&'static str>,
    pub nx: u32,
    pub ny: u32,
    pub w_mm: f64,
    pub h_mm: f64,
    pub cell_mm: f64,
    pub facets: u32,
}

impl LayoutInfo {
    fn of(l: &Layout) -> Self {
        let (kind, orientation) = match l.kind {
            LayoutKind::Square => ("square", None),
            LayoutKind::Hex { orientation } => (
                "hex",
                Some(if orientation == ad3d_core::squama::HexOrientation::Flat { "flat" } else { "pointy" }),
            ),
        };
        LayoutInfo {
            kind,
            orientation,
            nx: l.nx,
            ny: l.ny,
            w_mm: l.w_mm,
            h_mm: l.h_mm,
            cell_mm: l.cell_mm,
            facets: l.facet_count(),
        }
    }
}

/// Match of the simulated appearance against the target (§15.1).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreDto {
    pub pearson: f64,
    pub rmse_rel: f64,
    /// Visible contrast p99 / p1; `null` when the darkest facets are fully black.
    pub contrast: Option<f64>,
}

impl From<&ad3d_core::sim::MatchScore> for ScoreDto {
    fn from(s: &ad3d_core::sim::MatchScore) -> Self {
        ScoreDto { pearson: s.pearson, rmse_rel: s.rmse_rel, contrast: s.contrast.is_finite().then_some(s.contrast) }
    }
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
    /// Achievable contrast `(B_hi − B_lo) / B_hi` and the tilt cap actually used.
    pub contrast: f64,
    pub max_tilt_used_deg: f64,
    /// Final quality: the simulated appearance's match (after the optimiser when it ran).
    pub score: Option<ScoreDto>,
    pub score_before: Option<ScoreDto>,
    pub optimiser_sweeps: Option<u32>,
    pub timings: ad3d_core::squama::pipeline::PipelineTimings,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageInfo {
    pub path: String,
    pub width_px: u32,
    pub height_px: u32,
    /// Size of the working copy (≤ 2048 on the long side).
    pub working_width_px: u32,
    pub working_height_px: u32,
}

const SOURCE_CACHE_MAX: usize = 4;
/// Pixel memory the decoded-source cache may hold (each source keeps ≤ 4096 px levels).
const SOURCE_CACHE_BYTES: usize = 256 << 20;
const PARTITION_CACHE_MAX: usize = 4;
const JOB_INFO_MAX: usize = 16;
const SIM_CACHE_MAX: usize = 4;

/// `AD3T` target grid: magic, nx, ny, flags (bit 0 = a simulated grid follows), then
/// `nx·ny` f32 (row 0 at `v = 0`), then the simulated grid when flagged.
pub const TARGET_MAGIC: u32 = 0x5433_4441;
pub const TARGET_HAS_SIM: u32 = 1;

/// One decoded source image and its cached §16 stages.
struct Source {
    orig: (u32, u32),
    pipeline: Mutex<TargetPipeline>,
}

type PartKey = (u8, u32, u32, u64);

#[derive(Default)]
pub struct SquamaState {
    newest_job: AtomicU64,
    newest_target: AtomicU64,
    running: Mutex<Option<(u64, CancelToken)>>,
    sources: Mutex<Vec<(String, Arc<Source>)>>,
    partitions: Mutex<Vec<(PartKey, Arc<Partition>)>>,
    /// Simulated appearance raster of recent final plates, by settings key.
    sims: Mutex<VecDeque<(String, Arc<(u32, u32, Vec<f32>)>)>>,
    jobs: Mutex<VecDeque<JobInfo>>,
    export: Mutex<Option<CancelToken>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn core_err(e: CoreError) -> String {
    e.to_string()
}

/// Decodes a photo (§16 steps 1–2). HEIC goes through `/usr/bin/sips` on macOS.
pub fn decode_image(path: &str) -> Result<LinearLuma, String> {
    if path == SYNTHETIC_KEY {
        return Ok(synthetic_image());
    }
    let p = Path::new(path);
    #[cfg(target_os = "macos")]
    if ad3d_core::image::heic::needs_sips(p) {
        return ad3d_core::image::heic::decode_with_sips(p).map_err(core_err);
    }
    LinearLuma::load(p).map_err(|e| {
        let m = e.to_string();
        if m.contains("limit") {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            format!("{name} is too large to open (over the 512 MB decoder limit, roughly 100 megapixels); scale it down and try again")
        } else {
            m
        }
    })
}

/// Key of everything that shapes a plate except the job id and quality.
fn settings_key(dto: &SquamaParamsM1) -> String {
    let mut d = dto.clone();
    d.job_id = 0;
    d.quality = None;
    serde_json::to_string(&d).unwrap_or_default()
}

fn part_key(l: &Layout) -> PartKey {
    let kind = match l.kind {
        LayoutKind::Square => 0,
        LayoutKind::Hex { orientation } => 1 + (orientation == ad3d_core::squama::HexOrientation::Flat) as u8,
    };
    (kind, l.nx, l.ny, l.cell_mm.to_bits())
}

impl SquamaState {
    fn source(&self, image: &str) -> Result<Arc<Source>, String> {
        if let Some((_, s)) = lock(&self.sources).iter().find(|(k, _)| k == image) {
            return Ok(s.clone());
        }
        // Decode outside the lock; two racing decodes of one path are harmless.
        let img = decode_image(image)?;
        let s = Arc::new(Source { orig: (img.w, img.h), pipeline: Mutex::new(TargetPipeline::new(Arc::new(img))) });
        let mut c = lock(&self.sources);
        c.retain(|(k, _)| k != image);
        c.insert(0, (image.to_owned(), s.clone()));
        // Keep at most SOURCE_CACHE_MAX sources and SOURCE_CACHE_BYTES of pixels (the
        // newest always stays).
        let mut bytes = 0usize;
        let mut keep = 0usize;
        for (_, src) in c.iter() {
            let b = lock(&src.pipeline).source().px.len() * 4;
            if keep > 0 && (keep >= SOURCE_CACHE_MAX || bytes + b > SOURCE_CACHE_BYTES) {
                break;
            }
            bytes += b;
            keep += 1;
        }
        c.truncate(keep.max(1));
        Ok(s)
    }

    /// The image-independent cell partition, cached per layout (§13).
    fn partition(&self, l: &Layout) -> Result<Arc<Partition>, String> {
        let k = part_key(l);
        if let Some((_, p)) = lock(&self.partitions).iter().find(|(kk, _)| *kk == k) {
            return Ok(p.clone());
        }
        let p = Arc::new(build_partition(l, &[]).map_err(core_err)?);
        let mut c = lock(&self.partitions);
        c.insert(0, (k, p.clone()));
        c.truncate(PARTITION_CACHE_MAX);
        Ok(p)
    }

    /// Layout, partition and per-facet target for a job. Drag previews sample the
    /// 1024 px level (§16 step 6); final plates and exports the 2048 px level.
    fn prepared(
        &self,
        job: &SquamaJob,
    ) -> Result<(Layout, Arc<Partition>, ad3d_core::image::Target, Vec<ad3d_core::error::Warning>), String> {
        let src = self.source(&job.image)?;
        let gopts = GenOptions::default();
        let (l, warnings) = job_layout(job, src.orig, &gopts).map_err(core_err)?;
        let part = self.partition(&l)?;
        let side = if job.quality == Quality::Final { DEFAULT_MAX_SIDE } else { DRAG_MAX_SIDE };
        let sampler = self.sampler(&src, &job.image_opts, side)?;
        let target = job_target(&sampler, &l, &part).map_err(core_err)?;
        Ok((l, part, target, warnings))
    }

    /// The toned sampler for `opts` at `side`. A miss is built outside the source's
    /// lock (a crop or rotation of a big photo takes tens of ms), so a drag preview
    /// that needs the other level never waits behind it.
    fn sampler(&self, src: &Source, opts: &ImageOpts, side: u32) -> Result<Arc<ad3d_core::image::TargetSampler>, String> {
        let (cached, img, working) = {
            let mut p = lock(&src.pipeline);
            let c = p.cached_sampler(opts, side).map_err(core_err)?;
            (c, p.source().clone(), p.cached_working(opts, side))
        };
        if let Some(s) = cached {
            return Ok(s);
        }
        let (w, s) = build_sampler(&img, opts, side, working).map_err(core_err)?;
        lock(&src.pipeline).insert(opts, side, w, s.clone());
        Ok(s)
    }

    pub fn image_info(&self, path: &str) -> Result<ImageInfo, String> {
        let s = self.source(path)?;
        let wk = lock(&s.pipeline).working(&ImageOpts::default(), DEFAULT_MAX_SIDE).map_err(core_err)?;
        Ok(ImageInfo {
            path: path.to_owned(),
            width_px: s.orig.0,
            height_px: s.orig.1,
            working_width_px: wk.w,
            working_height_px: wk.h,
        })
    }

    /// Grey thumbnail: `[w u32][h u32]` then `w·h` sRGB bytes, top row first.
    pub fn thumbnail(&self, path: &str, max_px: u32, rotate: u32, mirror: bool) -> Result<Vec<u8>, String> {
        let s = self.source(path)?;
        let src = lock(&s.pipeline).source().clone();
        let opts = ImageOpts { rotate_deg: (rotate % 360) as i32 / 90 * 90, mirror, ..ImageOpts::default() };
        let wk = build_working(&src, &opts, max_px.clamp(16, DEFAULT_MAX_SIDE)).map_err(core_err)?;
        let mut out = Vec::with_capacity(8 + wk.v.len());
        out.extend_from_slice(&wk.w.to_le_bytes());
        out.extend_from_slice(&wk.h.to_le_bytes());
        out.extend(wk.v.iter().map(|&v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
        Ok(out)
    }

    /// Per-facet target as a raster, plus the simulated appearance of the last final
    /// plate with the same settings (§15.1). Sampled at the level the plate itself uses
    /// (the 1024 px drag level while a slider drags, else 2048 px), so it reuses the
    /// sampler the generation just built.
    pub fn target(&self, dto: &SquamaParamsM1) -> Result<Vec<u8>, String> {
        let quality = if dto.quality == Some(QualityDto::Preview) { Quality::Preview } else { Quality::Final };
        let job = dto.job().map_err(core_err)?.with_quality(quality);
        let (l, _, t, _) = self.prepared(&job)?;
        let (nx, ny, g) = facet_raster(&l, &t.g);
        let key = settings_key(dto);
        let sim = lock(&self.sims).iter().find(|(k, _)| *k == key).map(|(_, s)| s.clone());
        let sim = sim.filter(|s| s.0 == nx && s.1 == ny);
        Ok(encode_target(nx, ny, &g, sim.as_ref().map(|s| s.2.as_slice())))
    }

    /// One preview/final generation. Returns the payload and the job info.
    pub fn generate(&self, dto: &SquamaParamsM1, token: &CancelToken) -> Result<(Vec<u8>, JobInfo), String> {
        let t0 = Instant::now();
        let job = dto.job().map_err(core_err)?;
        let (l, part, target, warnings) = self.prepared(&job)?;
        token.check().map_err(core_err)?;
        let target_ms = t0.elapsed().as_secs_f64() * 1e3;
        let gopts = GenOptions::default();
        let mut out = plate_from_target(&job, &l, &part, target, warnings, &gopts, token).map_err(core_err)?;
        out.timings.target_ms = target_ms;
        let mut warnings: Vec<WarningDto> = out.warnings.iter().map(WarningDto::from).collect();
        let is_final = job.quality == Quality::Final;
        let validated = if is_final && cfg!(debug_assertions) {
            let rep = validate(&out.mesh, &out.validate_opts(ad3d_core::orient::Xform::IDENTITY));
            if !rep.ok {
                warnings.push(WarningDto::new("validate_failed", format!("validate(): {}", rep.failures.join("; "))));
            }
            Some(rep.ok)
        } else {
            None
        };
        if let Some(a) = &out.appearance {
            let raster = facet_raster(&l, &a.sim);
            let mut c = lock(&self.sims);
            let key = settings_key(dto);
            c.retain(|(k, _)| *k != key);
            c.push_front((key, Arc::new(raster)));
            c.truncate(SIM_CACHE_MAX);
        }
        let gen_ms = t0.elapsed().as_secs_f64() * 1e3;
        out.timings.total_ms = gen_ms;
        let flags = payload::FLAG_FACE_KINDS | if is_final { payload::FLAG_FINAL } else { 0 };
        let buf = payload::encode_ex(&out.mesh, dto.job_id, gen_ms as f32, flags, None);
        let info = JobInfo {
            job_id: dto.job_id,
            quality: if is_final { QualityDto::Final } else { QualityDto::Preview },
            layout: LayoutInfo::of(&l),
            warnings,
            gen_ms,
            triangles: out.mesh.triangle_count() as u32,
            validated,
            contrast: out.report.contrast,
            max_tilt_used_deg: out.report.max_tilt_used.to_degrees(),
            score: out.appearance.as_ref().map(|a| ScoreDto::from(&a.score)),
            score_before: out.appearance.as_ref().and_then(|a| a.score_before.as_ref()).map(ScoreDto::from),
            optimiser_sweeps: out.optimise.as_ref().map(|o| o.sweeps),
            timings: out.timings,
        };
        Ok((buf, info))
    }

    /// Cancels the generation in flight, if any.
    pub fn cancel_running(&self) -> bool {
        match lock(&self.running).as_ref() {
            Some((_, t)) => {
                t.cancel();
                true
            }
            None => false,
        }
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

    /// The tile plan an export with these settings would cut: the export's own
    /// planning stage (final solve, optimiser when on, refined seams) on the cached
    /// final-quality target.
    pub fn tile_plan(&self, e: &SquamaExportDto, cancel: &CancelToken) -> Result<TilePlanDto, String> {
        let job = e.params.job().map_err(core_err)?.with_quality(Quality::Final);
        let (l, _, target, _) = self.prepared(&job)?;
        let (plan, thickness) = preview_tile_plan(&job, &l, Some(&target.g), &e.options()?, cancel).map_err(core_err)?;
        let print_mm = plan
            .tiles
            .iter()
            .map(|t| {
                let (lo, hi) = t.transform.apply_box([t.rect_mm.min.x, t.rect_mm.min.y, 0.0], [t.rect_mm.max.x, t.rect_mm.max.y, thickness]);
                [0, 1, 2].map(|k| hi[k] - lo[k])
            })
            .collect();
        let counts = |u: &[u32]| u.windows(2).map(|w| w[1] - w[0]).collect::<Vec<u32>>();
        Ok(TilePlanDto {
            cols: counts(&plan.u_units),
            rows: counts(&plan.v_units),
            u_edges_mm: plan.u_edges(),
            v_edges_mm: plan.v_edges(),
            quarter_turns: plan.quarter_turns,
            print_mm,
            thickness_mm: thickness,
            w_mm: l.w_mm,
            h_mm: l.h_mm,
        })
    }

    /// The whole export: the core plans, solves (final quality, optimiser when on),
    /// meshes and validates every part; parts stream to disk one at a time.
    pub fn export(
        &self,
        e: &SquamaExportDto,
        cancel: &CancelToken,
        emit: &mut dyn FnMut(ExportEvent),
    ) -> Result<ExportResult, String> {
        let t0 = Instant::now();
        if e.formats.is_empty() {
            return Err("choose at least one format (STL or 3MF)".into());
        }
        let out_dir = PathBuf::from(&e.out_dir);
        if !out_dir.is_dir() {
            return Err(format!("{} is not a folder", out_dir.display()));
        }
        let base = sanitize_base(e.base_name.as_deref().unwrap_or("shadow_plate"));
        let opts = e.options()?;
        let job = e.params.job().map_err(core_err)?.with_quality(Quality::Final);
        cancel.check().map_err(|_| "cancelled".to_string())?;
        emit(ExportEvent::Progress { part: String::new(), index: 0, total_parts: 0, stage: "preparing the photo".into(), fraction: 0.0 });
        let (l, _, target, warnings) = self.prepared(&job)?;
        let meta = Meta {
            title: Some(base.clone()),
            description: Some("Squama shadow plate generated by AD3D".into()),
            custom: vec![
                ("generator".into(), "squama".into()),
                ("params".into(), serde_json::to_string(&e.params).unwrap_or_default()),
            ],
            ..Default::default()
        };
        let mut sink = AppSink::new(&out_dir, &base, &e.formats, &meta, cancel, emit)?;
        let report = export_plate(&job, &l, &target.g, warnings, &opts, cancel, &mut sink).map_err(|err| match err {
            CoreError::Cancelled => "cancelled".to_string(),
            err => core_err(err),
        })?;
        let mut warnings: Vec<WarningDto> = Vec::new();
        for w in &report.warnings {
            if !warnings.iter().any(|x| x.code == w.code) {
                (sink.emit)(ExportEvent::Warning { code: w.code.into(), message: w.message.clone() });
                warnings.push(w.into());
            }
        }
        if let Some(key) = &report.calibration_key {
            let p = out_dir.join(format!("{base}_calibration.key.json"));
            std::fs::write(&p, serde_json::to_string_pretty(key).map_err(|e| e.to_string())?)
                .map_err(|err| format!("{}: {err}", p.display()))?;
            sink.files.push(p.display().to_string());
        }
        Ok(ExportResult {
            out_dir: out_dir.display().to_string(),
            files: sink.files.clone(),
            parts: sink.parts.clone(),
            tiles: Some(tile_grid(&l, &report)),
            warnings,
            elapsed_ms: t0.elapsed().as_secs_f64() * 1e3,
            optimise: report.optimise,
            beds: report.beds_used,
            brim: report.brim.clone(),
        })
    }
}

/// Cells (square) or lattice units (hex) per tile column/row.
fn tile_grid(l: &Layout, r: &ExportReport) -> TileGrid {
    let (px, py) = ad3d_core::squama::tiles::lattice(l).map(|x| (x.0.x, x.0.y)).unwrap_or((l.cell_mm, l.cell_mm));
    let counts = |e: &[f64], p: f64| e.windows(2).map(|w| ((w[1] - w[0]) / p).round() as u32).collect();
    TileGrid { cols: counts(&r.u_edges_mm, px), rows: counts(&r.v_edges_mm, py) }
}

/// File name of a core part: the whole plate is `<base>`, everything else `<base>_<part>`.
fn part_file_stem(base: &str, part: &str) -> String {
    if part == "plate" { base.to_owned() } else { format!("{base}_{part}") }
}

/// Streams validated parts: one binary STL per part (atomic, cancellable) and/or one
/// 3MF per bed with that bed's parts (`<base>.3mf`, then `<base>_bed2.3mf`, …). The
/// core places every part in its own bed's coordinates, so each file opens on the
/// printer's plate as laid out. A failed or cancelled export removes what it wrote.
struct AppSink<'a> {
    dir: PathBuf,
    base: String,
    stl: bool,
    tmf: bool,
    meta: Meta,
    /// One 3MF sink per bed, opened when the bed's first part arrives.
    beds: Vec<Option<(PathBuf, IoSink<ThreeMfSink>)>>,
    cancel: CancelToken,
    emit: &'a mut dyn FnMut(ExportEvent),
    total: u32,
    index: u32,
    written: Vec<PathBuf>,
    files: Vec<String>,
    parts: Vec<PartReport>,
}

/// File of bed `b`'s 3MF: `<base>.3mf` for the first bed, `<base>_bed<b+1>.3mf` after.
pub fn bed_file_name(base: &str, bed: u32) -> String {
    if bed == 0 { format!("{base}.3mf") } else { format!("{base}_bed{}.3mf", bed + 1) }
}

impl<'a> AppSink<'a> {
    fn new(
        dir: &Path,
        base: &str,
        formats: &[ExportFormat],
        meta: &Meta,
        cancel: &CancelToken,
        emit: &'a mut dyn FnMut(ExportEvent),
    ) -> Result<Self, String> {
        Ok(AppSink {
            dir: dir.to_path_buf(),
            base: base.to_owned(),
            stl: formats.contains(&ExportFormat::Stl),
            tmf: formats.contains(&ExportFormat::ThreeMf),
            meta: meta.clone(),
            beds: Vec::new(),
            cancel: cancel.clone(),
            emit,
            total: 0,
            index: 0,
            written: Vec::new(),
            files: Vec::new(),
            parts: Vec::new(),
        })
    }

    fn bed_sink(&mut self, bed: u32) -> ad3d_core::Result<&mut (PathBuf, IoSink<ThreeMfSink>)> {
        let b = bed as usize;
        if self.beds.len() <= b {
            self.beds.resize_with(b + 1, || None);
        }
        if self.beds[b].is_none() {
            let path = self.dir.join(bed_file_name(&self.base, bed));
            let mut meta = self.meta.clone();
            if bed > 0 {
                meta.title = meta.title.map(|t| format!("{t} (bed {})", bed + 1));
            }
            let sink = IoSink::new(ThreeMfSink::new(&path, &meta, None, None, self.cancel.clone())?);
            self.beds[b] = Some((path, sink));
        }
        Ok(self.beds[b].as_mut().expect("opened above"))
    }
}

impl PartSink for AppSink<'_> {
    fn stage(&mut self, stage: &str) {
        (self.emit)(ExportEvent::Progress { part: String::new(), index: 0, total_parts: 0, stage: stage.into(), fraction: 0.0 });
    }

    fn begin(&mut self, parts: usize) {
        self.total = parts as u32;
        (self.emit)(ExportEvent::Started { total_parts: self.total });
    }

    fn accept(&mut self, part: &Part, r: &CorePartReport) -> ad3d_core::Result<()> {
        let total = self.total.max(1);
        (self.emit)(ExportEvent::Progress {
            part: part.name.clone(),
            index: self.index,
            total_parts: total,
            stage: "writing".into(),
            fraction: self.index as f64 / total as f64,
        });
        let mut files = Vec::new();
        if self.stl {
            let path = self.dir.join(format!("{}.stl", part_file_stem(&self.base, &part.name)));
            write_stl_cancellable(&path, &part.mesh, &part.transform, &self.cancel)?;
            self.written.push(path.clone());
            files.push(path.display().to_string());
            (self.emit)(ExportEvent::PartWritten {
                part: part.name.clone(),
                path: path.display().to_string(),
                triangles: r.triangles as u32,
            });
        }
        if self.tmf {
            let stl = self.stl;
            let (path, sink) = self.bed_sink(part.bed)?;
            sink.accept(part, r)?;
            let shown = path.display().to_string();
            files.push(shown.clone());
            if !stl {
                (self.emit)(ExportEvent::PartWritten { part: part.name.clone(), path: shown, triangles: r.triangles as u32 });
            }
        }
        self.files.extend(files.iter().filter(|f| !f.ends_with(".3mf")).cloned());
        self.parts.push(PartReport {
            name: part.name.clone(),
            files,
            triangles: r.triangles as u32,
            size_mm: [0, 1, 2].map(|k| r.max_mm[k] - r.min_mm[k]),
            fits_bed: Some(r.fits_bed),
            // The core validates every part before handing it over (it fails the export otherwise).
            validated: true,
            bed: part.bed,
            max_downward_ledge_mm: r.max_downward_ledge_mm,
            height_ratio: r.height_ratio,
        });
        self.index += 1;
        Ok(())
    }

    fn finish(&mut self, report: &ExportReport) -> ad3d_core::Result<()> {
        for (path, sink) in self.beds.iter_mut().flatten() {
            sink.finish(report)?;
            self.files.push(path.display().to_string());
        }
        Ok(())
    }

    fn abort(&mut self) {
        for (_, sink) in self.beds.iter_mut().flatten() {
            sink.abort();
        }
        self.beds.clear();
        for p in self.written.drain(..) {
            let _ = std::fs::remove_file(p);
        }
        self.files.clear();
        self.parts.clear();
    }
}

pub fn encode_target(nx: u32, ny: u32, g: &[f32], sim: Option<&[f32]>) -> Vec<u8> {
    let mut out = Vec::with_capacity(16 + 4 * g.len() * if sim.is_some() { 2 } else { 1 });
    let flags = if sim.is_some() { TARGET_HAS_SIM } else { 0 };
    for v in [TARGET_MAGIC, nx, ny, flags] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for v in g.iter().chain(sim.unwrap_or(&[])) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
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

/// Aborts the plate generation in flight (the UI calls it when a drag starts during a slow
/// final plate); that `generate_squama` call then fails with "cancelled".
#[tauri::command]
pub fn cancel_generate(state: State<'_, Arc<SquamaState>>) -> bool {
    state.cancel_running()
}

#[tauri::command]
pub fn squama_job_info(job_id: u32, state: State<'_, Arc<SquamaState>>) -> Option<JobInfo> {
    state.job_info(job_id)
}

#[tauri::command]
pub async fn squama_target(params: SquamaParamsM1, state: State<'_, Arc<SquamaState>>) -> Result<Response, String> {
    let state: Arc<SquamaState> = state.inner().clone();
    // Latest wins: a request that is already superseded when the worker picks it up is
    // skipped instead of queueing behind the others on the blocking pool.
    let seq = state.newest_target.fetch_add(1, Ordering::SeqCst) + 1;
    blocking(move || {
        if state.newest_target.load(Ordering::SeqCst) != seq {
            return Err("cancelled".to_owned());
        }
        state.target(&params).map(Response::new)
    })
    .await
}

#[tauri::command]
pub async fn squama_tile_plan(export: SquamaExportDto, state: State<'_, Arc<SquamaState>>) -> Result<TilePlanDto, String> {
    let state: Arc<SquamaState> = state.inner().clone();
    blocking(move || state.tile_plan(&export, &CancelToken::new())).await
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
    use ad3d_core::squama::SYNTHETIC_PX;
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
    fn v1_and_m1_payloads_parse() {
        let d = dto();
        assert_eq!(d.job_id, 42);
        assert!(d.job().unwrap().is_synthetic());
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
        let job = d.job().unwrap();
        assert_eq!(job.quality, Quality::Final);
        assert_eq!(job.image_opts.rotate_deg, 90);
        let mut bad = v1_json();
        bad["layout"] = serde_json::json!({ "kind": "triangle" });
        assert!(serde_json::from_value::<SquamaParamsM1>(bad).is_err());
    }

    #[test]
    fn generate_marks_final_payloads_with_face_kinds_and_scores() {
        let st = SquamaState::default();
        let mut d = dto();
        d.width_mm = 120.0;
        let (buf, info) = st.generate(&d, &CancelToken::new()).unwrap();
        let flags = u32::from_le_bytes(buf[52..56].try_into().unwrap());
        assert_eq!(flags, payload::FLAG_FACE_KINDS);
        assert!(info.score.is_none());
        d.quality = Some(QualityDto::Final);
        d.layout = Some(ad3d_core::squama::job::LayoutDto::Hex {
            orientation: ad3d_core::squama::job::HexOrientationDto::Flat,
        });
        d.optimiser = Some(ad3d_core::squama::job::OptimiserDto::Shadow { max_sweeps: Some(1) });
        let (buf, info) = st.generate(&d, &CancelToken::new()).unwrap();
        let (mesh, job, ..) = payload::decode(&buf).unwrap();
        assert_eq!(job, 42);
        assert_eq!(mesh.face_kind.len(), mesh.triangle_count());
        assert_eq!(u32::from_le_bytes(buf[52..56].try_into().unwrap()), 3);
        assert_eq!(info.layout.kind, "hex");
        assert!(info.score.is_some() && info.optimiser_sweeps == Some(1));
        if cfg!(debug_assertions) {
            assert_eq!(info.validated, Some(true));
        }
        st.remember(info);
        assert!(st.job_info(42).is_some());
        // The target view now carries the simulated appearance.
        let t = st.target(&d).unwrap();
        assert_eq!(u32::from_le_bytes(t[12..16].try_into().unwrap()), TARGET_HAS_SIM);
    }

    #[test]
    fn crop_and_rotation_change_the_grid() {
        let st = SquamaState::default();
        let mut d = dto();
        d.width_mm = 60.0;
        let t = st.target(&d).unwrap();
        let (nx, ny) = (u32::from_le_bytes(t[4..8].try_into().unwrap()), u32::from_le_bytes(t[8..12].try_into().unwrap()));
        assert_eq!(nx, 10);
        d.image_opts = Some(ImageOpts { rotate_deg: 90, ..ImageOpts::default() });
        let t = st.target(&d).unwrap();
        let ny2 = u32::from_le_bytes(t[8..12].try_into().unwrap());
        assert!(ny2 < ny, "a rotated portrait target is landscape");
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
            orientation: "standing-auto".into(),
            bed_mm: Some([235.0, 235.0, 250.0]),
            margin_mm: Some(5.0),
            bed_motion: None,
            keys: Some(KeysDto::Pegs),
            stand: Some(true),
            calibration_tile: Some(true),
        }
    }

    #[test]
    fn export_writes_valid_parts_and_reports_progress() {
        let dir = tmp_dir("export");
        let st = SquamaState::default();
        let e = export_dto(&dir);
        let plan = st.tile_plan(&e, &CancelToken::new()).unwrap();
        assert_eq!((plan.cols.len(), plan.rows.len()), (2, 2));
        let mut events = Vec::new();
        let r = st.export(&e, &CancelToken::new(), &mut |ev| events.push(ev)).unwrap();
        // 2 × 2 tiles, key splints, stand(s), calibration tile.
        assert!(r.parts.len() >= 7, "{:?}", r.parts.iter().map(|p| &p.name).collect::<Vec<_>>());
        assert_eq!(r.tiles.as_ref().map(|t| (t.cols.len(), t.rows.len())), Some((2, 2)));
        // The overlay's plan is the plan the export cut (refined seams included).
        let t = r.tiles.as_ref().unwrap();
        assert_eq!((&plan.cols, &plan.rows), (&t.cols, &t.rows));
        assert!(r.parts.iter().all(|p| p.fits_bed == Some(true)));
        assert!(r.files.iter().any(|f| f.ends_with("plate_tile_c1_r1.stl")));
        assert!(r.files.iter().any(|f| f.ends_with("plate_calibration.stl")));
        assert!(r.files.iter().any(|f| f.ends_with("plate.3mf")));
        assert!(r.files.iter().any(|f| f.ends_with("plate_calibration.key.json")));
        let n = r.parts.len() as u32;
        assert!(events.iter().any(|e| matches!(e, ExportEvent::Started { total_parts } if *total_parts == n)));
        assert_eq!(events.iter().filter(|e| matches!(e, ExportEvent::PartWritten { .. })).count(), r.parts.len());
        for p in r.parts.iter().filter(|p| !p.name.contains("splints")) {
            let m = read_stl(Path::new(&p.files[0])).unwrap();
            let rep = validate(&m, &ValidateOpts { soup: true, ..Default::default() });
            assert!(rep.ok, "{}: {}", p.name, rep.summary());
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
        e.params.base_mm = 2.0;
        let err = st.export(&e, &CancelToken::new(), &mut |_| {}).unwrap_err();
        assert!(err.contains("keys"), "{err}");
        let mut e = export_dto(&dir);
        e.out_dir = dir.join("missing").display().to_string();
        assert!(st.export(&e, &CancelToken::new(), &mut |_| {}).is_err());
        let t = CancelToken::new();
        t.cancel();
        let err = st.export(&export_dto(&dir), &t, &mut |_| {}).unwrap_err();
        assert_eq!(err, "cancelled");
        assert!(std::fs::read_dir(&dir).unwrap().next().is_none(), "failed exports wrote files");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_and_reveal() {
        assert_eq!(sanitize_base("plate.3mf"), "plate");
        assert_eq!(sanitize_base("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_base(".hidden"), "hidden");
        assert_eq!(sanitize_base(""), "shadow_plate");
        assert_eq!(sanitize_base("a/b:c*?.stl"), "b_c__");
        assert_eq!(part_file_stem("p", "plate"), "p");
        assert_eq!(part_file_stem("p", "stand"), "p_stand");
        assert!(reveal_command(Path::new("/definitely/not/here")).is_err());
        assert!(reveal_command(&std::env::temp_dir()).is_ok());
    }
}
