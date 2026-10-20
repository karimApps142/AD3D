//! `squama_export` (design §9, §14): plate → tiles, keys, stand → validated parts
//! streamed to a [`PartSink`].
//!
//! Facets are solved and quantised once for the whole plate (§14.1), the tile plan
//! picks the orientation and grid, every part's bed placement is computed from its
//! predicted bounds, then each part is meshed, validated on the exact `f32` values
//! that will be written (§10 stage 2, plus the self-intersection stage by default)
//! and handed to the sink one at a time, so large plates never sit in memory whole.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cancel::CancelToken;
use crate::error::{Error, Result, Warning};
use crate::image::{CellBrightness, LinearLuma};
use crate::mesh::validate::validate;
use crate::orient::{BedMotion, Orientation, QuarterTurns, down_in_plate, max_downward_ledge};
use crate::relief::{BackRegion, PieceSet, mesh_relief, quantise};
use crate::squama::parts::{
    KeySpec, LayoutItem, Part, PartKind, STAND_KEY_CLEARANCE_MM, StandSpec, engrave_regions, key_layout, key_regions, layout_on_beds, layout_on_beds_lenient, mesh_box,
    splints_mesh, stand_mesh,
};
use crate::squama::calibration::{CalibrationKey, CalibrationSpec, calibration_tile};
use crate::squama::job::SquamaJob;
use crate::squama::layout::TileCut as LayoutCut;
use crate::squama::pipeline::{dedup_warnings, job_layout, prepare_target, solve_job};
use crate::squama::solve::Quality;
use crate::squama::tiles::{Bed, CutAxis, DRect, MAX_STANDING_RATIO, SeamPolicy, TileOpts, plan_tiles, seam_step_cost, tile_xform};
use crate::squama::{GenOptions, Layout, LayoutKind, SquamaParams, SquamaParamsDto, build_partition, check_params};
use glam::DVec2;

/// Alignment key style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyKind {
    None,
    /// Blind holes in the back plus splints with pegs (§14.2).
    #[default]
    Pegs,
    /// Pegs whenever the plate is tiled and the base and tiles leave room for
    /// them; otherwise none (with a warning when tiled).
    Auto,
}

/// Tile grid request: automatic (fewest tiles that fit the bed), none (one part),
/// or a fixed `cols × rows` grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TileRequest {
    #[default]
    Auto,
    None,
    Grid(u32, u32),
}

/// Export options (Rust side of the §12.1 `SquamaExport` DTO).
#[derive(Debug, Clone, PartialEq)]
pub struct SquamaExport {
    pub orientation: Orientation,
    pub bed: Bed,
    pub keys: KeyKind,
    pub key_spec: KeySpec,
    pub stand: bool,
    pub calibration_tile: bool,
    /// Engrave the tile id and an UP arrow in the back (§14.2).
    pub engrave: bool,
    pub seam_policy: SeamPolicy,
    /// How the plate is split into tiles.
    pub tiles: TileRequest,
    /// Run the triangle-triangle self-intersection stage on every part.
    pub check_self_intersection: bool,
    pub gen_opts: GenOptions,
}

impl Default for SquamaExport {
    fn default() -> Self {
        SquamaExport {
            orientation: Orientation::Standing(QuarterTurns::Auto),
            bed: Bed::CR6_SE,
            keys: KeyKind::Pegs,
            key_spec: KeySpec::default(),
            stand: true,
            calibration_tile: false,
            engrave: true,
            seam_policy: SeamPolicy::SmallestStep,
            tiles: TileRequest::Auto,
            check_self_intersection: true,
            gen_opts: GenOptions::default(),
        }
    }
}

/// The JS → Rust export DTO (§12.1, camelCase). `bedMotion` and `seamPolicy` are
/// additive optional fields (default: bed moves in Y, smallest-step seams).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SquamaExportDto {
    pub params: SquamaParamsDto,
    pub out_dir: String,
    #[serde(default = "default_formats")]
    pub formats: Vec<String>,
    #[serde(default = "default_orientation")]
    pub orientation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bed_mm: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub margin_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bed_motion: Option<BedMotion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keys: Option<KeyKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stand: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibration_tile: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seam_policy: Option<SeamPolicy>,
}

fn default_formats() -> Vec<String> {
    vec!["stl".into()]
}
fn default_orientation() -> String {
    "standing-auto".into()
}

/// Parses an orientation string of the DTO.
pub fn parse_orientation(s: &str) -> Result<Orientation> {
    Ok(match s {
        "standing-auto" => Orientation::Standing(QuarterTurns::Auto),
        "standing-q0" => Orientation::Standing(QuarterTurns::Q0),
        "standing-q1" => Orientation::Standing(QuarterTurns::Q1),
        "standing-q2" => Orientation::Standing(QuarterTurns::Q2),
        "standing-q3" => Orientation::Standing(QuarterTurns::Q3),
        "flat" => Orientation::Flat,
        _ => return Err(Error::InvalidParams(format!("unknown orientation {s:?}"))),
    })
}

impl SquamaExportDto {
    /// Converts to generator params and export options (degrees → radians).
    pub fn export(&self) -> Result<(SquamaParams, SquamaExport)> {
        let mut e = SquamaExport { orientation: parse_orientation(&self.orientation)?, ..Default::default() };
        if let Some([x, y, z]) = self.bed_mm {
            e.bed.x_mm = x;
            e.bed.y_mm = y;
            e.bed.z_mm = z;
        }
        if let Some(m) = self.margin_mm {
            e.bed.margin_mm = m;
        }
        if let Some(m) = self.bed_motion {
            e.bed.motion = m;
        }
        if let Some(k) = self.keys {
            e.keys = k;
        }
        if let Some(s) = self.stand {
            e.stand = s;
        }
        if let Some(c) = self.calibration_tile {
            e.calibration_tile = c;
        }
        if let Some(s) = self.seam_policy {
            e.seam_policy = s;
        }
        for f in &self.formats {
            if f != "stl" && f != "3mf" {
                return Err(Error::InvalidParams(format!("unknown export format {f:?}")));
            }
        }
        Ok((self.params.params(), e))
    }
}

/// What happened to one part.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartReport {
    pub name: String,
    pub kind: PartKind,
    pub bed: u32,
    pub vertices: usize,
    pub triangles: usize,
    /// Print-space bounds (mm).
    pub min_mm: [f64; 3],
    pub max_mm: [f64; 3],
    /// Smallest distance between distinct written vertices; `None` when no pair
    /// is closer than twice the δ gate.
    pub min_distinct_distance_mm: Option<f64>,
    pub self_intersections: Option<usize>,
    /// Largest downward-facing ledge (standing tiles and the calibration tile).
    pub max_downward_ledge_mm: Option<f64>,
    /// Whether the part fits the usable bed (its placement).
    pub fits_bed: bool,
    /// Print-space footprint (X × Y) and height (Z), mm.
    pub footprint_mm: [f64; 2],
    pub height_mm: f64,
    /// `height / min(footprint)`: above [`BRIM_RATIO`] the part needs a brim.
    pub height_ratio: f64,
}

/// Height-to-footprint ratio above which a part should be printed with a brim.
pub const BRIM_RATIO: f64 = 10.0;

/// The brim advice for tall, thin standing parts (FDM, 0.4 mm nozzle).
pub const BRIM_ADVICE: &str =
    "brim 6–8 mm, outer only, brim-object gap 0.1 mm; 3–4 perimeters or 100 % infill; 0.12–0.16 mm layers; elephant-foot compensation 0.2 mm";

/// What the export did.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReport {
    pub plate_mm: [f64; 2],
    pub cols: u32,
    pub rows: u32,
    pub orientation: String,
    pub quarter_turns: Option<u32>,
    pub bed_motion: BedMotion,
    pub beds_used: u32,
    pub max_thickness_mm: f64,
    pub max_downward_ledge_mm: f64,
    pub keys: u32,
    pub parts: Vec<PartReport>,
    pub warnings: Vec<Warning>,
    /// Set when a calibration tile was exported: write it next to the parts as
    /// `<stem>.key.json` so a photo of the print can be read (§15.3).
    pub calibration_key: Option<CalibrationKey>,
    /// Tile boundaries in plate mm (`cols + 1` / `rows + 1` values).
    pub u_edges_mm: Vec<f64>,
    pub v_edges_mm: Vec<f64>,
    /// Set when the shadow-aware optimiser ran.
    pub optimise: Option<ExportOptimise>,
    /// Print advice when a part is tall and thin (height / footprint > [`BRIM_RATIO`]).
    pub brim: Option<String>,
}

/// What the optimiser achieved for an export.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOptimise {
    pub sweeps: u32,
    pub before: crate::sim::MatchScore,
    pub after: crate::sim::MatchScore,
    pub timed_out: bool,
}

/// Receives validated parts one at a time. A sink must not keep a part's mesh
/// unless it needs it; `finish` runs after the last part, `abort` on any error.
pub trait PartSink {
    /// Progress before any part exists (solving, optimising, planning).
    fn stage(&mut self, _stage: &str) {}
    /// Called once, before the first part, with the number of parts to come.
    fn begin(&mut self, _parts: usize) {}
    fn accept(&mut self, part: &Part, report: &PartReport) -> Result<()>;
    fn finish(&mut self, _report: &ExportReport) -> Result<()> {
        Ok(())
    }
    fn abort(&mut self) {}
}

/// Keeps every part in memory (tests, previews).
#[derive(Debug, Default)]
pub struct CollectSink {
    pub parts: Vec<(Part, PartReport)>,
    pub finished: bool,
}

impl PartSink for CollectSink {
    fn accept(&mut self, part: &Part, report: &PartReport) -> Result<()> {
        self.parts.push((part.clone(), report.clone()));
        Ok(())
    }
    fn finish(&mut self, _report: &ExportReport) -> Result<()> {
        self.finished = true;
        Ok(())
    }
    fn abort(&mut self) {
        self.parts.clear();
    }
}

/// Streams each part as `<dir>/<prefix><name>.stl` (binary, transformed to print
/// space). Files are written as `.stl.partial` and renamed on `finish`, and
/// removed on `abort`, so a failed or cancelled export leaves no output.
#[derive(Debug)]
pub struct StlDirSink {
    pub dir: PathBuf,
    pub prefix: String,
    partial: Vec<(PathBuf, PathBuf)>,
    /// Final paths, filled by `finish`.
    pub files: Vec<PathBuf>,
}

impl StlDirSink {
    pub fn new(dir: impl AsRef<Path>, prefix: &str) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir).map_err(|e| Error::Io(format!("{}: {e}", dir.display())))?;
        Ok(StlDirSink { dir, prefix: prefix.to_string(), partial: Vec::new(), files: Vec::new() })
    }
}

impl PartSink for StlDirSink {
    fn accept(&mut self, part: &Part, _report: &PartReport) -> Result<()> {
        let fin = self.dir.join(format!("{}{}.stl", self.prefix, part.name));
        let tmp = self.dir.join(format!("{}{}.stl.partial", self.prefix, part.name));
        crate::io::stl::write_stl(&tmp, &part.mesh, &part.transform)?;
        self.partial.push((tmp, fin));
        Ok(())
    }
    fn finish(&mut self, _report: &ExportReport) -> Result<()> {
        for (tmp, fin) in self.partial.drain(..) {
            std::fs::rename(&tmp, &fin).map_err(|e| Error::Io(format!("{}: {e}", fin.display())))?;
            self.files.push(fin);
        }
        Ok(())
    }
    fn abort(&mut self) {
        for (tmp, _) in self.partial.drain(..) {
            let _ = std::fs::remove_file(tmp);
        }
    }
}
/// Adapts an [`crate::io::sink::PartSink`] (3MF, or the io crate's STL sink) to
/// this module's [`PartSink`]: parts are forwarded as [`crate::io::sink::ExportPart`]s
/// and the io sink is finished with the export. Dropping an unfinished 3MF sink
/// removes its partial file.
pub struct IoSink<S: crate::io::sink::PartSink> {
    pub inner: Option<S>,
    /// Files the io sink reported on `finish`.
    pub written: Vec<crate::io::sink::WrittenFile>,
}

impl<S: crate::io::sink::PartSink> IoSink<S> {
    pub fn new(inner: S) -> Self {
        IoSink { inner: Some(inner), written: Vec::new() }
    }
}

/// The io sink's file-level kind of a part.
pub fn io_part_kind(k: &PartKind) -> crate::io::sink::PartKind {
    use crate::io::sink::PartKind as K;
    match k {
        PartKind::Tile { .. } => K::Tile,
        PartKind::KeySplints { .. } => K::Splint,
        PartKind::Stand { .. } => K::Stand,
        PartKind::Calibration => K::Calibration,
    }
}

impl<S: crate::io::sink::PartSink> PartSink for IoSink<S> {
    fn accept(&mut self, part: &Part, _report: &PartReport) -> Result<()> {
        let inner = self.inner.as_mut().ok_or_else(|| Error::Internal("sink already finished".into()))?;
        // Moves the mesh only by reference: the io sinks stream it out immediately.
        inner.put(&crate::io::sink::ExportPart {
            name: part.name.clone(),
            kind: io_part_kind(&part.kind),
            mesh: part.mesh.clone(),
            transform: part.transform,
        })
    }
    fn finish(&mut self, _report: &ExportReport) -> Result<()> {
        if let Some(mut s) = self.inner.take() {
            self.written = s.finish()?;
        }
        Ok(())
    }
    fn abort(&mut self) {
        self.inner = None;
    }
}

/// Several sinks at once (STL files and one 3MF from one run).
#[derive(Default)]
pub struct MultiSink(pub Vec<Box<dyn PartSink>>);

impl PartSink for MultiSink {
    fn stage(&mut self, stage: &str) {
        for s in &mut self.0 {
            s.stage(stage);
        }
    }
    fn begin(&mut self, parts: usize) {
        for s in &mut self.0 {
            s.begin(parts);
        }
    }
    fn accept(&mut self, part: &Part, report: &PartReport) -> Result<()> {
        for s in &mut self.0 {
            s.accept(part, report)?;
        }
        Ok(())
    }
    fn finish(&mut self, report: &ExportReport) -> Result<()> {
        for s in &mut self.0 {
            s.finish(report)?;
        }
        Ok(())
    }
    fn abort(&mut self) {
        for s in &mut self.0 {
            s.abort();
        }
    }
}

/// §12 entry point (v1 parameters): image → validated parts streamed to `sink`.
/// Square cells, directional light, Lambert; see [`squama_export_job`] for M1 jobs.
pub fn squama_export(
    p: &SquamaParams,
    img: &LinearLuma,
    e: &SquamaExport,
    cancel: &CancelToken,
    sink: &mut dyn PartSink,
) -> Result<ExportReport> {
    squama_export_job(&SquamaJob::from_params(p), Some(img), e, cancel, sink)
}

/// §12 `squama_export` for an M1 job: layout, §16 target, then [`export_plate`].
/// `img` is the decoded source (`None` = the synthetic target).
pub fn squama_export_job(
    job: &SquamaJob,
    img: Option<&LinearLuma>,
    e: &SquamaExport,
    cancel: &CancelToken,
    sink: &mut dyn PartSink,
) -> Result<ExportReport> {
    let mut run = || {
        let synth;
        let img = match img {
            Some(i) => i,
            None => {
                synth = super::pipeline::synthetic_image();
                &synth
            }
        };
        let (l, warnings) = job_layout(job, (img.w, img.h), &e.gen_opts)?;
        let part = build_partition(&l, &[])?;
        let t = prepare_target(img, &job.image_opts, &l, &part)?;
        cancel.check()?;
        export_inner(job, &l, &t.g, warnings, e, cancel, sink)
    };
    match run() {
        Ok(r) => Ok(r),
        Err(err) => {
            sink.abort();
            Err(err)
        }
    }
}

/// [`squama_export`] from per-cell brightness (pure; used by tests). v1
/// parameters, square cells, `cb.nx` must equal the snapped width.
pub fn squama_export_cb(
    p: &SquamaParams,
    cb: &CellBrightness,
    e: &SquamaExport,
    cancel: &CancelToken,
    sink: &mut dyn PartSink,
) -> Result<ExportReport> {
    let mut run = || {
        let (pc, warnings) = check_params(p, &e.gen_opts)?;
        let cell_um = (pc.cell_mm * 1000.0).round() as i64;
        let cell = cell_um as f64 / 1000.0;
        let nx = ((((pc.width_mm * 1000.0).round() as i64) + cell_um / 2) / cell_um).max(1) as u32;
        if cb.nx != nx || cb.ny == 0 || cb.g.len() != (cb.nx * cb.ny) as usize {
            return Err(Error::InvalidParams(format!(
                "brightness grid {}×{} does not match the snapped width ({nx} cells)",
                cb.nx, cb.ny
            )));
        }
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
        export_inner(&SquamaJob::from_params(p), &l, &cb.g, warnings, e, cancel, sink)
    };
    match run() {
        Ok(r) => Ok(r),
        Err(err) => {
            sink.abort();
            Err(err)
        }
    }
}

/// Export for a job whose layout and per-facet target `g` are known (the app
/// computes them with its cached image pipeline). Calls `sink.abort()` on error.
pub fn export_plate(
    job: &SquamaJob,
    l: &Layout,
    g: &[f32],
    warnings: Vec<Warning>,
    e: &SquamaExport,
    cancel: &CancelToken,
    sink: &mut dyn PartSink,
) -> Result<ExportReport> {
    match export_inner(job, l, g, warnings, e, cancel, sink) {
        Ok(r) => Ok(r),
        Err(err) => {
            sink.abort();
            Err(err)
        }
    }
}

/// One part before meshing: tiles are meshed lazily, small parts are ready.
enum Pending {
    Tile { index: usize, regions: Vec<BackRegion> },
    Ready(crate::mesh::Mesh),
}

/// The calibration tile's spec for a job: the plate's cell, base and surface
/// angle, lit like the plate centre. A point lamp more than 45° above the plate
/// centre is clamped to 45° for the tile (the tile's directional range), with a
/// warning.
pub fn calibration_spec(job: &SquamaJob, l: &Layout) -> Result<(CalibrationSpec, Vec<Warning>)> {
    let mut spec = CalibrationSpec::from_params(&job.params);
    spec.cell_mm = l.cell_mm;
    let c = job.light.reference(l.w_mm, l.h_mm, job.limits.base_mm)?;
    spec.light_azimuth = c.a.y.atan2(c.a.x);
    let max_el = crate::squama::light::MAX_DIRECTIONAL_ELEVATION_DEG.to_radians();
    let mut warnings = Vec::new();
    spec.light_elevation = if c.el > max_el {
        warnings.push(Warning {
            code: "calibration_elevation_clamped",
            message: format!(
                "the lamp is {:.0}° above the plate centre; the calibration tile is designed for 45° (photograph it under the plate's lamp anyway)",
                c.el.to_degrees()
            ),
        });
        max_el
    } else {
        c.el
    };
    Ok((spec, warnings))
}

/// Light azimuth (radians) for planning the print orientation: the directional
/// light's, or a point lamp's direction as seen from the plate centre (a point
/// light overrides the v1 `lightAzimuthDeg`).
pub fn planning_azimuth(job: &SquamaJob, l: &Layout) -> Result<f64> {
    match job.light {
        crate::squama::light::Light::Directional { az, .. } => Ok(az),
        crate::squama::light::Light::Point { .. } => {
            let c = job.light.reference(l.w_mm, l.h_mm, job.limits.base_mm)?;
            Ok(c.a.y.atan2(c.a.x))
        }
    }
}

fn tile_opts(e: &SquamaExport, thickness_mm: f64) -> TileOpts {
    // Forced pegs keep tiles wide enough for keys; `Auto` only adds them when they fit.
    let want_keys = e.keys == KeyKind::Pegs;
    TileOpts {
        thickness_mm,
        min_tile_mm: if want_keys { e.key_spec.min_tile_mm() } else { 0.0 },
        grid: match e.tiles {
            TileRequest::Auto => None,
            TileRequest::None => Some((1, 1)),
            TileRequest::Grid(c, r) => Some((c, r)),
        },
        max_standing_ratio: Some(MAX_STANDING_RATIO),
    }
}

/// The planning stage of an export: facets for the whole plate (final quality,
/// optimiser when on), their quantised heights, and the tile plan with refined
/// seams. Shared by the export and [`preview_tile_plan`], so the overlay shows the
/// seams the export cuts.
struct Planned {
    sj: crate::squama::pipeline::SolvedJob,
    plain: crate::relief::Partition,
    heights0: crate::relief::Heights,
    thickness0: f64,
    plan: crate::squama::tiles::TilePlan,
}

fn heights_thickness(h: &crate::relief::Heights, tol: &crate::relief::Tol) -> f64 {
    let max_corner = h.corner.iter().copied().max().unwrap_or(0) as f64 * tol.q;
    let max_apex = h.apex_w.iter().copied().fold(0.0f64, f64::max);
    max_corner.max(max_apex)
}

fn plan_export(job: &SquamaJob, l: &Layout, g: &[f32], e: &SquamaExport, cancel: &CancelToken) -> Result<Planned> {
    let tol = e.gen_opts.tol;
    let job = job.with_quality(Quality::Final);
    let pc = check_params(&job.params, &e.gen_opts)?.0;
    let plain = build_partition(l, &[])?;
    let sj = solve_job(&job, l, &plain, g, &e.gen_opts, cancel)?;
    let heights0 = quantise(&plain, &sj.facets, pc.base_mm, &tol)?;
    cancel.check()?;
    let thickness0 = heights_thickness(&heights0, &tol);
    if e.keys != KeyKind::None {
        e.key_spec.check()?;
    }
    let mut plan = plan_tiles(l, &e.bed, e.orientation, planning_azimuth(&job, l)?, &tile_opts(e, thickness0))?;
    // Square seams follow cell edges and can hide in flat areas; a hex seam runs
    // through cells that keep one plane, so it never steps.
    if e.seam_policy == SeamPolicy::SmallestStep && plan.tile_count() > 1 && l.hex_lattice().is_none() {
        let cost = seam_step_cost(&plain, &heights0, plan.pitch);
        plan.refine_seams(cost);
    }
    Ok(Planned { sj, plain, heights0, thickness0, plan })
}

#[allow(clippy::too_many_arguments)]
fn export_inner(
    job: &SquamaJob,
    l: &Layout,
    g: &[f32],
    mut warnings: Vec<Warning>,
    e: &SquamaExport,
    cancel: &CancelToken,
    sink: &mut dyn PartSink,
) -> Result<ExportReport> {
    let tol = e.gen_opts.tol;
    let job = job.with_quality(Quality::Final);
    let pc = check_params(&job.params, &e.gen_opts)?.0;

    // Facets once for the whole plate (§14.1), at final quality (optimiser when on),
    // then the tile plan with refined seams.
    sink.stage(if job.optimiser.is_some_and(|o| o.max_sweeps > 0) { "solving and optimising facets" } else { "solving facets" });
    let Planned { sj, plain, heights0, thickness0, mut plan } = plan_export(&job, l, g, e, cancel)?;
    sink.stage("planning tiles and keys");
    warnings.extend(sj.solved.warnings.iter().cloned());
    let facets = sj.facets;
    let thick = |h: &crate::relief::Heights| heights_thickness(h, &tol);
    let mut want_keys = e.keys == KeyKind::Pegs;
    let is_hex = l.hex_lattice().is_some();
    // The partition with the cut lines built in (§14.1); square cuts lie on cell
    // lines, so the plain grid already contains them.
    let (part, heights) = if is_hex && plan.tile_count() > 1 {
        let cuts: Vec<LayoutCut> = plan
            .cuts()
            .iter()
            .map(|c| match c.axis {
                CutAxis::U => LayoutCut::U(c.at_mm),
                CutAxis::V => LayoutCut::V(c.at_mm),
            })
            .collect();
        let p = build_partition(l, &cuts)?;
        let h = quantise(&p, &facets, pc.base_mm, &tol)?;
        (p, h)
    } else {
        (plain, heights0)
    };
    let thickness = thick(&heights).max(thickness0);
    let min_top = heights.corner.iter().copied().min().unwrap_or(0) as f64 * tol.q;
    plan.assign_pieces(&part)?;
    cancel.check()?;

    // Keys (§14.2).
    if e.keys == KeyKind::Auto && plan.tile_count() > 1 {
        let min_tile = e.key_spec.min_tile_mm();
        let narrow = plan.tiles.iter().any(|t| {
            let sz = t.rect_mm.size();
            (plan.cols > 1 && sz.x < min_tile - 1e-9) || (plan.rows > 1 && sz.y < min_tile - 1e-9)
        });
        if min_top < e.key_spec.min_base_mm() - 1e-9 {
            warnings.push(Warning {
                code: "no_keys",
                message: format!("no alignment keys: they need a base of at least {:.1} mm", e.key_spec.min_base_mm()),
            });
        } else if narrow {
            warnings.push(Warning { code: "no_keys", message: format!("no alignment keys: tiles narrower than {min_tile:.1} mm") });
        } else {
            want_keys = true;
        }
    }
    let stand_spec = e.stand.then(|| StandSpec::for_plate(l.w_mm, l.h_mm, thickness, e.key_spec.splint_t_mm, &e.bed));
    let keys = if want_keys && plan.tile_count() > 1 {
        if min_top < e.key_spec.min_base_mm() - 1e-9 {
            return Err(Error::InvalidParams(format!(
                "keys need a base of at least {:.1} mm ({:.1} mm of plate in front of each {:.1} mm pocket); raise the base or turn keys off",
                e.key_spec.min_base_mm(),
                crate::squama::parts::KEY_FRONT_WALL_MM,
                e.key_spec.hole_depth_mm
            )));
        }
        let mut ks = e.key_spec;
        if let Some(s) = &stand_spec {
            // Splints stay above the stand's slot.
            ks.bottom_clear_mm = s.slot_depth_mm + 0.5 * ks.splint_w_mm + STAND_KEY_CLEARANCE_MM;
        }
        Some(key_layout(&plan, &ks))
    } else {
        None
    };
    let n_keys = keys.as_ref().map_or(0, |k| k.pairs.len()) as u32;

    // Pending parts and their layout items.
    let mut pending: Vec<(String, PartKind, Pending, LayoutItem, Option<f64>, Option<f64>, usize)> = Vec::new();
    let engrave_ok = e.engrave && min_top >= crate::squama::parts::ENGRAVE_DEPTH_MM + crate::relief::POCKET_CLEARANCE_MM - 1e-9;
    if e.engrave && !engrave_ok {
        warnings.push(Warning { code: "no_engraving", message: "base too thin to engrave the tile id and UP arrow".into() });
    }
    let mut engrave_dropped = 0;
    for (i, t) in plan.tiles.iter().enumerate() {
        let mut regions = keys.as_ref().map(|k| key_regions(&k.holes[i], &e.key_spec)).unwrap_or_default();
        if engrave_ok {
            let label = if plan.tile_count() > 1 && plan.cols <= 9 && plan.rows <= 9 {
                format!("{}{}", t.id.0 + 1, t.id.1 + 1)
            } else {
                String::new()
            };
            let g = engrave_regions(t, &label, &regions);
            if g.is_empty() || (!label.is_empty() && g.len() == 1) {
                engrave_dropped += 1;
            }
            regions.extend(g);
        }
        let name = if plan.tile_count() == 1 { "plate".to_string() } else { format!("tile_c{}_r{}", t.id.0 + 1, t.id.1 + 1) };
        let item = LayoutItem {
            base: t.transform,
            min: [t.rect_mm.min.x, t.rect_mm.min.y, 0.0],
            max: [t.rect_mm.max.x, t.rect_mm.max.y, thickness],
            may_rotate: plan.quarter_turns.is_none(),
        };
        pending.push((
            name,
            PartKind::Tile { col: t.id.0, row: t.id.1 },
            Pending::Tile { index: i, regions },
            item,
            Some(pc.base_mm - tol.q / 2.0),
            Some(t.rect_mm.area()),
            1,
        ));
    }
    if engrave_dropped > 0 {
        warnings.push(Warning {
            code: "engraving_dropped",
            message: format!("{engrave_dropped} tile(s) too small for the full tile id / UP arrow engraving"),
        });
    }
    if n_keys > 0 {
        let m = splints_mesh(n_keys, &e.key_spec);
        let (min, max) = mesh_box(&m);
        let item = LayoutItem { base: crate::orient::Xform::IDENTITY, min, max, may_rotate: true };
        pending.push(("key_splints".into(), PartKind::KeySplints { count: n_keys }, Pending::Ready(m), item, None, None, n_keys as usize));
    }
    if let Some(s) = stand_spec {
        let m = stand_mesh(&s)?;
        let (min, max) = mesh_box(&m);
        for k in 0..s.count {
            let item = LayoutItem { base: crate::orient::Xform::IDENTITY, min, max, may_rotate: true };
            let name = if s.count == 1 { "stand".to_string() } else { format!("stand_{}", k + 1) };
            pending.push((name, PartKind::Stand { index: k }, Pending::Ready(m.clone()), item, None, None, 1));
        }
    }
    let mut calibration_key = None;
    let mut calibration_ledge = None;
    if e.calibration_tile {
        let (spec, w) = calibration_spec(&job, l)?;
        warnings.extend(w);
        let ct = calibration_tile(&spec, &e.gen_opts, cancel)?;
        warnings.extend(ct.warnings.iter().cloned());
        if let Some(k) = plan.quarter_turns {
            let all: Vec<u32> = (0..ct.partition.pieces.len() as u32).collect();
            calibration_ledge = Some(max_downward_ledge(&ct.partition, &ct.heights, &all, down_in_plate(k), &tol));
        }
        let (min, max) = mesh_box(&ct.mesh);
        let rect = DRect { min: DVec2::new(min[0], min[1]), max: DVec2::new(max[0], max[1]) };
        let base = tile_xform(plan.orientation, plan.motion, &rect);
        let item = LayoutItem { base, min, max, may_rotate: plan.quarter_turns.is_none() };
        let (mf, ba) = (ct.min_facet_w, ct.back_area);
        calibration_key = Some(ct.key);
        pending.push(("calibration".into(), PartKind::Calibration, Pending::Ready(ct.mesh), item, Some(mf), Some(ba), 1));
    }
    let items: Vec<LayoutItem> = pending.iter().map(|p| p.3).collect();
    let placements =
        if e.tiles == TileRequest::Auto { layout_on_beds(&items, &e.bed)? } else { layout_on_beds_lenient(&items, &e.bed)? };
    for ((name, ..), pl) in pending.iter().zip(&placements) {
        if !pl.fits {
            warnings.push(Warning {
                code: "part_exceeds_bed",
                message: format!("{name} is larger than the bed; use automatic tiling or a finer grid"),
            });
        }
    }

    let down = plan.quarter_turns.map(down_in_plate);
    sink.begin(pending.len());
    let mut reports = Vec::with_capacity(pending.len());
    let mut max_ledge = 0.0f64;
    let mut tall_parts = 0usize;
    for ((name, kind, pend, _item, min_w, back_area, comps), pl) in pending.into_iter().zip(&placements) {
        cancel.check()?;
        let (mesh, ledge) = match pend {
            Pending::Ready(m) => {
                let ledge = if kind == PartKind::Calibration { calibration_ledge } else { None };
                (m, ledge)
            }
            Pending::Tile { index, regions } => {
                let t = &plan.tiles[index];
                let m = mesh_relief(&part, PieceSet::Subset(&t.pieces), &heights, &regions, &tol, cancel)?;
                let ledge = down.map(|d| max_downward_ledge(&part, &heights, &t.pieces, d, &tol));
                (m, ledge)
            }
        };
        if let Some(lg) = ledge {
            max_ledge = max_ledge.max(lg);
        }
        let part_out = Part { name, kind, mesh, transform: pl.transform, bed: pl.bed, components: comps, min_facet_w: min_w, back_area };
        let r = validate(&part_out.mesh, &part_out.validate_opts(e.check_self_intersection));
        if !r.ok {
            return Err(Error::Internal(format!("part {} failed validate():\n{}", part_out.name, r.summary())));
        }
        let (lo, hi) = mesh_box(&part_out.mesh);
        let (min_mm, max_mm) = part_out.transform.apply_box(lo, hi);
        let size = [0, 1, 2].map(|k| max_mm[k] - min_mm[k]);
        let height_ratio = size[2] / size[0].min(size[1]).max(1e-9);
        if height_ratio > BRIM_RATIO {
            tall_parts += 1;
        }
        let rep = PartReport {
            name: part_out.name.clone(),
            kind: part_out.kind.clone(),
            bed: part_out.bed,
            vertices: part_out.mesh.vertex_count(),
            triangles: part_out.mesh.triangle_count(),
            min_mm,
            max_mm,
            min_distinct_distance_mm: r.min_distinct_distance,
            self_intersections: r.self_intersecting_pairs,
            max_downward_ledge_mm: ledge,
            fits_bed: pl.fits,
            footprint_mm: [size[0], size[1]],
            height_mm: size[2],
            height_ratio,
        };
        sink.accept(&part_out, &rep)?;
        reports.push(rep);
    }
    if e.tiles != TileRequest::Auto && plan.quarter_turns.is_some() {
        let tallest = reports.iter().filter(|p| matches!(p.kind, PartKind::Tile { .. })).map(|p| p.height_mm / thickness.max(1e-9)).fold(0.0, f64::max);
        if tallest > MAX_STANDING_RATIO {
            warnings.push(Warning {
                code: "tall_standing_tile",
                message: format!(
                    "standing tiles are {:.0}× as tall as the plate is thick (automatic tiling keeps this under {MAX_STANDING_RATIO:.0}×); they may wobble or detach",
                    tallest
                ),
            });
        }
    }
    let brim = (tall_parts > 0).then(|| BRIM_ADVICE.to_string());
    if let Some(b) = &brim {
        warnings.push(Warning {
            code: "brim_recommended",
            message: format!("{tall_parts} part(s) stand more than {BRIM_RATIO:.0}× taller than their footprint: {b}"),
        });
    }
    if max_ledge > 0.6 {
        warnings.push(Warning {
            code: "downward_ledge",
            message: format!("largest downward-facing ledge {max_ledge:.2} mm (above 0.6 mm may sag)"),
        });
    }
    dedup_warnings(&mut warnings);
    let orientation = match plan.orientation {
        Orientation::Flat => "flat".to_string(),
        _ => format!("standing-q{}", plan.quarter_turns.unwrap_or(0)),
    };
    let report = ExportReport {
        plate_mm: [l.w_mm, l.h_mm],
        cols: plan.cols,
        rows: plan.rows,
        orientation,
        quarter_turns: plan.quarter_turns,
        bed_motion: plan.motion,
        beds_used: placements.iter().map(|p| p.bed + 1).max().unwrap_or(0),
        max_thickness_mm: thickness,
        max_downward_ledge_mm: max_ledge,
        keys: n_keys,
        parts: reports,
        warnings,
        calibration_key,
        u_edges_mm: plan.u_edges(),
        v_edges_mm: plan.v_edges(),
        optimise: sj.optimise.map(|o| ExportOptimise { sweeps: o.sweeps, before: o.before, after: o.after, timed_out: o.timed_out }),
        brim,
    };
    sink.finish(&report)?;
    Ok(report)
}

/// Upper bound of the plate thickness before solving: `T` plus the largest
/// centre-pivot relief `2·r·tan S` (`r` the cell's circumradius), capped by
/// `maxReliefMm`. Good enough to plan tiles for a preview overlay.
pub fn estimate_thickness(job: &SquamaJob, l: &Layout) -> f64 {
    let r = l.corner_offsets().iter().map(|o| o.length()).fold(0.0, f64::max);
    let relief = 2.0 * r * job.limits.max_tilt.tan() + 2.0 * e_tol_q();
    job.limits.base_mm + job.limits.max_relief_mm.map_or(relief, |m| m.min(relief))
}

fn e_tol_q() -> f64 {
    crate::relief::Tol::default().q
}

/// The tile plan an export would use, for the UI's tile overlay, and the plate
/// thickness it was planned with. With the per-facet target `g` it runs the
/// export's own planning stage (final solve, optimiser when on, refined seams), so
/// the seams and tile sizes are exactly the export's; without it, balanced seams
/// on an estimated thickness.
pub fn preview_tile_plan(
    job: &SquamaJob,
    l: &Layout,
    g: Option<&[f32]>,
    e: &SquamaExport,
    cancel: &CancelToken,
) -> Result<(crate::squama::tiles::TilePlan, f64)> {
    match g {
        Some(g) => {
            let p = plan_export(job, l, g, e, cancel)?;
            Ok((p.plan, p.thickness0))
        }
        None => {
            let t = estimate_thickness(job, l);
            Ok((plan_tiles(l, &e.bed, e.orientation, planning_azimuth(job, l)?, &tile_opts(e, t))?, t))
        }
    }
}
