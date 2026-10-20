//! Export orientation (design §9). Plate space is `u` right, `v` up, `w` out of
//! the front face. Every option is a proper rotation plus a translation.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuarterTurns {
    Auto,
    Q0,
    Q1,
    Q2,
    Q3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orientation {
    Standing(QuarterTurns),
    Flat,
}

impl Default for Orientation {
    fn default() -> Self {
        Orientation::Standing(QuarterTurns::Auto)
    }
}

/// `p' = m · p + t`, applied in f64.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Xform {
    pub m: [[f64; 3]; 3],
    pub t: [f64; 3],
}

impl Xform {
    pub const IDENTITY: Xform = Xform { m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], t: [0.0; 3] };

    #[inline]
    pub fn apply(&self, p: [f64; 3]) -> [f64; 3] {
        let m = &self.m;
        [
            m[0][0] * p[0] + m[0][1] * p[1] + m[0][2] * p[2] + self.t[0],
            m[1][0] * p[0] + m[1][1] * p[1] + m[1][2] * p[2] + self.t[1],
            m[2][0] * p[0] + m[2][1] * p[1] + m[2][2] * p[2] + self.t[2],
        ]
    }

    /// The exact `f32` values written to STL/3MF for a stored `f32` vertex.
    /// `validate()` stage 2 and the STL writer both use this.
    #[inline]
    pub fn apply_f32(&self, p: [f32; 3]) -> [f32; 3] {
        if *self == Xform::IDENTITY {
            return [p[0] + 0.0, p[1] + 0.0, p[2] + 0.0];
        }
        let q = self.apply([p[0] as f64, p[1] as f64, p[2] as f64]);
        [q[0] as f32 + 0.0, q[1] as f32 + 0.0, q[2] as f32 + 0.0]
    }

    pub fn det(&self) -> f64 {
        let m = &self.m;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }
}

/// Resolves `Auto` to the quarter turn that puts the light below (§9):
/// the `k` minimising `sin(φ + k·90°)`, ties to the smaller `k`.
pub fn auto_quarter_turns(light_azimuth: f64) -> u32 {
    let mut best = 0;
    let mut best_s = f64::INFINITY;
    for k in 0..4u32 {
        // Round to kill 1e-16 noise so exact ties pick the smaller k.
        let s = ((light_azimuth + k as f64 * std::f64::consts::FRAC_PI_2).sin() * 1e12).round();
        if s < best_s {
            best_s = s;
            best = k;
        }
    }
    best
}

/// The transform from plate space to print space for a `w × h` mm plate.
/// Returns the transform and the resolved quarter turns (None for Flat).
pub fn export_xform(o: Orientation, w: f64, h: f64, light_azimuth: f64) -> (Xform, Option<u32>) {
    let k = match o {
        Orientation::Flat => return (Xform::IDENTITY, None),
        Orientation::Standing(QuarterTurns::Auto) => auto_quarter_turns(light_azimuth),
        Orientation::Standing(QuarterTurns::Q0) => 0,
        Orientation::Standing(QuarterTurns::Q1) => 1,
        Orientation::Standing(QuarterTurns::Q2) => 2,
        Orientation::Standing(QuarterTurns::Q3) => 3,
    };
    // In-plane rotation by k·90° CCW, translated back to u', v' >= 0:
    // u' = r00 u + r01 v + tu, v' = r10 u + r11 v + tv.
    let (r, tu, tv) = match k {
        0 => ([[1.0, 0.0], [0.0, 1.0]], 0.0, 0.0),
        1 => ([[0.0, -1.0], [1.0, 0.0]], h, 0.0),
        2 => ([[-1.0, 0.0], [0.0, -1.0]], w, h),
        _ => ([[0.0, 1.0], [-1.0, 0.0]], 0.0, w),
    };
    // X = u', Y = -w, Z = v'.
    let x = Xform {
        m: [[r[0][0], r[0][1], 0.0], [0.0, 0.0, -1.0], [r[1][0], r[1][1], 0.0]],
        t: [tu, 0.0, tv],
    };
    (x, Some(k))
}

/// Which way the printer moves the part relative to the nozzle in the horizontal
/// plane. A bed-slinger (e.g. Creality CR-6 SE) moves the bed in `Y`; tall, thin
/// parts printed standing should then have their face parallel to that axis so the
/// bed's acceleration pushes them edge-on, not face-on (PLAN §4, START_HERE).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BedMotion {
    /// The bed moves in X.
    X,
    /// The bed moves in Y (bed-slinger such as the CR-6 SE).
    #[default]
    Y,
    /// The bed only moves in Z (CoreXY, delta): no preferred face direction.
    None,
}

impl Xform {
    /// A pure translation.
    pub fn translation(t: [f64; 3]) -> Xform {
        Xform { t, ..Xform::IDENTITY }
    }

    /// `n` quarter turns counter-clockwise about +Z (seen from above).
    pub fn rot_z(n: u32) -> Xform {
        let (c, s) = match n % 4 {
            0 => (1.0, 0.0),
            1 => (0.0, 1.0),
            2 => (-1.0, 0.0),
            _ => (0.0, -1.0),
        };
        Xform { m: [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]], t: [0.0; 3] }
    }

    /// `self` followed by `next`: `next(self(p))`. Exact for the axis-permutation
    /// matrices used here (entries 0 and ±1).
    pub fn then(&self, next: &Xform) -> Xform {
        let (a, b) = (&next.m, &self.m);
        let mut m = [[0.0; 3]; 3];
        for (i, row) in m.iter_mut().enumerate() {
            for (j, v) in row.iter_mut().enumerate() {
                *v = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
            }
        }
        Xform { m, t: next.apply(self.t) }
    }

    /// Axis-aligned bounds of the image of the box `[min, max]`.
    pub fn apply_box(&self, min: [f64; 3], max: [f64; 3]) -> ([f64; 3], [f64; 3]) {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for c in 0..8 {
            let p = [
                if c & 1 == 0 { min[0] } else { max[0] },
                if c & 2 == 0 { min[1] } else { max[1] },
                if c & 4 == 0 { min[2] } else { max[2] },
            ];
            let q = self.apply(p);
            for k in 0..3 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        }
        (lo, hi)
    }
}

/// Quarter turns ordered by preference for `Standing(Auto)` (§9): first every `k`
/// with `sin(φ + k·90°) ≤ 0` (the light comes from below or the lower side in print
/// space), by increasing `sin`, ties to the smaller `k`; then the rest, so a caller
/// that must fit a bed can still find an orientation. The first entry is always
/// [`auto_quarter_turns`].
pub fn auto_candidates(light_azimuth: f64) -> Vec<u32> {
    let s = |k: u32| ((light_azimuth + k as f64 * std::f64::consts::FRAC_PI_2).sin() * 1e12).round();
    let mut ks: Vec<u32> = (0..4).collect();
    ks.sort_by(|&a, &b| s(a).total_cmp(&s(b)).then(a.cmp(&b)));
    ks
}

/// `sin(φ + k·90°)`, the §9 Auto score (lower is better; ≤ 0 means no main step
/// faces down).
pub fn light_score(light_azimuth: f64, k: u32) -> f64 {
    (light_azimuth + k as f64 * std::f64::consts::FRAC_PI_2).sin()
}

/// The `k` an explicit standing orientation names (`None` for `Auto` or `Flat`).
pub fn fixed_quarter_turns(o: Orientation) -> Option<u32> {
    match o {
        Orientation::Standing(QuarterTurns::Q0) => Some(0),
        Orientation::Standing(QuarterTurns::Q1) => Some(1),
        Orientation::Standing(QuarterTurns::Q2) => Some(2),
        Orientation::Standing(QuarterTurns::Q3) => Some(3),
        _ => None,
    }
}

/// In-plane rotation by `k·90°` CCW of a `w × h` rectangle, translated back to
/// `u', v' ≥ 0`: `u' = r[0]·(u, v) + t[0]`, `v' = r[1]·(u, v) + t[1]`.
pub fn in_plane_rotation(k: u32, w: f64, h: f64) -> ([[f64; 2]; 2], [f64; 2]) {
    match k % 4 {
        0 => ([[1.0, 0.0], [0.0, 1.0]], [0.0, 0.0]),
        1 => ([[0.0, -1.0], [1.0, 0.0]], [h, 0.0]),
        2 => ([[-1.0, 0.0], [0.0, -1.0]], [w, h]),
        _ => ([[0.0, 1.0], [-1.0, 0.0]], [0.0, w]),
    }
}

/// Plate space → print space for a `w × h` plate standing on its (rotated) bottom
/// edge, with its face parallel to the bed's motion axis:
/// - `BedMotion::Y`: `X = w`, `Y = u'`, `Z = v'` (front face toward +X);
/// - `BedMotion::X` and `None`: `X = u'`, `Y = −w`, `Z = v'` (the §9 mapping, front
///   face toward −Y), identical to [`export_xform`].
///
/// Always a proper rotation plus a translation; the back face lies on `X = 0` or
/// `Y = 0` and the bottom edge on `Z = 0`.
pub fn standing_xform(k: u32, w: f64, h: f64, motion: BedMotion) -> Xform {
    let (r, t) = in_plane_rotation(k, w, h);
    match motion {
        BedMotion::Y => Xform {
            m: [[0.0, 0.0, 1.0], [r[0][0], r[0][1], 0.0], [r[1][0], r[1][1], 0.0]],
            t: [0.0, t[0], t[1]],
        },
        BedMotion::X | BedMotion::None => Xform {
            m: [[r[0][0], r[0][1], 0.0], [0.0, 0.0, -1.0], [r[1][0], r[1][1], 0.0]],
            t: [t[0], 0.0, t[1]],
        },
    }
}

/// [`export_xform`] that also honours the bed's motion axis (for `Standing`).
/// `Auto` resolves to [`auto_quarter_turns`]; fitting a bed is the tiler's job
/// (`squama::tiles::plan_tiles`).
pub fn bed_xform(o: Orientation, w: f64, h: f64, light_azimuth: f64, motion: BedMotion) -> (Xform, Option<u32>) {
    let k = match o {
        Orientation::Flat => return (Xform::IDENTITY, None),
        Orientation::Standing(QuarterTurns::Auto) => auto_quarter_turns(light_azimuth),
        _ => fixed_quarter_turns(o).unwrap_or(0),
    };
    (standing_xform(k, w, h, motion), Some(k))
}

/// The in-plane plate-space direction that points down (−Z) in print space when
/// the plate stands after `k` quarter turns. Ledges are walls facing this way.
pub fn down_in_plate(k: u32) -> glam::DVec2 {
    let (r, _) = in_plane_rotation(k, 1.0, 1.0);
    // Z = v' = r[1]·(u, v), so −Z is −r[1].
    glam::DVec2::new(-r[1][0], -r[1][1])
}

/// Largest downward-facing ledge (mm) over the interior walls of `pieces` when the
/// plate stands with plate-space direction `down` pointing to −Z (§9 report).
///
/// A wall between pieces A and B on edge `a → b` faces from the higher piece to the
/// lower one; at each end its depth is the height difference there. It counts as a
/// ledge when its facing has a component of at least cos 60° along `down`. Boundary
/// walls are skipped (the one facing down stands on the bed) and saddle splits are
/// ignored (they move vertices by 50 µm).
pub fn max_downward_ledge(
    p: &crate::relief::Partition,
    h: &crate::relief::Heights,
    pieces: &[u32],
    down: glam::DVec2,
    tol: &crate::relief::Tol,
) -> f64 {
    use std::collections::HashMap;
    let next = |pi: u32, s: usize| {
        let r = &p.pieces[pi as usize].ring;
        if s + 1 == r.end as usize { r.start as usize } else { s + 1 }
    };
    // Directed edge (a, b) → (slot of a, slot of b) in the piece that owns it.
    let mut edge: HashMap<(u32, u32), (usize, usize)> = HashMap::with_capacity(pieces.len() * 4);
    for &pi in pieces {
        let r = p.pieces[pi as usize].ring.clone();
        for s in r.start as usize..r.end as usize {
            let ns = next(pi, s);
            edge.insert((p.rings[s], p.rings[ns]), (s, ns));
        }
    }
    let down = down.normalize_or_zero();
    let mut best = 0i64;
    for (&(a, b), &(sa, sb)) in &edge {
        if a > b {
            continue; // each interior edge once
        }
        let Some(&(tb, ta)) = edge.get(&(b, a)) else { continue };
        let e = p.verts[b as usize] - p.verts[a as usize];
        let right = glam::DVec2::new(e.y, -e.x).normalize_or_zero();
        for d in [h.corner[sa] - h.corner[ta], h.corner[sb] - h.corner[tb]] {
            let facing = if d > 0 { right } else { -right };
            if d != 0 && facing.dot(down) >= 0.5 {
                best = best.max(d.abs());
            }
        }
    }
    best as f64 * tol.q
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn standing_xform_matches_export_xform_without_bed_motion() {
        for k in 0..4 {
            let o = [QuarterTurns::Q0, QuarterTurns::Q1, QuarterTurns::Q2, QuarterTurns::Q3][k as usize];
            let (x, _) = export_xform(Orientation::Standing(o), 120.0, 80.0, PI);
            assert_eq!(x, standing_xform(k, 120.0, 80.0, BedMotion::None));
            assert_eq!(x, standing_xform(k, 120.0, 80.0, BedMotion::X));
        }
    }

    #[test]
    fn bed_slinger_puts_face_parallel_to_y() {
        for k in 0..4 {
            let x = standing_xform(k, 120.0, 80.0, BedMotion::Y);
            assert!((x.det() - 1.0).abs() < 1e-12);
            // The plate normal (w) maps to ±X: the face is parallel to Y (and Z).
            let n = [x.m[0][2], x.m[1][2], x.m[2][2]];
            assert_eq!(n, [1.0, 0.0, 0.0]);
            // The plate lands in X ∈ [0, w], Y ∈ [0, ·], Z ∈ [0, ·].
            let (lo, hi) = x.apply_box([0.0, 0.0, 0.0], [120.0, 80.0, 5.0]);
            assert_eq!(lo, [0.0, 0.0, 0.0]);
            let (ew, eh) = if k % 2 == 0 { (120.0, 80.0) } else { (80.0, 120.0) };
            assert_eq!(hi, [5.0, ew, eh]);
        }
    }

    #[test]
    fn auto_candidates_start_with_auto() {
        for d in 0..72 {
            let az = (d as f64 * 5.0).to_radians();
            let c = auto_candidates(az);
            assert_eq!(c[0], auto_quarter_turns(az));
            let mut s = c.clone();
            s.sort();
            assert_eq!(s, vec![0, 1, 2, 3]);
        }
    }

    #[test]
    fn down_direction_follows_rotation() {
        // k = 0: v' = v, so down is −v.
        assert_eq!(down_in_plate(0), glam::DVec2::new(0.0, -1.0));
        // k = 2: v' = h − v, so down is +v.
        assert_eq!(down_in_plate(2), glam::DVec2::new(0.0, 1.0));
        // k = 1: v' = u, so down is −u.
        assert_eq!(down_in_plate(1), glam::DVec2::new(-1.0, 0.0));
    }

    #[test]
    fn then_composes() {
        let a = standing_xform(1, 50.0, 30.0, BedMotion::Y);
        let b = Xform::rot_z(1).then(&Xform::translation([10.0, 20.0, 0.0]));
        let c = a.then(&b);
        let p = [3.0, 4.0, 5.0];
        let q = b.apply(a.apply(p));
        assert_eq!(c.apply(p), q);
        assert!((c.det() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn all_options_are_proper_rotations() {
        for o in [QuarterTurns::Q0, QuarterTurns::Q1, QuarterTurns::Q2, QuarterTurns::Q3] {
            let (x, _) = export_xform(Orientation::Standing(o), 300.0, 378.0, PI);
            assert!((x.det() - 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn q2_example_from_design() {
        let (x, _) = export_xform(Orientation::Standing(QuarterTurns::Q2), 300.0, 378.0, PI);
        let p = x.apply([10.0, 20.0, 3.0]);
        assert_eq!(p, [290.0, -3.0, 358.0]);
    }

    #[test]
    fn auto_puts_light_below() {
        // Left light (180°): k = 1 gives sin(270°) = -1.
        assert_eq!(auto_quarter_turns(PI), 1);
        // Bottom light (270°): already below.
        assert_eq!(auto_quarter_turns(1.5 * PI), 0);
        // Top light (90°): flip.
        assert_eq!(auto_quarter_turns(0.5 * PI), 2);
    }
}
