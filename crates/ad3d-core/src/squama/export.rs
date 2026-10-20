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
use crate::image::{CellBrightness, LinearLuma, cell_brightness};
use crate::mesh::validate::validate;
use crate::orient::{BedMotion, Orientation, QuarterTurns, down_in_plate, max_downward_ledge};
use crate::relief::{BackRegion, PieceSet, mesh_relief, quantise};
use crate::squama::parts::{
    KeySpec, LayoutItem, Part, PartKind, StandSpec, engrave_regions, key_layout, key_regions, layout_on_beds, mesh_box,
    splints_mesh, stand_mesh,
};
use crate::squama::tiles::{Bed, SeamPolicy, TileOpts, plan_tiles, seam_step_cost};
use crate::squama::{GenOptions, Layout, LayoutKind, SquamaParams, SquamaParamsDto, build_partition, check_params, layout, solve_facets};

/// Alignment key style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyKind {
    None,
    /// Blind holes in the back plus splints with pegs (§14.2).
    #[default]
    Pegs,
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
}

/// Receives validated parts one at a time. A sink must not keep a part's mesh
/// unless it needs it; `finish` runs after the last part, `abort` on any error.
pub trait PartSink {
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
// MERGE: wire a 3MF sink to the io 3MF writer (§14.4: one object per part, build
// items carrying `Part::transform`) once it lands; `formats` already accepts "3mf".

/// §12 entry point: image → validated parts streamed to `sink`.
pub fn squama_export(
    p: &SquamaParams,
    img: &LinearLuma,
    e: &SquamaExport,
    cancel: &CancelToken,
    sink: &mut dyn PartSink,
) -> Result<ExportReport> {
    // MERGE: wire to squama::prepare_target (§16 image opts, hex coverage) when it lands.
    let l = layout(p, (img.w, img.h))?;
    let cb = cell_brightness(img, l.nx, l.ny, l.crop)?;
    squama_export_cb(p, &cb, e, cancel, sink)
}

/// [`squama_export`] from per-cell brightness (pure; used by tests and the CLI).
pub fn squama_export_cb(
    p: &SquamaParams,
    cb: &CellBrightness,
    e: &SquamaExport,
    cancel: &CancelToken,
    sink: &mut dyn PartSink,
) -> Result<ExportReport> {
    match export_inner(p, cb, e, cancel, sink) {
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

fn export_inner(
    p: &SquamaParams,
    cb: &CellBrightness,
    e: &SquamaExport,
    cancel: &CancelToken,
    sink: &mut dyn PartSink,
) -> Result<ExportReport> {
    let tol = e.gen_opts.tol;
    let (pc, mut warnings) = check_params(p, &e.gen_opts)?;
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
    if e.calibration_tile {
        // MERGE: wire to squama::calibration_tile (§15.3) and add it as PartKind::Calibration.
        warnings.push(Warning { code: "no_calibration_tile", message: "calibration tile is not available in this build".into() });
    }

    // Facets once for the whole plate (§14.1). MERGE: wire to squama::solve_facets
    // with the optimiser at Quality::Final and to build_partition(l, &plan.cuts())
    // for hex; square-grid cuts lie on cell lines, so the plain grid already has them.
    let facets = solve_facets(cb, &l, &pc, &tol);
    let part = build_partition(&l, &[])?;
    let heights = quantise(&part, &facets, pc.base_mm, &tol)?;
    cancel.check()?;
    let max_corner = heights.corner.iter().copied().max().unwrap_or(0) as f64 * tol.q;
    let max_apex = heights.apex_w.iter().copied().fold(0.0f64, f64::max);
    let thickness = max_corner.max(max_apex);
    let min_top = heights.corner.iter().copied().min().unwrap_or(0) as f64 * tol.q;

    let want_keys = e.keys == KeyKind::Pegs;
    if want_keys {
        e.key_spec.check()?;
    }
    let topts = TileOpts { thickness_mm: thickness, min_tile_mm: if want_keys { e.key_spec.min_tile_mm() } else { 0.0 } };
    let mut plan = plan_tiles(&l, &e.bed, e.orientation, pc.light_azimuth, &topts)?;
    if e.seam_policy == SeamPolicy::SmallestStep && plan.tile_count() > 1 {
        let cost = seam_step_cost(&part, &heights, plan.pitch);
        plan.refine_seams(cost);
    }
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
    let items: Vec<LayoutItem> = pending.iter().map(|p| p.3).collect();
    let placements = layout_on_beds(&items, &e.bed)?;

    let down = plan.quarter_turns.map(down_in_plate);
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
    };
    sink.finish(&report)?;
    Ok(report)
}
