//! Brightness response (§3, §15.2): how bright a facet looks at tilt `θ` under
//! its light, and the inverse used to turn target brightness into tilt.
//!
//! - `Lambert { ambient }`: `B(θ) = ambient + I·max(0, sin(α + θ))`.
//! - `Measured { tilt_rad, brightness }`: a calibration table (§15.3) measured
//!   under the design light, fitted monotone (pool-adjacent-violators), trimmed to
//!   its strictly rising range and interpolated with a monotone cubic
//!   (Fritsch–Carlson PCHIP), so the inverse is unique.

use crate::error::{Error, Result};

use super::light::CellLight;

/// §15.2 response model.
#[derive(Debug, Clone, PartialEq)]
pub enum Response {
    /// Lambert with a constant ambient term (relative to a directly lit facet = 1).
    Lambert { ambient: f64 },
    /// Measured brightness per tilt (radians), under the design light. Any order;
    /// at least two distinct tilts.
    Measured { tilt_rad: Vec<f64>, brightness: Vec<f64> },
}

impl Default for Response {
    fn default() -> Self {
        Response::Lambert { ambient: 0.0 }
    }
}

/// The monotone tilt range of one facet and the brightness it spans.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellRange {
    pub tilt_lo: f64,
    pub tilt_hi: f64,
    pub b_lo: f64,
    pub b_hi: f64,
}

/// A validated, ready-to-evaluate [`Response`].
#[derive(Debug, Clone, PartialEq)]
pub struct ResponseCurve {
    kind: CurveKind,
    /// Elevation the table was measured at (radians); a facet whose light sits at
    /// elevation `α_k` reads the table at `θ + α_k − ref_el`.
    ref_el: f64,
}

#[derive(Debug, Clone, PartialEq)]
enum CurveKind {
    Lambert { ambient: f64 },
    Table { t: Vec<f64>, b: Vec<f64>, m: Vec<f64> },
}

impl Response {
    /// Validates the response. `ref_el` is the design light's elevation at the
    /// plate centre (the calibration tile is lit the same way); Lambert ignores it.
    pub fn prepare(&self, ref_el: f64) -> Result<ResponseCurve> {
        match self {
            Response::Lambert { ambient } => {
                if !(ambient.is_finite() && *ambient >= 0.0) {
                    return Err(Error::InvalidParams(format!("ambient {ambient} must be finite and ≥ 0")));
                }
                Ok(ResponseCurve { kind: CurveKind::Lambert { ambient: *ambient }, ref_el })
            }
            Response::Measured { tilt_rad, brightness } => {
                if tilt_rad.len() != brightness.len() || tilt_rad.len() < 2 {
                    return Err(Error::InvalidParams("measured response needs ≥ 2 (tilt, brightness) pairs".into()));
                }
                if tilt_rad.iter().chain(brightness.iter()).any(|x| !x.is_finite()) {
                    return Err(Error::InvalidParams("measured response has non-finite values".into()));
                }
                if brightness.iter().any(|&b| b < 0.0) {
                    return Err(Error::InvalidParams("measured brightness must be ≥ 0".into()));
                }
                let mut pts: Vec<(f64, f64)> = tilt_rad.iter().copied().zip(brightness.iter().copied()).collect();
                pts.sort_by(|a, b| a.0.total_cmp(&b.0));
                if pts.windows(2).any(|w| w[1].0 - w[0].0 < 1e-9) {
                    return Err(Error::InvalidParams("measured response repeats a tilt".into()));
                }
                let t: Vec<f64> = pts.iter().map(|p| p.0).collect();
                let b = pav(&pts.iter().map(|p| p.1).collect::<Vec<_>>());
                // Trim to the strictly rising range: last point of the bottom plateau
                // to the first point at the top.
                let (bmin, bmax) = (b[0], b[b.len() - 1]);
                if !(bmax - bmin > 1e-9 * bmax.abs().max(1.0)) {
                    return Err(Error::InvalidParams("measured response has no contrast".into()));
                }
                let lo = b.iter().rposition(|&x| x == bmin).unwrap();
                let hi = b.iter().position(|&x| x == bmax).unwrap();
                let (t, b) = (t[lo..=hi].to_vec(), b[lo..=hi].to_vec());
                let m = pchip_slopes(&t, &b);
                Ok(ResponseCurve { kind: CurveKind::Table { t, b, m }, ref_el })
            }
        }
    }
}

impl ResponseCurve {
    /// Brightness of a facet tilted by `θ` toward its light.
    pub fn forward(&self, theta: f64, cl: &CellLight) -> f64 {
        match &self.kind {
            CurveKind::Lambert { ambient } => ambient + cl.irradiance * (cl.el + theta).sin().max(0.0),
            CurveKind::Table { t, b, m } => cl.irradiance * hermite(t, b, m, theta + cl.el - self.ref_el),
        }
    }

    /// The facet's monotone tilt range within `[−max_tilt, max_tilt]`, or `None`
    /// when its light lies outside the measured table.
    pub fn cell_range(&self, cl: &CellLight, max_tilt: f64) -> Option<CellRange> {
        let (lo, hi) = match &self.kind {
            // sin(α + θ) rises until α + θ = 90°.
            CurveKind::Lambert { .. } => (-max_tilt, max_tilt.min(std::f64::consts::FRAC_PI_2 - cl.el)),
            CurveKind::Table { t, .. } => {
                let d = cl.el - self.ref_el;
                ((-max_tilt).max(t[0] - d), max_tilt.min(t[t.len() - 1] - d))
            }
        };
        if !(hi > lo) {
            return None;
        }
        Some(CellRange { tilt_lo: lo, tilt_hi: hi, b_lo: self.forward(lo, cl), b_hi: self.forward(hi, cl) })
    }

    /// The tilt in `r` at which the facet shows brightness `b` (clamped to `r`).
    pub fn inverse(&self, b: f64, cl: &CellLight, r: &CellRange) -> f64 {
        match &self.kind {
            CurveKind::Lambert { ambient } => {
                let e = (b - ambient) / cl.irradiance;
                (e.clamp(0.0, 1.0).asin() - cl.el).clamp(r.tilt_lo, r.tilt_hi)
            }
            CurveKind::Table { t, b: tb, m } => {
                let target = b / cl.irradiance;
                let d = cl.el - self.ref_el;
                let (x_lo, x_hi) = (r.tilt_lo + d, r.tilt_hi + d);
                if target <= hermite(t, tb, m, x_lo) {
                    return r.tilt_lo;
                }
                if target >= hermite(t, tb, m, x_hi) {
                    return r.tilt_hi;
                }
                // Monotone non-decreasing: bisection converges to the first x
                // reaching the target. 60 halvings reach f64 resolution.
                let (mut a, mut c) = (x_lo, x_hi);
                for _ in 0..60 {
                    let mid = 0.5 * (a + c);
                    if hermite(t, tb, m, mid) < target {
                        a = mid;
                    } else {
                        c = mid;
                    }
                }
                (0.5 * (a + c) - d).clamp(r.tilt_lo, r.tilt_hi)
            }
        }
    }

    /// Ambient term (Lambert), or 0.
    pub fn ambient(&self) -> f64 {
        match self.kind {
            CurveKind::Lambert { ambient } => ambient,
            CurveKind::Table { .. } => 0.0,
        }
    }
}

/// Pool-adjacent-violators: the least-squares non-decreasing fit (unit weights).
pub fn pav(y: &[f64]) -> Vec<f64> {
    let mut blocks: Vec<(f64, usize)> = Vec::with_capacity(y.len()); // (mean, count)
    for &v in y {
        blocks.push((v, 1));
        while blocks.len() > 1 {
            let (m1, c1) = blocks[blocks.len() - 1];
            let (m0, c0) = blocks[blocks.len() - 2];
            if m0 <= m1 {
                break;
            }
            blocks.pop();
            let k = blocks.len() - 1;
            blocks[k] = ((m0 * c0 as f64 + m1 * c1 as f64) / (c0 + c1) as f64, c0 + c1);
        }
    }
    blocks.iter().flat_map(|&(m, c)| std::iter::repeat_n(m, c)).collect()
}

/// Fritsch–Carlson (Fritsch–Butland weighted harmonic mean) slopes for monotone
/// data; the cubic Hermite interpolant through them is monotone.
pub fn pchip_slopes(t: &[f64], b: &[f64]) -> Vec<f64> {
    let n = t.len();
    let h: Vec<f64> = (0..n - 1).map(|k| t[k + 1] - t[k]).collect();
    let d: Vec<f64> = (0..n - 1).map(|k| (b[k + 1] - b[k]) / h[k]).collect();
    let mut m = vec![0.0; n];
    m[0] = d[0];
    m[n - 1] = d[n - 2];
    for k in 1..n - 1 {
        if d[k - 1] * d[k] <= 0.0 {
            m[k] = 0.0;
        } else {
            let (w1, w2) = (2.0 * h[k] + h[k - 1], h[k] + 2.0 * h[k - 1]);
            m[k] = (w1 + w2) / (w1 / d[k - 1] + w2 / d[k]);
        }
    }
    m
}

/// Cubic Hermite evaluation, clamped to the table's ends.
fn hermite(t: &[f64], b: &[f64], m: &[f64], x: f64) -> f64 {
    let n = t.len();
    if x <= t[0] {
        return b[0];
    }
    if x >= t[n - 1] {
        return b[n - 1];
    }
    let k = t.partition_point(|&ti| ti <= x).saturating_sub(1).min(n - 2);
    let h = t[k + 1] - t[k];
    let s = (x - t[k]) / h;
    let (s2, s3) = (s * s, s * s * s);
    (2.0 * s3 - 3.0 * s2 + 1.0) * b[k]
        + (s3 - 2.0 * s2 + s) * h * m[k]
        + (-2.0 * s3 + 3.0 * s2) * b[k + 1]
        + (s3 - s2) * h * m[k + 1]
}

/// §3: Lambert-inverse tilt for linear brightness `g` under one directional light
/// (`Response::Lambert { ambient: 0 }`, irradiance 1). The M0 square-grid path.
#[inline]
pub fn tilt_from_brightness(g: f64, elevation: f64, max_tilt: f64) -> f64 {
    let g = if g.is_finite() { g.clamp(0.0, 1.0) } else { 0.0 };
    let e_min = (elevation - max_tilt).sin().max(0.0);
    let e_maxv = (elevation + max_tilt).sin();
    let e = e_min + g * (e_maxv - e_min);
    (e.clamp(-1.0, 1.0).asin() - elevation).clamp(-max_tilt, max_tilt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec2;

    fn cl(el_deg: f64, i: f64) -> CellLight {
        CellLight { a: DVec2::X, el: el_deg.to_radians(), irradiance: i }
    }

    #[test]
    fn lambert_endpoints() {
        let (a, s) = (5f64.to_radians(), 10f64.to_radians());
        assert!((tilt_from_brightness(1.0, a, s) - s).abs() < 1e-12);
        assert!((tilt_from_brightness(0.0, a, s) + a).abs() < 1e-12);
        assert_eq!(tilt_from_brightness(f64::NAN, a, s), tilt_from_brightness(0.0, a, s));
    }

    #[test]
    fn lambert_round_trip_and_ambient() {
        for &amb in &[0.0, 0.1, 0.5] {
            let c = Response::Lambert { ambient: amb }.prepare(0.0).unwrap();
            for &(el, i) in &[(5.0, 1.0), (30.0, 0.4), (80.0, 0.9)] {
                let l = cl(el, i);
                let r = c.cell_range(&l, 20f64.to_radians()).unwrap();
                assert!(r.tilt_hi <= std::f64::consts::FRAC_PI_2 - l.el + 1e-15);
                for k in 0..=20 {
                    let th = r.tilt_lo + (r.tilt_hi - r.tilt_lo) * k as f64 / 20.0;
                    if (l.el + th) <= 0.0 {
                        continue; // shadowed plateau
                    }
                    let b = c.forward(th, &l);
                    let back = c.inverse(b, &l, &r);
                    assert!((c.forward(back, &l) - b).abs() < 1e-12, "el {el} th {th}");
                    // asin is ill-conditioned only at the top (α + θ = 90°).
                    if l.el + th < 89f64.to_radians() {
                        assert!((back - th).abs() < 1e-9, "el {el} th {th}");
                    }
                }
            }
        }
    }

    #[test]
    fn pav_is_monotone_least_squares() {
        assert_eq!(pav(&[1.0, 3.0, 2.0, 4.0]), vec![1.0, 2.5, 2.5, 4.0]);
        assert_eq!(pav(&[3.0, 2.0, 1.0]), vec![2.0, 2.0, 2.0]);
    }

    #[test]
    fn measured_table_monotone_and_trimmed() {
        // Rises, then falls past the top (a real Lambert curve beyond 90° would).
        let t: Vec<f64> = [-20.0, -10.0, 0.0, 10.0, 20.0, 25.0].iter().map(|d: &f64| d.to_radians()).collect();
        let b = vec![0.05, 0.05, 0.2, 0.5, 0.9, 0.8];
        let c = Response::Measured { tilt_rad: t.clone(), brightness: b }.prepare(10f64.to_radians()).unwrap();
        let l = cl(10.0, 1.0);
        let r = c.cell_range(&l, 30f64.to_radians()).unwrap();
        // Trimmed to [−10°, 20°]: bottom plateau end … first max.
        assert!((r.tilt_lo - (-10f64).to_radians()).abs() < 1e-12);
        assert!(r.b_hi > 0.8 && r.b_lo < 0.1);
        let mut prev = -1.0;
        for k in 0..=200 {
            let th = r.tilt_lo + (r.tilt_hi - r.tilt_lo) * k as f64 / 200.0;
            let v = c.forward(th, &l);
            assert!(v >= prev - 1e-12, "not monotone at {th}");
            prev = v;
            let back = c.inverse(v, &l, &r);
            assert!((c.forward(back, &l) - v).abs() < 1e-9);
        }
        // Interpolates the data.
        assert!((c.forward(0.0, &l) - 0.2).abs() < 1e-12);
        // Elevation shift: a facet lit 5° higher reads the table 5° further on.
        let l2 = cl(15.0, 1.0);
        assert!((c.forward(0.0, &l2) - c.forward(5f64.to_radians(), &l)).abs() < 1e-12);
        // Errors.
        assert!(Response::Measured { tilt_rad: vec![0.0, 0.0], brightness: vec![0.0, 1.0] }.prepare(0.0).is_err());
        assert!(Response::Measured { tilt_rad: vec![0.0, 1.0], brightness: vec![0.5, 0.5] }.prepare(0.0).is_err());
        assert!(Response::Measured { tilt_rad: vec![0.0], brightness: vec![0.5] }.prepare(0.0).is_err());
        assert!(Response::Lambert { ambient: -0.1 }.prepare(0.0).is_err());
    }
}
