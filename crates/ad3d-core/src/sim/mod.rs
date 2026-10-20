//! Forward model of a shadow plate (design §15.1): what the relief looks like
//! from the front under the design light.
//!
//! Per facet: `B = shadowed + direct · lit_fraction`, where
//! - `direct` is the unshadowed lamp term, Lambert `I·max(0, n·L)` (or the
//!   measured response table, §15.2),
//! - `lit_fraction` is the share of the facet's front-view area that is not in a
//!   cast shadow, from the exact horizon sweep in [`lines`],
//! - `shadowed` is the ambient term (room light reaches shadowed parts too).
//!
//! Viewed from the front a matte (Lambertian) surface looks equally bright from
//! any direction, so brightness is averaged over plate-plane area; walls are
//! vertical and have no front-view area.
//!
//! Entry points: [`shade`] (per-facet brightness), [`match_score`] (Pearson r and
//! RMSE against a target) and [`render::render`] (a grayscale image).

pub mod lines;
pub mod render;

use glam::{DVec2, DVec3};
use rayon::prelude::*;
use serde::Serialize;

use crate::error::{Error, Result};
use crate::relief::{Facet, Partition, PartitionKind};
pub use lines::{LineGeom, LineSet, Seg, Sweep};

// `Light` and `Response` are the §12/§15.2/§15.4 contract types, defined once in
// `squama::{light, response}` and re-exported here. The forward-model methods on
// `Response` (`shadowed`, `direct`, `inverse`) live below.
pub use crate::squama::light::Light;
pub use crate::squama::response::Response;

impl Response {
    /// Brightness of a part in cast shadow.
    pub fn shadowed(&self) -> f64 {
        match self {
            Response::Lambert { ambient } => *ambient,
            Response::Measured { brightness, .. } => brightness.iter().copied().fold(f64::INFINITY, f64::min),
        }
    }

    pub(crate) fn check(&self) -> Result<()> {
        match self {
            Response::Lambert { ambient } => {
                if !ambient.is_finite() || *ambient < 0.0 {
                    return Err(Error::InvalidParams(format!("ambient {ambient} must be finite and ≥ 0")));
                }
            }
            Response::Measured { tilt_rad, brightness } => {
                if tilt_rad.len() < 2 || tilt_rad.len() != brightness.len() {
                    return Err(Error::InvalidParams("measured response needs ≥ 2 (tilt, brightness) pairs".into()));
                }
                if !tilt_rad.windows(2).all(|w| w[1] > w[0]) || !tilt_rad.iter().chain(brightness).all(|x| x.is_finite())
                {
                    return Err(Error::InvalidParams("measured response tilts must be finite and ascending".into()));
                }
            }
        }
        Ok(())
    }

    /// §15.2 `θ = resp.inverse(target)`: the tilt in `[−max_tilt, max_tilt]` whose
    /// unshadowed brightness reaches the absolute `target`, restricted to the
    /// response's monotone range (see [`invert_direct`]).
    pub fn inverse(&self, fl: &FacetLight, max_tilt: f64, target: f64) -> f64 {
        invert_direct(fl, max_tilt, self, target - self.shadowed())
    }

    /// Unshadowed lamp term (above `shadowed()`) for a facet whose normal makes
    /// `n·L = cos_inc` with a light of elevation `el` and irradiance `irr`.
    #[inline]
    pub fn direct(&self, cos_inc: f64, el: f64, irr: f64) -> f64 {
        match self {
            Response::Lambert { .. } => irr * cos_inc.max(0.0),
            Response::Measured { tilt_rad, brightness } => {
                let th = cos_inc.clamp(-1.0, 1.0).asin() - el;
                irr * (interp(tilt_rad, brightness, th) - self.shadowed()).max(0.0)
            }
        }
    }
}

fn interp(x: &[f64], y: &[f64], t: f64) -> f64 {
    let n = x.len();
    if t <= x[0] {
        return y[0];
    }
    if t >= x[n - 1] {
        return y[n - 1];
    }
    let k = x.partition_point(|&v| v <= t).clamp(1, n - 1);
    let u = (t - x[k - 1]) / (x[k] - x[k - 1]);
    y[k - 1] + u * (y[k] - y[k - 1])
}

/// The light as one facet sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FacetLight {
    /// Unit vector in the plate plane toward the light.
    pub a: DVec2,
    /// Elevation of the light above the plate plane, seen from the facet.
    pub el: f64,
    /// Irradiance relative to the plate centre (1 for a directional light).
    pub irr: f64,
}

impl FacetLight {
    /// `n·L` for a facet tilted by `theta` toward the light (gradient `−tan θ · a`).
    #[inline]
    pub fn cos_tilt(&self, theta: f64) -> f64 {
        (self.el + theta).sin()
    }

    /// `n·L` for an arbitrary facet plane.
    #[inline]
    pub fn cos_facet(&self, f: &Facet) -> f64 {
        let n = DVec3::new(-f.grad.x, -f.grad.y, 1.0).normalize();
        let l = DVec3::new(self.el.cos() * self.a.x, self.el.cos() * self.a.y, self.el.sin());
        n.dot(l)
    }
}

/// Snaps azimuths within 1e-12 rad of an axis to the exact axis vector.
pub fn azimuth_vec(az: f64) -> DVec2 {
    let q = az / std::f64::consts::FRAC_PI_2;
    if (q - q.round()).abs() < 1e-12 {
        match (q.round() as i64).rem_euclid(4) {
            0 => DVec2::X,
            1 => DVec2::Y,
            2 => -DVec2::X,
            _ => -DVec2::Y,
        }
    } else {
        DVec2::new(az.cos(), az.sin())
    }
}

/// Area centroid of the plate outline.
pub fn outline_centroid(p: &Partition) -> DVec2 {
    let v: Vec<DVec2> = p.outline.iter().map(|&i| p.verts[i as usize]).collect();
    let (mut a, mut c) = (0.0, DVec2::ZERO);
    for i in 0..v.len() {
        let (p0, p1) = (v[i], v[(i + 1) % v.len()]);
        let x = p0.perp_dot(p1);
        a += x;
        c += (p0 + p1) * x;
    }
    if a.abs() < 1e-300 { v.iter().copied().sum::<DVec2>() / v.len().max(1) as f64 } else { c / (3.0 * a) }
}

/// Per-facet light for facets centred at `pivot` with height `h`.
/// For a point light, irradiance is `(d_ref / d)²` with `d_ref` the lamp's
/// distance from the outline centroid at `w = 0`.
pub fn facet_lights(p: &Partition, centres: &[(DVec2, f64)], light: &Light) -> Result<Vec<FacetLight>> {
    match *light {
        Light::Directional { az, el } => {
            if !(az.is_finite() && el.is_finite()) || el <= 0.0 || el >= std::f64::consts::FRAC_PI_2 {
                return Err(Error::InvalidParams(format!("light elevation {el} rad must be in (0, π/2)")));
            }
            let a = azimuth_vec(az);
            Ok(vec![FacetLight { a, el, irr: 1.0 }; centres.len()])
        }
        Light::Point { pos_mm } => {
            if !pos_mm.is_finite() {
                return Err(Error::InvalidParams("non-finite lamp position".into()));
            }
            let c = outline_centroid(p);
            let d_ref = (pos_mm - DVec3::new(c.x, c.y, 0.0)).length();
            centres
                .iter()
                .map(|&(q, h)| {
                    let v = pos_mm - DVec3::new(q.x, q.y, h);
                    if v.z <= 0.0 {
                        return Err(Error::InvalidParams("lamp must be in front of the relief (w above every facet)".into()));
                    }
                    let d = v.length();
                    let horiz = DVec2::new(v.x, v.y);
                    let a = if horiz.length() > 1e-12 { horiz.normalize() } else { DVec2::X };
                    Ok(FacetLight { a, el: (v.z / d).asin(), irr: (d_ref / d).powi(2) })
                })
                .collect()
        }
    }
}

/// Number of facets a partition refers to.
pub fn facet_count(p: &Partition) -> usize {
    p.pieces.iter().map(|q| q.facet as usize + 1).max().unwrap_or(0)
}

/// Lines across the plate for `light`. `lines_per_cell = 0` picks automatically:
/// 1 for a square grid lit along an axis with every gradient parallel to the
/// light (exact), else 4.
pub fn line_set(p: &Partition, facets: &[Facet], light: &Light, lines_per_cell: u32) -> Result<LineSet> {
    let nf = facet_count(p);
    if facets.len() < nf {
        return Err(Error::InvalidParams(format!("{} facets for a partition that uses {nf}", facets.len())));
    }
    let cell = lines::mean_cell(p);
    let geom = match *light {
        Light::Directional { az, el } => {
            if !(el > 0.0 && el < std::f64::consts::FRAC_PI_2) {
                return Err(Error::InvalidParams(format!("light elevation {el} rad must be in (0, π/2)")));
            }
            LineGeom::Parallel { d: -azimuth_vec(az), tan_el: el.tan() }
        }
        Light::Point { pos_mm } => LineGeom::Radial { origin: DVec2::new(pos_mm.x, pos_mm.y), height: pos_mm.z },
    };
    let n = if lines_per_cell > 0 {
        lines_per_cell
    } else if exact_one_line(p, facets, light) {
        1
    } else {
        4
    };
    Ok(LineSet::build(p, nf, geom, cell / n as f64))
}

fn exact_one_line(p: &Partition, facets: &[Facet], light: &Light) -> bool {
    let Light::Directional { az, .. } = *light else { return false };
    let PartitionKind::SquareGrid { .. } = p.kind else { return false };
    let a = azimuth_vec(az);
    if !(a.x == 0.0 || a.y == 0.0) {
        return false;
    }
    facets.iter().all(|f| f.grad.perp_dot(a).abs() <= 1e-12 * (1.0 + f.grad.length()))
}

/// Result of [`shade_lines`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Shading {
    /// Perceived brightness per facet (same units as the response).
    pub brightness: Vec<f64>,
    /// Unshadowed share of each facet's area.
    pub lit_fraction: Vec<f64>,
    /// Start of the lit part of every segment (`+∞` when unlit).
    #[serde(skip)]
    pub lit_start: Vec<f64>,
}

/// Runs the horizon sweep on every line (parallel over lines). Returns the start
/// of the lit part of each segment, `+∞` when unlit.
pub fn sweep_all(ls: &LineSet, facets: &[Facet]) -> Vec<f64> {
    let per_line: Vec<Vec<f64>> = ls
        .lines
        .par_iter()
        .map(|line| {
            let mut st = ls.sweep.init();
            ls.segs[line.segs.start as usize..line.segs.end as usize]
                .iter()
                .map(|s| {
                    let f = &facets[s.facet as usize];
                    ls.sweep.seg(s.s0, s.s1, f.eval(s.p0), f.eval(s.p1), &mut st).unwrap_or(f64::INFINITY)
                })
                .collect()
        })
        .collect();
    per_line.concat()
}

/// Brightness per facet on a prepared line set.
pub fn shade_lines(ls: &LineSet, facets: &[Facet], lights: &[FacetLight], resp: &Response) -> Shading {
    let lit_start = sweep_all(ls, facets);
    let nf = ls.n_facets();
    let mut lit = vec![0.0; nf];
    for (s, &a) in ls.segs.iter().zip(&lit_start) {
        if a.is_finite() {
            lit[s.facet as usize] += ls.sweep.measure(ls.lines[s.line as usize].weight, a, s.s1);
        }
    }
    let sh = resp.shadowed();
    let mut brightness = Vec::with_capacity(nf);
    let mut lit_fraction = Vec::with_capacity(nf);
    for f in 0..nf {
        let m = ls.facet_meas[f];
        let frac = if m > 0.0 { (lit[f] / m).clamp(0.0, 1.0) } else { 0.0 };
        let fl = &lights[f];
        let direct = resp.direct(fl.cos_facet(&facets[f]), fl.el, fl.irr);
        brightness.push(sh + direct * frac);
        lit_fraction.push(frac);
    }
    Shading { brightness, lit_fraction, lit_start }
}

/// §15.1 forward model: perceived brightness per facet under `light`, with cast
/// shadows. `samples_per_piece` is the number of light lines per mean cell width
/// (0 = automatic, see [`line_set`]).
pub fn shade(p: &Partition, f: &[Facet], light: &Light, resp: &Response, samples_per_piece: u32) -> Result<Vec<f64>> {
    resp.check()?;
    let ls = line_set(p, f, light, samples_per_piece)?;
    let centres: Vec<(DVec2, f64)> = f.iter().map(|x| (x.pivot, x.h)).collect();
    let lights = facet_lights(p, &centres, light)?;
    Ok(shade_lines(&ls, f, &lights, resp).brightness)
}

/// The brightness range a target maps into (§15.2): Lambert
/// `[ambient + E_min, ambient + E_max]` for a directional light; for a point
/// light the range every facet can reach (§15.4); for a measured response the
/// table's range over `[−S, S]`.
pub fn target_range(lights: &[FacetLight], max_tilt: f64, resp: &Response) -> Result<(f64, f64)> {
    let sh = resp.shadowed();
    let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
    for fl in lights {
        let (dlo, dhi) = direct_range(fl, max_tilt, resp);
        lo = lo.max(dlo);
        hi = hi.min(dhi);
    }
    if lights.is_empty() {
        return Err(Error::InvalidParams("no facets".into()));
    }
    if !(hi > lo) {
        return Err(Error::InvalidParams(
            "no brightness range is reachable by every facet (lamp too close or too low)".into(),
        ));
    }
    Ok((sh + lo, sh + hi))
}

/// Unshadowed direct-term range of one facet over tilts `[−S, S]`.
pub fn direct_range(fl: &FacetLight, s: f64, resp: &Response) -> (f64, f64) {
    match resp {
        Response::Lambert { .. } => (fl.irr * (fl.el - s).sin().max(0.0), fl.irr * (fl.el + s).sin()),
        Response::Measured { .. } => {
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for k in 0..=256 {
                let th = -s + 2.0 * s * k as f64 / 256.0;
                let v = resp.direct(fl.cos_tilt(th), fl.el, fl.irr);
                lo = lo.min(v);
                hi = hi.max(v);
            }
            (lo, hi)
        }
    }
}

/// Inverse of the direct term on its monotone range: the smallest tilt in
/// `[−S, S]` whose (running-max) direct term reaches `want`.
pub fn invert_direct(fl: &FacetLight, s: f64, resp: &Response, want: f64) -> f64 {
    match resp {
        Response::Lambert { .. } => {
            let e = (want / fl.irr).clamp(-1.0, 1.0);
            (e.asin() - fl.el).clamp(-s, s)
        }
        Response::Measured { .. } => {
            let n = 1024;
            let mut best = f64::NEG_INFINITY;
            for k in 0..=n {
                let th = -s + 2.0 * s * k as f64 / n as f64;
                best = best.max(resp.direct(fl.cos_tilt(th), fl.el, fl.irr));
                if best >= want {
                    return th;
                }
            }
            s
        }
    }
}

/// How well a simulated appearance matches its target.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct MatchScore {
    /// Area-weighted Pearson correlation between brightness and target.
    pub pearson: f64,
    /// Area-weighted RMS of `brightness − target` (absolute brightness units).
    pub rmse: f64,
    /// `rmse` divided by the target range `hi − lo`.
    pub rmse_rel: f64,
    /// p99 / p1 of brightness (area-weighted percentiles); the visible contrast.
    pub contrast: f64,
}

/// Scores per-facet `brightness` against absolute `target` (both per facet) with
/// area `weight`s. `range` is the target range for `rmse_rel`.
pub fn match_score(brightness: &[f64], target: &[f64], weight: &[f64], range: (f64, f64)) -> MatchScore {
    let n = brightness.len().min(target.len()).min(weight.len());
    let wsum: f64 = (0..n).map(|i| weight[i]).sum();
    if n == 0 || wsum <= 0.0 {
        return MatchScore::default();
    }
    let mb = (0..n).map(|i| weight[i] * brightness[i]).sum::<f64>() / wsum;
    let mt = (0..n).map(|i| weight[i] * target[i]).sum::<f64>() / wsum;
    let (mut sbb, mut stt, mut sbt, mut se) = (0.0, 0.0, 0.0, 0.0);
    for i in 0..n {
        let (db, dt) = (brightness[i] - mb, target[i] - mt);
        sbb += weight[i] * db * db;
        stt += weight[i] * dt * dt;
        sbt += weight[i] * db * dt;
        se += weight[i] * (brightness[i] - target[i]).powi(2);
    }
    let pearson = if sbb > 0.0 && stt > 0.0 { sbt / (sbb * stt).sqrt() } else { 0.0 };
    let rmse = (se / wsum).sqrt();
    let span = range.1 - range.0;
    let mut idx: Vec<usize> = (0..n).filter(|&i| weight[i] > 0.0).collect();
    idx.sort_by(|&a, &b| brightness[a].total_cmp(&brightness[b]).then(a.cmp(&b)));
    let pct = |q: f64| {
        let mut acc = 0.0;
        for &i in &idx {
            acc += weight[i];
            if acc >= q * wsum {
                return brightness[i];
            }
        }
        brightness[*idx.last().unwrap()]
    };
    let (p1, p99) = (pct(0.01), pct(0.99));
    MatchScore {
        pearson,
        rmse,
        rmse_rel: if span > 0.0 { rmse / span } else { 0.0 },
        contrast: if p1 > 0.0 { p99 / p1 } else { f64::INFINITY },
    }
}

/// Absolute per-facet targets `lo + g·(hi − lo)` from linear brightness `g`
/// (non-finite → 0, clamped to [0, 1], as §3).
pub fn absolute_targets(g: &[f32], range: (f64, f64)) -> Vec<f64> {
    g.iter()
        .map(|&x| {
            let x = if x.is_finite() { (x as f64).clamp(0.0, 1.0) } else { 0.0 };
            range.0 + x * (range.1 - range.0)
        })
        .collect()
}

#[cfg(test)]
mod tests;
