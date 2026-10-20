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

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

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
