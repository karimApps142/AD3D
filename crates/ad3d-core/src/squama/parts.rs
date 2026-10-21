//! Export parts besides the tiles themselves (design §14.2, §14.4): alignment keys
//! (blind holes in the back plus printable splints with pegs), the engraved tile id
//! and UP arrow, the stand, and the layout of every part on the bed.
//!
//! Every builder here constructs its mesh directly from indices (no booleans), so
//! each part is a closed, consistently wound 2-manifold by construction and passes
//! `validate()` on its own.

use std::f64::consts::{PI, TAU};

use glam::DVec2;
use serde::Serialize;

use crate::error::{Error, Result};
use crate::mesh::validate::ValidateOpts;
use crate::mesh::{FaceKind, Mesh};
use crate::orient::Xform;
use crate::relief::{BACK_REGION_GAP_MM, BackRegion, POCKET_CLEARANCE_MM, triangulate_outline, triangulate_with_holes};
use crate::squama::tiles::{Bed, CutAxis, Tile, TilePlan};

/// What a part is.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PartKind {
    /// One tile of the plate (or the whole plate when it fits).
    Tile { col: u32, row: u32 },
    /// All key splints in one multi-shell part, printed flat.
    KeySplints { count: u32 },
    /// One stand foot, printed flat.
    Stand { index: u32 },
    /// The calibration tile (§15.3).
    Calibration,
}

/// One exported part: its mesh in its own space (plate space for tiles), the
/// transform to print space (orientation plus bed placement), and what `validate()`
/// should expect of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub name: String,
    pub kind: PartKind,
    pub mesh: Mesh,
    /// Part space → print space. Proper rotation plus translation.
    pub transform: Xform,
    /// Which bed (print job) the layout put it on, from 0.
    pub bed: u32,
    /// Closed shells in the mesh.
    pub components: usize,
    /// Lowest allowed facet vertex height (tiles).
    pub min_facet_w: Option<f64>,
    /// Expected back-face area (tiles).
    pub back_area: Option<f64>,
}

impl Part {
    /// `validate()` options for this part on its written (transformed) data.
    pub fn validate_opts(&self, check_self_intersection: bool) -> ValidateOpts {
        ValidateOpts {
            transform: self.transform,
            min_facet_w: self.min_facet_w,
            back_area: self.back_area,
            expect_components: self.components,
            check_self_intersection,
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

/// Alignment keys (§14.2): blind holes in the back on both sides of each seam and
/// splints, each a flat bar with two pegs, that bridge the seam on the back.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct KeySpec {
    /// Hole diameter (the polygon is circumscribed, so a round peg of this
    /// diameter fits) and depth into the back.
    pub hole_d_mm: f64,
    pub hole_depth_mm: f64,
    /// Hole centre distance from the seam.
    pub offset_mm: f64,
    /// Largest spacing between keys along a seam.
    pub max_spacing_mm: f64,
    /// No key centre closer than this to a tile corner along the seam.
    pub end_margin_mm: f64,
    /// No key centre on a vertical seam closer than this to the plate's bottom
    /// edge (`v = 0`), so splints stay out of the stand's slot. 0 = no limit.
    pub bottom_clear_mm: f64,
    /// Peg diameter (inscribed polygon) and height above the bar.
    pub peg_d_mm: f64,
    pub peg_h_mm: f64,
    /// Splint bar thickness and width.
    pub splint_t_mm: f64,
    pub splint_w_mm: f64,
    /// Polygon segments per circle.
    pub segments: u32,
}

impl Default for KeySpec {
    fn default() -> Self {
        KeySpec {
            // 0.5 mm diametral clearance: the holes print with their axis
            // horizontal (standing tiles), so their tops droop; the pegs print flat.
            hole_d_mm: 3.5,
            hole_depth_mm: 2.0,
            offset_mm: 6.0,
            max_spacing_mm: 40.0,
            end_margin_mm: 10.0,
            bottom_clear_mm: 0.0,
            peg_d_mm: 3.0,
            peg_h_mm: 1.7,
            splint_t_mm: 1.6,
            splint_w_mm: 6.0,
            segments: 24,
        }
    }
}

impl KeySpec {
    /// Smallest base thickness for keys: the pocket plus a [`KEY_FRONT_WALL_MM`]
    /// wall in front of it, so the pocket never shows through the shadow face
    /// (3.0 mm with the defaults).
    pub fn min_base_mm(&self) -> f64 {
        self.hole_depth_mm + KEY_FRONT_WALL_MM.max(POCKET_CLEARANCE_MM)
    }

    /// Smallest tile side across a seam: holes from the seams on both sides plus
    /// the gap rules.
    pub fn min_tile_mm(&self) -> f64 {
        (2.0 * (self.offset_mm + self.hole_d_mm) + BACK_REGION_GAP_MM + 2.0).max(2.0 * self.crossing_margin_mm())
    }

    /// Splint bar length: both pegs at `offset` from the seam plus their rims.
    pub fn splint_len_mm(&self) -> f64 {
        2.0 * self.offset_mm + self.peg_d_mm + 3.0
    }

    /// Smallest distance from a key centre to a seam crossing along its seam, so
    /// its splint clears the splints of the crossing seam (half of each bar plus a
    /// 0.5 mm gap).
    pub fn crossing_margin_mm(&self) -> f64 {
        (0.5 * self.splint_len_mm() + 0.5 * self.splint_w_mm + 0.5).max(self.end_margin_mm)
    }

    pub fn check(&self) -> Result<()> {
        let ok = self.hole_d_mm > self.peg_d_mm
            && self.peg_d_mm >= 1.0
            && self.hole_depth_mm > self.peg_h_mm
            && self.peg_h_mm > 0.2
            && self.offset_mm >= self.hole_d_mm / 2.0 + BACK_REGION_GAP_MM + 0.5
            && self.max_spacing_mm >= self.hole_d_mm + 2.0
            && self.end_margin_mm >= self.offset_mm
            && self.splint_t_mm >= 0.6
            && self.splint_w_mm >= self.peg_d_mm + 1.0
            && (8..=128).contains(&self.segments);
        if ok { Ok(()) } else { Err(Error::InvalidParams(format!("inconsistent key spec {self:?}"))) }
    }
}

/// Material kept in front of a key pocket (two to three 0.4 mm perimeters).
pub const KEY_FRONT_WALL_MM: f64 = 1.0;

/// Key positions for a tile plan.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct KeyLayout {
    /// Hole centres per tile (plate space), parallel to `TilePlan::tiles`.
    pub holes: Vec<Vec<DVec2>>,
    /// One entry per splint: the two hole centres it joins.
    pub pairs: Vec<(DVec2, DVec2)>,
}

/// Places keys along every seam segment between two neighbouring tiles: centres
/// at least `end_margin` from the segment's ends and at most `max_spacing` apart,
/// holes `offset` either side of the seam.
pub fn key_layout(plan: &TilePlan, ks: &KeySpec) -> KeyLayout {
    let mut out = KeyLayout { holes: vec![Vec::new(); plan.tiles.len()], pairs: Vec::new() };
    let (cols, rows) = (plan.cols as usize, plan.rows as usize);
    let idx = |c: usize, r: usize| r * cols + c;
    // Where a seam segment ends at another seam, its end key keeps the crossing
    // margin, so the two seams' splints never overlap on the back.
    let cross = ks.crossing_margin_mm();
    let margin = |at_crossing: bool| if at_crossing { cross } else { ks.end_margin_mm };
    let positions = |a: f64, b: f64, ma: f64, mb: f64| -> Vec<f64> {
        let (lo, hi) = (a + ma, b - mb);
        if hi <= lo {
            return vec![0.5 * (a + b)];
        }
        let n = ((hi - lo) / ks.max_spacing_mm).ceil() as usize;
        (0..=n).map(|k| lo + (hi - lo) * k as f64 / n.max(1) as f64).collect::<Vec<_>>()
    };
    for r in 0..rows {
        for c in 0..cols {
            let t = &plan.tiles[idx(c, r)];
            // Seam to the right (vertical line u = max.x).
            if c + 1 < cols {
                let x = t.rect_mm.max.x;
                // Keep splints out of the stand's slot along the plate's bottom edge.
                let lo = if ks.bottom_clear_mm > 0.0 && t.rect_mm.min.y <= 1e-9 {
                    (ks.bottom_clear_mm - ks.end_margin_mm).max(t.rect_mm.min.y)
                } else {
                    t.rect_mm.min.y
                };
                let lo = if lo + 2.0 * ks.end_margin_mm < t.rect_mm.max.y { lo } else { t.rect_mm.min.y };
                let mut ps = positions(lo, t.rect_mm.max.y, margin(r > 0), margin(r + 1 < rows));
                ps.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
                for y in ps {
                    let (l, rr) = (DVec2::new(x - ks.offset_mm, y), DVec2::new(x + ks.offset_mm, y));
                    out.holes[idx(c, r)].push(l);
                    out.holes[idx(c + 1, r)].push(rr);
                    out.pairs.push((l, rr));
                }
            }
            // Seam above (horizontal line v = max.y).
            if r + 1 < rows {
                let y = t.rect_mm.max.y;
                let mut ps = positions(t.rect_mm.min.x, t.rect_mm.max.x, margin(c > 0), margin(c + 1 < cols));
                ps.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
                for x in ps {
                    let (b, a) = (DVec2::new(x, y - ks.offset_mm), DVec2::new(x, y + ks.offset_mm));
                    out.holes[idx(c, r)].push(b);
                    out.holes[idx(c, r + 1)].push(a);
                    out.pairs.push((b, a));
                }
            }
        }
    }
    let _ = CutAxis::U;
    out
}

/// Regular `n`-gon, CCW, first vertex at angle `phase`.
pub fn circle(c: DVec2, r: f64, n: u32, phase: f64) -> Vec<DVec2> {
    (0..n)
        .map(|k| {
            let a = phase + TAU * k as f64 / n as f64;
            c + r * DVec2::new(a.cos(), a.sin())
        })
        .collect()
}

/// The hole pockets for one tile's key centres.
pub fn key_regions(holes: &[DVec2], ks: &KeySpec) -> Vec<BackRegion> {
    // Circumscribed polygon: its inscribed circle has the hole diameter.
    let r = 0.5 * ks.hole_d_mm / (PI / ks.segments as f64).cos();
    holes
        .iter()
        .map(|&c| BackRegion { poly: circle(c, r, ks.segments, 0.0), w_mm: ks.hole_depth_mm })
        .collect()
}

/// `count` splints in one multi-shell mesh, laid out in a grid on `z = 0` from the
/// origin: each a `splint_w × (2·offset + peg_d + 3)` bar with two pegs `2·offset`
/// apart, printed flat with the pegs up. Faces are `FaceKind::Key`.
pub fn splints_mesh(count: u32, ks: &KeySpec) -> Mesh {
    let len = ks.splint_len_mm();
    let (w, gap) = (ks.splint_w_mm, 4.0);
    let per_row = ((count as f64).sqrt().ceil() as u32).max(1);
    let mut m = Mesh::default();
    for k in 0..count {
        let (i, j) = (k % per_row, k / per_row);
        let o = DVec2::new(i as f64 * (len + gap), j as f64 * (w + gap));
        let outer = vec![o, o + DVec2::new(len, 0.0), o + DVec2::new(len, w), o + DVec2::new(0.0, w)];
        let c = o + DVec2::new(0.5 * len, 0.5 * w);
        let r = 0.5 * ks.peg_d_mm;
        let pegs = [
            circle(c - DVec2::new(ks.offset_mm, 0.0), r, ks.segments, 0.0),
            circle(c + DVec2::new(ks.offset_mm, 0.0), r, ks.segments, 0.0),
        ];
        let shell = solid_with_bosses(&outer, ks.splint_t_mm, &pegs, ks.peg_h_mm, FaceKind::Key)
            .expect("splint outline is a valid polygon");
        append_mesh(&mut m, &shell);
    }
    m
}

/// A prism over the simple CCW polygon `outer` from `z = 0` to `z = t`, with a
/// prism of height `boss_h` rising from each simple CCW `boss` polygon (strictly
/// inside `outer`, pairwise disjoint) on its top face. Built from indices, so it is
/// closed and consistently wound.
pub fn solid_with_bosses(outer: &[DVec2], t: f64, bosses: &[Vec<DVec2>], boss_h: f64, kind: FaceKind) -> Result<Mesh> {
    let n = outer.len();
    let mut m = Mesh::default();
    let push = |m: &mut Mesh, p: DVec2, z: f64| m.push_vertex([p.x as f32, p.y as f32, z as f32]);
    let b0: Vec<u32> = outer.iter().map(|&p| push(&mut m, p, 0.0)).collect();
    let t0: Vec<u32> = outer.iter().map(|&p| push(&mut m, p, t)).collect();
    // Bottom faces −z.
    for tri in triangulate_outline(outer)? {
        m.push_triangle([b0[tri[0] as usize], b0[tri[2] as usize], b0[tri[1] as usize]], kind);
    }
    side_walls(&mut m, &b0, &t0, kind);
    // Top face with the bosses as holes.
    let holes: Vec<&[DVec2]> = bosses.iter().map(|b| b.as_slice()).collect();
    let mut top_ids = t0.clone();
    let mut boss_rings = Vec::with_capacity(bosses.len());
    for b in bosses {
        let ring: Vec<u32> = b.iter().map(|&p| push(&mut m, p, t)).collect();
        top_ids.extend_from_slice(&ring);
        boss_rings.push(ring);
    }
    let top_tris = if bosses.is_empty() { triangulate_outline(outer)? } else { triangulate_with_holes(outer, &holes)? };
    for tri in top_tris {
        m.push_triangle(tri.map(|k| top_ids[k as usize]), kind);
    }
    for (b, ring) in bosses.iter().zip(&boss_rings) {
        let cap: Vec<u32> = b.iter().map(|&p| push(&mut m, p, t + boss_h)).collect();
        side_walls(&mut m, ring, &cap, kind);
        for tri in triangulate_outline(b)? {
            m.push_triangle(tri.map(|k| cap[k as usize]), kind);
        }
    }
    let _ = n;
    Ok(m)
}

/// Outward walls between a lower ring and an upper ring (same CCW polygon).
fn side_walls(m: &mut Mesh, lo: &[u32], hi: &[u32], kind: FaceKind) {
    let n = lo.len();
    for k in 0..n {
        let k1 = (k + 1) % n;
        m.push_triangle([lo[k], lo[k1], hi[k1]], kind);
        m.push_triangle([lo[k], hi[k1], hi[k]], kind);
    }
}

/// Appends `src` to `dst` as separate shells (multi-shell output).
pub fn append_mesh(dst: &mut Mesh, src: &Mesh) {
    let off = dst.vertex_count() as u32;
    dst.positions.extend_from_slice(&src.positions);
    dst.indices.extend(src.indices.iter().map(|&i| i + off));
    if src.face_kind.len() == src.triangle_count() {
        dst.face_kind.extend_from_slice(&src.face_kind);
    } else {
        dst.face_kind.resize(dst.triangle_count(), FaceKind::Other);
    }
}

// ---------------------------------------------------------------------------
// Engraving
// ---------------------------------------------------------------------------

/// Depth of the engraved tile id and UP arrow (§14.2).
pub const ENGRAVE_DEPTH_MM: f64 = 0.4;

/// 7-segment digit, segments as separate rectangles (0..7 × 0..11 mm) so every
/// pair stays at least 1 mm apart (§14.3 region gap).
fn digit_segments(d: u32) -> Vec<[f64; 4]> {
    // x0, y0, x1, y1
    const A: [f64; 4] = [2.1, 10.0, 4.9, 11.0];
    const B: [f64; 4] = [6.0, 6.1, 7.0, 11.0];
    const C: [f64; 4] = [6.0, 0.0, 7.0, 4.9];
    const D: [f64; 4] = [2.1, 0.0, 4.9, 1.0];
    const E: [f64; 4] = [0.0, 0.0, 1.0, 4.9];
    const F: [f64; 4] = [0.0, 6.1, 1.0, 11.0];
    const G: [f64; 4] = [2.1, 5.0, 4.9, 6.0];
    let segs: &[[f64; 4]] = match d {
        0 => &[A, B, C, D, E, F],
        1 => &[B, C],
        2 => &[A, B, G, E, D],
        3 => &[A, B, G, C, D],
        4 => &[F, G, B, C],
        5 => &[A, F, G, C, D],
        6 => &[A, F, G, E, C, D],
        7 => &[A, B, C],
        8 => &[A, B, C, D, E, F, G],
        _ => &[A, B, C, D, F, G],
    };
    segs.to_vec()
}

/// Digit cell size (mm) and advance.
const DIGIT_W: f64 = 7.0;
const DIGIT_H: f64 = 11.0;
const DIGIT_ADVANCE: f64 = 9.0;

/// UP arrow (pointing +v), 9 × 16 mm, CCW, centred on `c`.
fn arrow(c: DVec2) -> Vec<DVec2> {
    let p = |x: f64, y: f64| c + DVec2::new(x, y);
    vec![p(-1.5, -8.0), p(1.5, -8.0), p(1.5, 2.0), p(4.5, 2.0), p(0.0, 8.0), p(-4.5, 2.0), p(-1.5, 2.0)]
}

/// Engraving for one tile: an UP arrow at the tile centre and the label (digits)
/// below it, mirrored in u so it reads correctly from behind. Returns no region
/// that would come within `gap + 1` mm of the tile edge or of `avoid`.
pub fn engrave_regions(tile: &Tile, label: &str, avoid: &[BackRegion]) -> Vec<BackRegion> {
    let r = tile.rect_mm;
    let c = 0.5 * (r.min + r.max);
    let mut regions = vec![BackRegion { poly: arrow(c + DVec2::new(0.0, 4.0)), w_mm: ENGRAVE_DEPTH_MM }];
    let digits: Vec<u32> = label.chars().filter_map(|ch| ch.to_digit(10)).collect();
    let total_w = DIGIT_W + DIGIT_ADVANCE * (digits.len().max(1) - 1) as f64;
    // Seen from the back, +u points left: lay digits out from the right in plate u.
    let y0 = c.y - 4.0 - 3.0 - DIGIT_H;
    for (k, &d) in digits.iter().enumerate() {
        let x_right = c.x + 0.5 * total_w - DIGIT_ADVANCE * k as f64;
        for s in digit_segments(d) {
            // Mirror: glyph x → x_right − x.
            let (x0, x1) = (x_right - s[2], x_right - s[0]);
            let (y_lo, y_hi) = (y0 + s[1], y0 + s[3]);
            regions.push(BackRegion {
                poly: vec![DVec2::new(x0, y_lo), DVec2::new(x1, y_lo), DVec2::new(x1, y_hi), DVec2::new(x0, y_hi)],
                w_mm: ENGRAVE_DEPTH_MM,
            });
        }
    }
    let margin = BACK_REGION_GAP_MM + 1.0;
    let fits = |poly: &[DVec2]| {
        poly.iter().all(|p| {
            p.x >= r.min.x + margin && p.x <= r.max.x - margin && p.y >= r.min.y + margin && p.y <= r.max.y - margin
        }) && avoid.iter().all(|a| poly_gap(poly, &a.poly) >= margin)
    };
    if regions.iter().all(|g| fits(&g.poly)) {
        regions
    } else if fits(&regions[0].poly) {
        regions.truncate(1);
        regions
    } else {
        Vec::new()
    }
}

/// Lower bound on the distance between two polygons (vertex-to-edge distances, 0
/// if one contains a vertex of the other).
fn poly_gap(a: &[DVec2], b: &[DVec2]) -> f64 {
    let mut d = f64::INFINITY;
    for i in 0..a.len() {
        for j in 0..b.len() {
            d = d.min(crate::relief::seg_seg_dist(a[i], a[(i + 1) % a.len()], b[j], b[(j + 1) % b.len()]));
        }
    }
    let inside = |p: DVec2, q: &[DVec2]| {
        let mut ins = false;
        for k in 0..q.len() {
            let (s, e) = (q[k], q[(k + 1) % q.len()]);
            if (s.y > p.y) != (e.y > p.y) && p.x < s.x + (p.y - s.y) / (e.y - s.y) * (e.x - s.x) {
                ins = !ins;
            }
        }
        ins
    };
    if inside(a[0], b) || inside(b[0], a) { 0.0 } else { d }
}

// ---------------------------------------------------------------------------
// Stand
// ---------------------------------------------------------------------------

/// A foot that holds the assembled plate upright (§14.2): a slab with two walls
/// forming a slot, extruded along the plate's width. Printed flat.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StandSpec {
    /// Slot width: plate thickness + clearance (splints are kept out of the slot).
    pub slot_mm: f64,
    /// Front-to-back depth of the base slab.
    pub base_depth_mm: f64,
    /// Total height and slot depth.
    pub height_mm: f64,
    pub slot_depth_mm: f64,
    /// Base slab thickness and slot wall thickness.
    pub base_mm: f64,
    pub wall_mm: f64,
    /// Length of one foot along the plate, and how many feet.
    pub length_mm: f64,
    pub count: u32,
}

/// Clearance added to the plate thickness for the slot (§14.2): both the slot
/// walls and the plate's thickness are printed as XY perimeters, and elephant's
/// foot can flare the plate's bottom edge.
pub const STAND_CLEARANCE_MM: f64 = 0.5;

/// Keys on a vertical seam keep this much above the slot (mm).
pub const STAND_KEY_CLEARANCE_MM: f64 = 3.0;

impl StandSpec {
    /// A stand for a plate `plate_w × plate_h` mm whose thickest point is
    /// `thickness` mm, fitted to `bed`. `_splint_t` is the splint thickness with
    /// keys; splints are kept out of the slot (see [`KeySpec::bottom_clear_mm`]),
    /// so it does not widen the slot.
    pub fn for_plate(plate_w: f64, plate_h: f64, thickness: f64, _splint_t: f64, bed: &Bed) -> StandSpec {
        let [ux, uy, uz] = bed.usable();
        let long = ux.max(uy);
        let base_mm = 4.0;
        // A deeper slot for taller plates keeps the lean from the clearance small.
        let slot_depth_mm = (0.06 * plate_h).clamp(15.0, 40.0).min(uz - base_mm - 1.0);
        let height_mm = (0.08 * plate_h).clamp(15.0, 30.0).max(slot_depth_mm + base_mm + 1.0).min(uz);
        let slot_mm = thickness + STAND_CLEARANCE_MM;
        let wall_mm = 4.0;
        let base_depth_mm = (0.25 * plate_h).clamp(40.0, 120.0).max(slot_mm + 2.0 * wall_mm + 10.0).min(ux.min(uy));
        let (length_mm, count) =
            if plate_w <= 150.0 { ((0.7 * plate_w).max(20.0).min(long), 1) } else { ((0.25 * plate_w).min(80.0).min(long), 2) };
        StandSpec { slot_mm, base_depth_mm, height_mm, slot_depth_mm, base_mm, wall_mm, length_mm, count }
    }

    pub fn check(&self) -> Result<()> {
        let ok = [self.slot_mm, self.base_depth_mm, self.height_mm, self.slot_depth_mm, self.base_mm, self.wall_mm, self.length_mm]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.5)
            && self.base_depth_mm >= self.slot_mm + 2.0 * self.wall_mm + 1.0
            && self.height_mm - self.slot_depth_mm >= self.base_mm
            && self.count >= 1;
        if ok { Ok(()) } else { Err(Error::InvalidParams(format!("inconsistent stand {self:?}"))) }
    }

    /// The cross-section in (depth, height), CCW, depth centred on 0.
    pub fn profile(&self) -> Vec<DVec2> {
        let (d, s, t, h, b) = (0.5 * self.base_depth_mm, 0.5 * self.slot_mm, self.wall_mm, self.height_mm, self.base_mm);
        let f = h - self.slot_depth_mm;
        let p = DVec2::new;
        let mut v = vec![p(-d, 0.0), p(d, 0.0)];
        if s + t < d - 1e-9 {
            v.extend([p(d, b), p(s + t, b)]);
        }
        v.extend([p(s + t, h), p(s, h), p(s, f), p(-s, f), p(-s, h), p(-s - t, h)]);
        if s + t < d - 1e-9 {
            v.extend([p(-s - t, b), p(-d, b)]);
        }
        v
    }
}

/// One stand foot: the profile extruded along X from 0 to `length`, depth along Y
/// from 0, height along Z from 0. Faces are `FaceKind::Stand`.
pub fn stand_mesh(s: &StandSpec) -> Result<Mesh> {
    s.check()?;
    let m = solid_with_bosses(&s.profile(), s.length_mm, &[], 0.0, FaceKind::Stand)?;
    // (a, b, c) = (depth, height, length) → (X, Y, Z) = (c, a + D/2, b): a cyclic
    // permutation, so orientation is kept.
    let half = (0.5 * s.base_depth_mm) as f32;
    Ok(m.map_positions(|p| [p[2], p[0] + half, p[1]]))
}

// ---------------------------------------------------------------------------
// Bed layout
// ---------------------------------------------------------------------------

/// One part to place: its orientation transform (part space → print space,
/// before placement), its part-space bounds, and whether it may turn 90° about Z.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutItem {
    pub base: Xform,
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub may_rotate: bool,
}

/// Where a part ended up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub transform: Xform,
    pub bed: u32,
    /// `false` when the part is larger than the usable bed (placed alone on its
    /// own bed; only with [`layout_on_beds_lenient`]).
    pub fits: bool,
}

/// Gap between parts on one bed (mm): room for two brims between standing tiles.
pub const PART_GAP_MM: f64 = 10.0;

/// Shelf-packs the items on as many beds as needed (in order, no sorting, so the
/// layout is deterministic and tiles stay in reading order). Every transform is
/// in its own bed's coordinates (`Placement::bed` says which print job): each part
/// lies inside the usable area `[m, x − m] × [m, y − m]` with `Z ≥ 0`, and no two
/// parts on one bed overlap. Write one file per bed.
pub fn layout_on_beds(items: &[LayoutItem], bed: &Bed) -> Result<Vec<Placement>> {
    layout_inner(items, bed, false)
}

/// [`layout_on_beds`] that places a part too large for the bed alone on a bed of
/// its own (with `fits: false`) instead of failing, for a user-forced tile grid.
pub fn layout_on_beds_lenient(items: &[LayoutItem], bed: &Bed) -> Result<Vec<Placement>> {
    layout_inner(items, bed, true)
}

/// Free rectangle of one bed, in usable-area coordinates (origin at the margin).
#[derive(Debug, Clone, Copy)]
struct FreeRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl FreeRect {
    fn contains(&self, o: &FreeRect) -> bool {
        o.x >= self.x - 1e-9 && o.y >= self.y - 1e-9 && o.x + o.w <= self.x + self.w + 1e-9 && o.y + o.h <= self.y + self.h + 1e-9
    }
}

/// One bed being filled: maximal free rectangles. Every part claims its footprint
/// plus [`PART_GAP_MM`] on the far sides, and the bed is that gap larger than the
/// usable area, so parts are a gap apart and stay inside the margins.
struct BedFill {
    free: Vec<FreeRect>,
    /// (item, x, y, w, h) placed on this bed.
    placed: Vec<(usize, f64, f64, f64, f64)>,
}

impl BedFill {
    fn new(ux: f64, uy: f64) -> Self {
        BedFill { free: vec![FreeRect { x: 0.0, y: 0.0, w: ux + PART_GAP_MM, h: uy + PART_GAP_MM }], placed: Vec::new() }
    }

    /// Bottom-left position for a `w × h` part (gap included), if any.
    fn find(&self, w: f64, h: f64) -> Option<(f64, f64)> {
        self.free
            .iter()
            .filter(|f| w <= f.w + 1e-9 && h <= f.h + 1e-9)
            .map(|f| (f.x, f.y))
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.total_cmp(&b.0)))
    }

    fn claim(&mut self, r: FreeRect) {
        let mut next = Vec::with_capacity(self.free.len() + 4);
        for f in &self.free {
            let disjoint = r.x >= f.x + f.w - 1e-9 || r.x + r.w <= f.x + 1e-9 || r.y >= f.y + f.h - 1e-9 || r.y + r.h <= f.y + 1e-9;
            if disjoint {
                next.push(*f);
                continue;
            }
            if r.x > f.x + 1e-9 {
                next.push(FreeRect { x: f.x, y: f.y, w: r.x - f.x, h: f.h });
            }
            if r.x + r.w < f.x + f.w - 1e-9 {
                next.push(FreeRect { x: r.x + r.w, y: f.y, w: f.x + f.w - (r.x + r.w), h: f.h });
            }
            if r.y > f.y + 1e-9 {
                next.push(FreeRect { x: f.x, y: f.y, w: f.w, h: r.y - f.y });
            }
            if r.y + r.h < f.y + f.h - 1e-9 {
                next.push(FreeRect { x: f.x, y: r.y + r.h, w: f.w, h: f.y + f.h - (r.y + r.h) });
            }
        }
        // Drop rectangles contained in another (keep the first of equal ones).
        let mut keep = vec![true; next.len()];
        for i in 0..next.len() {
            for j in 0..next.len() {
                if i != j && keep[j] && next[j].contains(&next[i]) && !(next[i].contains(&next[j]) && i < j) {
                    keep[i] = false;
                    break;
                }
            }
        }
        self.free = next.into_iter().zip(keep).filter_map(|(f, k)| k.then_some(f)).collect();
    }
}

fn layout_inner(items: &[LayoutItem], bed: &Bed, lenient: bool) -> Result<Vec<Placement>> {
    let [ux, uy, uz] = bed.usable();
    let m = bed.margin_mm;
    let g = PART_GAP_MM;
    // (bed, x, y, rotated, fits) per item, in usable coordinates.
    let mut spots: Vec<(u32, f64, f64, bool, bool)> = Vec::with_capacity(items.len());
    let mut beds: Vec<Option<BedFill>> = Vec::new(); // None = a bed holding one oversize part
    for (i, it) in items.iter().enumerate() {
        let (lo, hi) = it.base.apply_box(it.min, it.max);
        let (fx, fy, fz) = (hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]);
        let fits_as = |w: f64, h: f64| w <= ux + 1e-9 && h <= uy + 1e-9 && fz <= uz + 1e-9;
        let mut shapes = Vec::with_capacity(2);
        if fits_as(fx, fy) {
            shapes.push((fx, fy, false));
        }
        if it.may_rotate && fits_as(fy, fx) && (fx - fy).abs() > 1e-9 {
            shapes.push((fy, fx, true));
        }
        if shapes.is_empty() {
            if lenient {
                beds.push(None);
                spots.push(((beds.len() - 1) as u32, 0.0, 0.0, false, false));
                continue;
            }
            return Err(Error::InvalidParams(format!(
                "part {i} ({fx:.1} × {fy:.1} × {fz:.1} mm) does not fit the usable bed ({ux:.1} × {uy:.1} × {uz:.1} mm)"
            )));
        }
        // The earliest bed with room (unrotated first), else a new bed.
        let mut spot = None;
        'beds: for (b, fill) in beds.iter().enumerate() {
            let Some(fill) = fill else { continue };
            for &(w, h, rot) in &shapes {
                if let Some((x, y)) = fill.find(w + g, h + g) {
                    spot = Some((b, x, y, w, h, rot));
                    break 'beds;
                }
            }
        }
        let (b, x, y, w, h, rot) = match spot {
            Some(s) => s,
            None => {
                beds.push(Some(BedFill::new(ux, uy)));
                let (w, h, rot) = shapes[0];
                (beds.len() - 1, 0.0, 0.0, w, h, rot)
            }
        };
        let fill = beds[b].as_mut().expect("a packing bed");
        fill.claim(FreeRect { x, y, w: w + g, h: h + g });
        fill.placed.push((i, x, y, w, h));
        spots.push((b as u32, x, y, rot, true));
    }
    // Centre each bed's group on the usable area (the middle of a bed is its
    // flattest and best-levelled part, and brims stay clear of the edges).
    let mut shift = vec![(0.0f64, 0.0f64); beds.len()];
    for (b, fill) in beds.iter().enumerate() {
        let Some(fill) = fill else { continue };
        let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for &(_, x, y, w, h) in &fill.placed {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x + w);
            y1 = y1.max(y + h);
        }
        if x0.is_finite() {
            shift[b] = (0.5 * (ux - (x1 - x0)) - x0, 0.5 * (uy - (y1 - y0)) - y0);
        }
    }
    let mut out = Vec::with_capacity(items.len());
    for (it, &(b, x, y, rot, fits)) in items.iter().zip(&spots) {
        let mut t = it.base;
        if rot {
            t = t.then(&Xform::rot_z(1));
        }
        let (lo2, _) = t.apply_box(it.min, it.max);
        let (sx, sy) = if fits { shift[b as usize] } else { (0.0, 0.0) };
        let origin = [m + x + sx, m + y + sy, 0.0];
        t = t.then(&Xform::translation([origin[0] - lo2[0], origin[1] - lo2[1], -lo2[2]]));
        out.push(Placement { transform: t, bed: b, fits });
    }
    Ok(out)
}

/// Part-space bounds of a mesh in f64.
pub fn mesh_box(m: &Mesh) -> ([f64; 3], [f64; 3]) {
    match m.bounds() {
        Some(b) => (b.min.map(|v| v as f64), b.max.map(|v| v as f64)),
        None => ([0.0; 3], [0.0; 3]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::validate::validate;

    fn ok(m: &Mesh, comps: usize) {
        let r = validate(m, &ValidateOpts { expect_components: comps, check_self_intersection: true, ..Default::default() });
        assert!(r.ok, "{}", r.summary());
    }

    #[test]
    fn splints_are_valid_multi_shell() {
        for n in [1, 2, 5, 9] {
            let m = splints_mesh(n, &KeySpec::default());
            ok(&m, n as usize);
        }
    }

    #[test]
    fn stand_is_valid() {
        for (w, h, t) in [(100.0, 120.0, 5.0), (300.0, 378.0, 4.2), (600.0, 1000.0, 12.0)] {
            let s = StandSpec::for_plate(w, h, t, 0.0, &Bed::CR6_SE);
            let m = stand_mesh(&s).unwrap();
            ok(&m, 1);
            let b = m.bounds().unwrap();
            assert_eq!(b.min, [0.0, 0.0, 0.0]);
            assert!((b.max[0] as f64 - s.length_mm).abs() < 1e-4);
        }
    }

    #[test]
    fn layout_does_not_overlap() {
        let bed = Bed::CR6_SE;
        let items: Vec<LayoutItem> = (0..7)
            .map(|k| LayoutItem {
                base: Xform::IDENTITY,
                min: [0.0; 3],
                max: [40.0 + 10.0 * k as f64, 100.0, 5.0],
                may_rotate: true,
            })
            .collect();
        let pl = layout_on_beds(&items, &bed).unwrap();
        let boxes: Vec<_> = items.iter().zip(&pl).map(|(it, p)| (p.bed, p.transform.apply_box(it.min, it.max))).collect();
        let u = bed.usable();
        for (i, (bi, (lo, hi))) in boxes.iter().enumerate() {
            let x0 = bed.margin_mm;
            assert!(lo[0] >= x0 - 1e-9 && hi[0] <= x0 + u[0] + 1e-9);
            assert!(lo[1] >= bed.margin_mm - 1e-9 && hi[1] <= bed.margin_mm + u[1] + 1e-9);
            assert!(lo[2].abs() < 1e-12);
            for (bj, (lo2, hi2)) in &boxes[i + 1..] {
                let overlap = bi == bj && (0..2).all(|k| lo[k] < hi2[k] && lo2[k] < hi[k]);
                assert!(!overlap);
            }
        }
    }
}
