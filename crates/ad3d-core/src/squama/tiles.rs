//! Tiling a plate to a printer bed (design §14.1, §9).
//!
//! A plate that does not fit the bed in its print orientation is cut into a grid
//! of rectangular tiles along lattice lines (cell boundaries for square cells, the
//! §6.2 centre lines for hex cells). Facets are solved once for the whole plate and
//! the partition is built with the cut lines, so a tile is just `mesh_relief` over
//! the subset of pieces inside its rectangle and the two sides of a seam carry the
//! same facet planes (no step across a seam).
//!
//! Orientation: `Standing(Auto)` tries the quarter turns whose light comes from
//! below or the lower side (§9) and keeps the one that needs the fewest tiles (ties
//! to the better light score). On a bed-slinger the plate's face is parallel to the
//! bed's motion axis ([`crate::orient::standing_xform`]).

use glam::DVec2;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::orient::{BedMotion, Orientation, QuarterTurns, Xform, auto_candidates, fixed_quarter_turns, light_score, standing_xform};
use crate::relief::Partition;
use crate::squama::Layout;

/// A printer's build volume (mm) and the clearance kept from its limits.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bed {
    pub x_mm: f64,
    pub y_mm: f64,
    pub z_mm: f64,
    /// Kept clear on every side in X and Y, and at the top in Z.
    pub margin_mm: f64,
    /// The axis the bed moves in (a bed-slinger moves it in Y).
    pub motion: BedMotion,
}

impl Bed {
    /// The owner's Creality CR-6 SE: 235 × 235 × 250 mm, bed moves in Y (START_HERE).
    pub const CR6_SE: Bed = Bed { x_mm: 235.0, y_mm: 235.0, z_mm: 250.0, margin_mm: 5.0, motion: BedMotion::Y };

    /// Usable size `[x − 2m, y − 2m, z − m]`.
    pub fn usable(&self) -> [f64; 3] {
        [self.x_mm - 2.0 * self.margin_mm, self.y_mm - 2.0 * self.margin_mm, self.z_mm - self.margin_mm]
    }

    pub fn check(&self) -> Result<()> {
        let u = self.usable();
        let finite = [self.x_mm, self.y_mm, self.z_mm, self.margin_mm].iter().all(|v| v.is_finite());
        if !finite || self.margin_mm < 0.0 || u.iter().any(|&v| v < 10.0) {
            return Err(Error::InvalidParams(format!(
                "bed {} × {} × {} mm with margin {} mm leaves less than 10 mm usable",
                self.x_mm, self.y_mm, self.z_mm, self.margin_mm
            )));
        }
        Ok(())
    }
}

impl Default for Bed {
    fn default() -> Self {
        Bed::CR6_SE
    }
}

/// An axis-aligned rectangle in plate space (mm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DRect {
    pub min: DVec2,
    pub max: DVec2,
}

impl DRect {
    pub fn size(&self) -> DVec2 {
        self.max - self.min
    }
    pub fn area(&self) -> f64 {
        let s = self.size();
        s.x * s.y
    }
    /// Inclusive containment with a small absolute tolerance.
    pub fn contains(&self, p: DVec2, eps: f64) -> bool {
        p.x >= self.min.x - eps && p.x <= self.max.x + eps && p.y >= self.min.y - eps && p.y <= self.max.y + eps
    }
}

/// Which family of lattice lines a cut runs along.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CutAxis {
    /// A vertical line `u = at_mm`.
    U,
    /// A horizontal line `v = at_mm`.
    V,
}

/// One straight seam through the whole plate, on lattice line `index`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TileCut {
    pub axis: CutAxis,
    /// Lattice index: the line lies at `index · pitch` (§6.2 units for hex).
    pub index: u32,
    pub at_mm: f64,
}

/// One tile: grid position `(col, row)` from the lower left, its rectangle, the
/// partition pieces inside it, and plate space → print space with the tile's
/// lower-left print corner at the origin (bed placement is added by the part layout).
#[derive(Debug, Clone, PartialEq)]
pub struct Tile {
    pub id: (u32, u32),
    pub rect_mm: DRect,
    pub pieces: Vec<u32>,
    pub transform: Xform,
}

/// Seam placement rule (PROJ-SQUAMA-TILE `seam_policy`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeamPolicy {
    /// Near-equal tile sizes (63 rows → 32 + 31).
    Balanced,
    /// Start balanced, then move each seam within ±25 % of a tile to the lattice
    /// line with the smallest height step, so seams hide in flat areas.
    #[default]
    SmallestStep,
}

/// Inputs to [`plan_tiles`] besides the layout and bed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileOpts {
    /// Plate thickness (back to highest facet point, plus any rib depth), mm.
    pub thickness_mm: f64,
    /// Smallest tile side allowed when the plate is cut in that direction (keys
    /// need room on both sides of each seam). 0 = no limit.
    pub min_tile_mm: f64,
    /// A user-forced grid `(cols, rows)` (clamped to the lattice). The tiles may
    /// then not fit the bed; the export warns.
    pub grid: Option<(u32, u32)>,
}

/// The result of [`plan_tiles`].
#[derive(Debug, Clone, PartialEq)]
pub struct TilePlan {
    pub cols: u32,
    pub rows: u32,
    /// Lattice pitch (mm) along u and v; cut `index` lies at `index · pitch`.
    pub pitch: DVec2,
    /// Tile boundaries in lattice units, `cols + 1` / `rows + 1` entries from 0.
    pub u_units: Vec<u32>,
    pub v_units: Vec<u32>,
    /// Most and fewest lattice units a tile may span along u and v.
    pub max_units: (u32, u32),
    pub min_units: (u32, u32),
    /// Tiles in row-major order from the bottom row.
    pub tiles: Vec<Tile>,
    /// The resolved orientation (`Auto` replaced by the chosen quarter turn).
    pub orientation: Orientation,
    pub quarter_turns: Option<u32>,
    pub motion: BedMotion,
    /// Plate size (mm).
    pub plate: DVec2,
}

impl TilePlan {
    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    /// The cut lines, for `build_partition(l, cuts)` (§14.1).
    pub fn cuts(&self) -> Vec<TileCut> {
        let mut c = Vec::new();
        for &i in &self.u_units[1..self.u_units.len() - 1] {
            c.push(TileCut { axis: CutAxis::U, index: i, at_mm: i as f64 * self.pitch.x });
        }
        for &j in &self.v_units[1..self.v_units.len() - 1] {
            c.push(TileCut { axis: CutAxis::V, index: j, at_mm: j as f64 * self.pitch.y });
        }
        c
    }

    /// Boundaries in mm along u (`cols + 1` values). The last is the plate width.
    pub fn u_edges(&self) -> Vec<f64> {
        edges_mm(&self.u_units, self.pitch.x, self.plate.x)
    }

    pub fn v_edges(&self) -> Vec<f64> {
        edges_mm(&self.v_units, self.pitch.y, self.plate.y)
    }

    /// Moves seams within the allowed range to the lattice line with the lowest
    /// `cost(axis, index)` (ties: closest to the balanced position), one seam at a
    /// time from the left/bottom, then rebuilds the tiles. Piece lists are cleared.
    pub fn refine_seams(&mut self, mut cost: impl FnMut(CutAxis, u32) -> f64) {
        for (axis, units, maxu, minu) in [
            (CutAxis::U, &mut self.u_units, self.max_units.0, self.min_units.0),
            (CutAxis::V, &mut self.v_units, self.max_units.1, self.min_units.1),
        ] {
            let n = units.len();
            for c in 1..n - 1 {
                let (prev, cur, next) = (units[c - 1], units[c], units[c + 1]);
                let span = (cur - prev).max(1);
                let slack = (span / 4).max(1);
                let lo = cur.saturating_sub(slack).max(prev + minu).max(next.saturating_sub(maxu));
                let hi = (cur + slack).min(next.saturating_sub(minu)).min(prev + maxu);
                if lo > hi {
                    continue;
                }
                let mut best = (f64::INFINITY, u32::MAX, cur);
                for i in lo..=hi {
                    let k = cost(axis, i);
                    let d = i.abs_diff(cur);
                    if k < best.0 - 1e-12 || ((k - best.0).abs() <= 1e-12 && d < best.1) {
                        best = (k, d, i);
                    }
                }
                units[c] = best.2;
            }
        }
        self.rebuild_tiles();
    }

    fn rebuild_tiles(&mut self) {
        let (ue, ve) = (self.u_edges(), self.v_edges());
        self.tiles.clear();
        for row in 0..self.rows as usize {
            for col in 0..self.cols as usize {
                let rect = DRect { min: DVec2::new(ue[col], ve[row]), max: DVec2::new(ue[col + 1], ve[row + 1]) };
                let transform = tile_xform(self.orientation, self.motion, &rect);
                self.tiles.push(Tile { id: (col as u32, row as u32), rect_mm: rect, pieces: Vec::new(), transform });
            }
        }
    }

    /// Assigns every piece of `p` to the tile that contains it. Fails if a piece
    /// crosses a cut (the partition must be built with [`TilePlan::cuts`]).
    pub fn assign_pieces(&mut self, p: &Partition) -> Result<()> {
        let (ue, ve) = (self.u_edges(), self.v_edges());
        for t in &mut self.tiles {
            t.pieces.clear();
        }
        let eps = 1e-6;
        for (pi, piece) in p.pieces.iter().enumerate() {
            let c = crate::relief::piece_apex(p, pi);
            let col = ue[1..].partition_point(|&e| e <= c.x).min(self.cols as usize - 1);
            let row = ve[1..].partition_point(|&e| e <= c.y).min(self.rows as usize - 1);
            let tile = &mut self.tiles[row * self.cols as usize + col];
            for s in piece.ring.start..piece.ring.end {
                let v = p.verts[p.rings[s as usize] as usize];
                if !tile.rect_mm.contains(v, eps) {
                    return Err(Error::Internal(format!(
                        "piece {pi} crosses a tile cut near ({:.3}, {:.3}); build the partition with the cuts",
                        v.x, v.y
                    )));
                }
            }
            tile.pieces.push(pi as u32);
        }
        if let Some(t) = self.tiles.iter().find(|t| t.pieces.is_empty()) {
            return Err(Error::Internal(format!("tile {:?} has no pieces", t.id)));
        }
        Ok(())
    }
}

fn edges_mm(units: &[u32], pitch: f64, total: f64) -> Vec<f64> {
    let last = units.len() - 1;
    units.iter().enumerate().map(|(k, &i)| if k == last { total } else { i as f64 * pitch }).collect()
}

/// Lattice pitch and line counts for a layout: cuts may only run along these lines.
pub fn lattice(l: &Layout) -> Result<(DVec2, u32, u32)> {
    match l.hex_lattice() {
        None => Ok((DVec2::splat(l.cell_mm), l.nx, l.ny)),
        // §6.2 centre lines: `cell/2` across the flats (m units), the row pitch
        // `√3/2·cell` along the rows (n units); flat-top swaps u and v. Cuts through
        // hexes split them into pieces of one facet (`build_partition(l, cuts)`).
        Some(lat) if !lat.flat => Ok((DVec2::new(lat.s_unit(), lat.pitch()), lat.m, lat.n)),
        Some(lat) => Ok((DVec2::new(lat.pitch(), lat.s_unit()), lat.n, lat.m)),
    }
}

/// Plate space → print space for one tile, its lower-left print corner at the origin.
pub fn tile_xform(o: Orientation, motion: BedMotion, rect: &DRect) -> Xform {
    let to_origin = Xform::translation([-rect.min.x, -rect.min.y, 0.0]);
    let s = rect.size();
    match o {
        Orientation::Flat => to_origin,
        _ => to_origin.then(&standing_xform(fixed_quarter_turns(o).unwrap_or(0), s.x, s.y, motion)),
    }
}

/// Largest in-plane extents (u, v) a tile may have in orientation `k` (None = flat),
/// or `None` when the plate's thickness does not fit.
fn max_extent(bed: &Bed, k: Option<u32>, thickness: f64) -> Option<(f64, f64)> {
    let [ux, uy, uz] = bed.usable();
    match k {
        None => (thickness <= uz).then_some((ux, uy)),
        Some(k) => {
            // Face along the motion axis; thickness along the other horizontal axis.
            let (along, across) = match bed.motion {
                BedMotion::Y => (uy, ux),
                BedMotion::X | BedMotion::None => (ux, uy),
            };
            if thickness > across {
                return None;
            }
            Some(if k % 2 == 0 { (along, uz) } else { (uz, along) })
        }
    }
}

/// Balanced split of `n` units into `parts` (larger parts first), as boundaries.
fn balanced(n: u32, parts: u32) -> Vec<u32> {
    let (base, extra) = (n / parts, n % parts);
    let mut v = Vec::with_capacity(parts as usize + 1);
    let mut acc = 0;
    v.push(0);
    for k in 0..parts {
        acc += base + u32::from(k < extra);
        v.push(acc);
    }
    v
}

/// §14.1: chooses the orientation (for `Auto`) and the tile grid.
///
/// Each tile spans whole lattice units along u and v, at most what fits the usable
/// bed in the print orientation. `Standing(Auto)` considers only the quarter turns
/// with `sin(φ + k·90°) ≤ 0` (there are always two), picks the one with the fewest
/// tiles and breaks ties by the lower score. Errors: the thickness does not fit, a
/// single lattice unit does not fit, or tiles would be narrower than `min_tile_mm`.
pub fn plan_tiles(l: &Layout, bed: &Bed, o: Orientation, light_azimuth: f64, opts: &TileOpts) -> Result<TilePlan> {
    bed.check()?;
    if !(opts.thickness_mm.is_finite() && opts.thickness_mm > 0.0) {
        return Err(Error::InvalidParams(format!("plate thickness {} mm", opts.thickness_mm)));
    }
    let (pitch, nu, nv) = lattice(l)?;
    let candidates: Vec<Option<u32>> = match o {
        Orientation::Flat => vec![None],
        Orientation::Standing(QuarterTurns::Auto) => auto_candidates(light_azimuth)
            .into_iter()
            .filter(|&k| light_score(light_azimuth, k) <= 1e-9)
            .map(Some)
            .collect(),
        _ => vec![fixed_quarter_turns(o)],
    };
    let min_u = if opts.min_tile_mm > 0.0 { (opts.min_tile_mm / pitch.x - 1e-9).ceil() as u32 } else { 1 }.max(1);
    let min_v = if opts.min_tile_mm > 0.0 { (opts.min_tile_mm / pitch.y - 1e-9).ceil() as u32 } else { 1 }.max(1);
    let mut best: Option<(u32, u32, u32, u32, Option<u32>)> = None; // (tiles, cols, rows, mu·mv packed later)
    let mut best_max = (0, 0);
    let mut why = String::new();
    for k in candidates {
        let Some((eu, ev)) = max_extent(bed, k, opts.thickness_mm) else {
            why = format!("plate thickness {:.2} mm does not fit the bed", opts.thickness_mm);
            continue;
        };
        let mu = (eu / pitch.x + 1e-9).floor() as u32;
        let mv = (ev / pitch.y + 1e-9).floor() as u32;
        if mu == 0 || mv == 0 {
            why = "one cell does not fit the usable bed".into();
            continue;
        }
        let (cols, rows) = match opts.grid {
            Some((c, r)) => (c.clamp(1, nu), r.clamp(1, nv)),
            None => (nu.div_ceil(mu), nv.div_ceil(mv)),
        };
        if opts.grid.is_none() && ((cols > 1 && nu / cols < min_u) || (rows > 1 && nv / rows < min_v)) {
            why = format!("tiles would be narrower than {:.1} mm", opts.min_tile_mm);
            continue;
        }
        // A forced grid ranks orientations by how well its tiles fit.
        let n = match opts.grid {
            None => cols * rows,
            Some(_) => u32::from(nu.div_ceil(cols) > mu) + u32::from(nv.div_ceil(rows) > mv),
        };
        if best.is_none_or(|b| n < b.0) {
            best = Some((n, cols, rows, 0, k));
            best_max = (mu.max(nu.div_ceil(cols)), mv.max(nv.div_ceil(rows)));
        }
    }
    let Some((_, cols, rows, _, k)) = best else {
        return Err(Error::InvalidParams(format!("cannot tile the plate for this bed: {why}")));
    };
    let orientation = match k {
        None => Orientation::Flat,
        Some(k) => Orientation::Standing([QuarterTurns::Q0, QuarterTurns::Q1, QuarterTurns::Q2, QuarterTurns::Q3][k as usize]),
    };
    let mut plan = TilePlan {
        cols,
        rows,
        pitch,
        u_units: balanced(nu, cols),
        v_units: balanced(nv, rows),
        max_units: best_max,
        min_units: (min_u, min_v),
        tiles: Vec::new(),
        orientation,
        quarter_turns: k,
        motion: bed.motion,
        plate: DVec2::new(l.w_mm, l.h_mm),
    };
    plan.rebuild_tiles();
    Ok(plan)
}

/// Seam cost for [`TilePlan::refine_seams`]: the summed height step (µm) across
/// the lattice line, from the partition and clustered corner heights. Edges of the
/// partition that lie on the line contribute `max(|d0|, |d1|)`.
pub fn seam_step_cost(p: &Partition, h: &crate::relief::Heights, pitch: DVec2) -> impl Fn(CutAxis, u32) -> f64 + use<> {
    use std::collections::HashMap;
    let mut edge: HashMap<(u32, u32), (usize, usize)> = HashMap::with_capacity(p.rings.len());
    for piece in &p.pieces {
        let (s0, s1) = (piece.ring.start as usize, piece.ring.end as usize);
        for s in s0..s1 {
            let ns = if s + 1 == s1 { s0 } else { s + 1 };
            edge.insert((p.rings[s], p.rings[ns]), (s, ns));
        }
    }
    // Per line: total step. Lines are keyed by (axis, rounded lattice index).
    let mut lines: HashMap<(bool, i64), f64> = HashMap::new();
    for (&(a, b), &(sa, sb)) in &edge {
        if a > b {
            continue;
        }
        let Some(&(tb, ta)) = edge.get(&(b, a)) else { continue };
        let (pa, pb) = (p.verts[a as usize], p.verts[b as usize]);
        let step = (h.corner[sa] - h.corner[ta]).abs().max((h.corner[sb] - h.corner[tb]).abs()) as f64;
        if (pa.x - pb.x).abs() < 1e-9 {
            let i = (pa.x / pitch.x).round();
            if (i * pitch.x - pa.x).abs() < 1e-6 {
                *lines.entry((true, i as i64)).or_default() += step;
            }
        }
        if (pa.y - pb.y).abs() < 1e-9 {
            let j = (pa.y / pitch.y).round();
            if (j * pitch.y - pa.y).abs() < 1e-6 {
                *lines.entry((false, j as i64)).or_default() += step;
            }
        }
    }
    move |axis, i| lines.get(&(axis == CutAxis::U, i as i64)).copied().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::squama::LayoutKind;
    use std::f64::consts::PI;

    fn square(nx: u32, ny: u32, cell: f64) -> Layout {
        Layout {
            kind: LayoutKind::Square,
            w_mm: nx as f64 * cell,
            h_mm: ny as f64 * cell,
            cell_mm: cell,
            nx,
            ny,
            crop: None,
        }
    }

    const OPTS: TileOpts = TileOpts { thickness_mm: 5.0, min_tile_mm: 0.0, grid: None };

    #[test]
    fn balanced_63_rows() {
        assert_eq!(balanced(63, 2), vec![0, 32, 63]);
        assert_eq!(balanced(10, 3), vec![0, 4, 7, 10]);
    }

    #[test]
    fn luban_default_plate_on_cr6se() {
        // 300 × 378 mm, light from the left: Auto picks k = 1 (light from below),
        // so v' = u: the 300 mm width stands up (≤ 245 Z? no) → tiles needed.
        let l = square(50, 63, 6.0);
        let plan = plan_tiles(&l, &Bed::CR6_SE, Orientation::Standing(QuarterTurns::Auto), PI, &OPTS).unwrap();
        // Usable: along Y 225, Z 245. k = 1: u (300) along Z (245 → 40 cells), v (378) along Y (225 → 37 cells).
        // k = 3 has sin = +1 and is not considered.
        assert_eq!(plan.quarter_turns, Some(1));
        assert_eq!((plan.cols, plan.rows), (2, 2));
        assert_eq!(plan.u_units, vec![0, 25, 50]);
        assert_eq!(plan.v_units, vec![0, 32, 63]);
        for t in &plan.tiles {
            let (lo, hi) = t.transform.apply_box([t.rect_mm.min.x, t.rect_mm.min.y, 0.0], [t.rect_mm.max.x, t.rect_mm.max.y, 5.0]);
            assert!(lo.iter().all(|&v| v.abs() < 1e-9), "{lo:?}");
            let u = Bed::CR6_SE.usable();
            assert!(hi[0] <= u[0] && hi[1] <= u[1] && hi[2] <= u[2], "{hi:?}");
            // Face parallel to Y: the plate normal maps to X.
            assert_eq!(t.transform.m[0][2], 1.0);
        }
    }

    #[test]
    fn small_plate_is_one_tile() {
        let l = square(30, 20, 6.0);
        let plan = plan_tiles(&l, &Bed::CR6_SE, Orientation::Standing(QuarterTurns::Auto), 1.5 * PI, &OPTS).unwrap();
        assert_eq!(plan.tile_count(), 1);
        assert_eq!(plan.quarter_turns, Some(0));
        assert!(plan.cuts().is_empty());
    }

    #[test]
    fn auto_prefers_fewer_tiles_among_light_below_candidates() {
        // Side light φ = 0° (from the right): sin(k·90°) ≤ 0 for k = 0 (0) and k = 2 (0)
        // and k = 3 (−1). k = 3 is best by score; with a plate 240 wide × 100 tall,
        // k = 3 puts the 240 mm width along Z (245 ok) and 100 along Y: one tile.
        let l = square(40, 16, 6.0);
        let plan = plan_tiles(&l, &Bed::CR6_SE, Orientation::Standing(QuarterTurns::Auto), 0.0, &OPTS).unwrap();
        assert_eq!(plan.tile_count(), 1);
        assert_eq!(plan.quarter_turns, Some(3));
    }

    #[test]
    fn flat_and_thickness() {
        let l = square(50, 63, 6.0);
        let plan = plan_tiles(&l, &Bed::CR6_SE, Orientation::Flat, PI, &OPTS).unwrap();
        assert_eq!((plan.cols, plan.rows), (2, 2));
        let thick = TileOpts { thickness_mm: 400.0, min_tile_mm: 0.0, grid: None };
        assert!(plan_tiles(&l, &Bed::CR6_SE, Orientation::Flat, PI, &thick).is_err());
    }

    #[test]
    fn min_tile_refuses_slivers() {
        // 2 cells wide on a tiny bed: tiles of one 6 mm cell are refused at 20 mm.
        let bed = Bed { x_mm: 20.0, y_mm: 200.0, z_mm: 200.0, margin_mm: 1.0, motion: BedMotion::None };
        let l = square(4, 4, 6.0);
        let o = Orientation::Standing(QuarterTurns::Q0);
        assert!(plan_tiles(&l, &bed, o, PI, &OPTS).is_ok());
        assert!(plan_tiles(&l, &bed, o, PI, &TileOpts { thickness_mm: 5.0, min_tile_mm: 20.0, grid: None }).is_err());
    }

    #[test]
    fn refine_moves_seam_to_cheap_line_within_limits() {
        let l = square(50, 10, 6.0);
        let bed = Bed { x_mm: 180.0, y_mm: 200.0, z_mm: 200.0, margin_mm: 0.0, motion: BedMotion::None };
        let mut plan = plan_tiles(&l, &bed, Orientation::Standing(QuarterTurns::Q0), 1.5 * PI, &OPTS).unwrap();
        assert_eq!(plan.u_units, vec![0, 25, 50]);
        plan.refine_seams(|axis, i| if axis == CutAxis::U && i == 21 { 0.0 } else { 1.0 });
        assert_eq!(plan.u_units, vec![0, 21, 50]);
        // A line outside ±25 % or making a tile too wide (30 cells max) is ignored.
        plan.refine_seams(|axis, i| if axis == CutAxis::U && i == 10 { 0.0 } else { 1.0 });
        assert!(plan.u_units[1] >= 20);
        assert!(plan.u_units[2] - plan.u_units[1] <= plan.max_units.0);
    }
}
