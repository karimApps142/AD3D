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
    KeySpec, LayoutItem, Part, PartKind, StandSpec, engrave_regions, key_layout, key_regions, layout_on_beds, layout_on_beds_lenient, mesh_box,
    splints_mesh, stand_mesh,
};
use crate::squama::calibration::{CalibrationKey, CalibrationSpec, calibration_tile};
use crate::squama::job::SquamaJob;
use crate::squama::layout::TileCut as LayoutCut;
use crate::squama::pipeline::{dedup_warnings, job_layout, prepare_target, solve_job};
use crate::squama::solve::Quality;
use crate::squama::tiles::{Bed, CutAxis, DRect, SeamPolicy, TileOpts, plan_tiles, seam_step_cost, tile_xform};
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
    /// Largest downward-facing ledge (standing tiles only).
    pub max_downward_ledge_mm: Option<f64>,
}

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
/// angle, lit like the plate centre.
pub fn calibration_spec(job: &SquamaJob, l: &Layout) -> Result<CalibrationSpec> {
    let mut spec = CalibrationSpec::from_params(&job.params);
    spec.cell_mm = l.cell_mm;
    let c = job.light.reference(l.w_mm, l.h_mm, job.limits.base_mm)?;
    spec.light_azimuth = c.a.y.atan2(c.a.x);
    spec.light_elevation = c.el;
    Ok(spec)
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

    // Facets once for the whole plate (§14.1), at final quality (optimiser when on).
    let plain = build_partition(l, &[])?;
    let sj = solve_job(&job, l, &plain, g, &e.gen_opts, cancel)?;
    warnings.extend(sj.solved.warnings.iter().cloned());
    let facets = sj.facets;
    let heights0 = quantise(&plain, &facets, pc.base_mm, &tol)?;
    cancel.check()?;
    let thick = |h: &crate::relief::Heights| {
        let max_corner = h.corner.iter().copied().max().unwrap_or(0) as f64 * tol.q;
        let max_apex = h.apex_w.iter().copied().fold(0.0f64, f64::max);
        max_corner.max(max_apex)
    };
    let thickness0 = thick(&heights0);

    let want_keys = e.keys == KeyKind::Pegs;
    if want_keys {
        e.key_spec.check()?;
    }
    let topts = TileOpts {
        thickness_mm: thickness0,
        min_tile_mm: if want_keys { e.key_spec.min_tile_mm() } else { 0.0 },
        grid: match e.tiles {
            TileRequest::Auto => None,
            TileRequest::None => Some((1, 1)),
            TileRequest::Grid(c, r) => Some((c, r)),
        },
    };
    let mut plan = plan_tiles(l, &e.bed, e.orientation, pc.light_azimuth, &topts)?;
    let is_hex = l.hex_lattice().is_some();
    // Square seams follow cell edges and can hide in flat areas; a hex seam runs
    // through cells that keep one plane, so it never steps.
    if e.seam_policy == SeamPolicy::SmallestStep && plan.tile_count() > 1 && !is_hex {
        let cost = seam_step_cost(&plain, &heights0, plan.pitch);
        plan.refine_seams(cost);
    }
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
    let keys = if want_keys && plan.tile_count() > 1 {
        if min_top < e.key_spec.min_base_mm() - 1e-9 {
            return Err(Error::InvalidParams(format!(
                "keys need a base of at least {:.1} mm (pocket floor {} mm below the facets); raise the base or turn keys off",
                e.key_spec.min_base_mm(),
                crate::relief::POCKET_CLEARANCE_MM
            )));
        }
        Some(key_layout(&plan, &e.key_spec))
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
    if e.stand {
        let extra = if n_keys > 0 { e.key_spec.splint_t_mm } else { 0.0 };
        let s = StandSpec::for_plate(l.w_mm, l.h_mm, thickness, extra, &e.bed);
        let m = stand_mesh(&s)?;
        let (min, max) = mesh_box(&m);
        for k in 0..s.count {
            let item = LayoutItem { base: crate::orient::Xform::IDENTITY, min, max, may_rotate: true };
            let name = if s.count == 1 { "stand".to_string() } else { format!("stand_{}", k + 1) };
            pending.push((name, PartKind::Stand { index: k }, Pending::Ready(m.clone()), item, None, None, 1));
        }
    }
    let mut calibration_key = None;
    if e.calibration_tile {
        let spec = calibration_spec(&job, l)?;
        let ct = calibration_tile(&spec, &e.gen_opts, cancel)?;
        warnings.extend(ct.warnings.iter().cloned());
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
    for ((name, kind, pend, _item, min_w, back_area, comps), pl) in pending.into_iter().zip(&placements) {
        cancel.check()?;
        let (mesh, ledge) = match pend {
            Pending::Ready(m) => (m, None),
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
        };
        sink.accept(&part_out, &rep)?;
        reports.push(rep);
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

/// The tile plan an export would use (balanced seams, estimated thickness), for
/// the UI's tile overlay. The export itself plans again from the solved plate.
pub fn preview_tile_plan(job: &SquamaJob, l: &Layout, e: &SquamaExport) -> Result<crate::squama::tiles::TilePlan> {
    let want_keys = e.keys == KeyKind::Pegs;
    let topts = TileOpts {
        thickness_mm: estimate_thickness(job, l),
        min_tile_mm: if want_keys { e.key_spec.min_tile_mm() } else { 0.0 },
        grid: match e.tiles {
            TileRequest::Auto => None,
            TileRequest::None => Some((1, 1)),
            TileRequest::Grid(c, r) => Some((c, r)),
        },
    };
    plan_tiles(l, &e.bed, e.orientation, job.params.light_azimuth, &topts)
}
