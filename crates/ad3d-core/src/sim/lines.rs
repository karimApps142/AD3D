//! Light lines over a partition and the exact horizon sweep (design §15.1).
//!
//! A [`LineSet`] cuts the plate into lines that run the way the light travels:
//! parallel lines for a directional light, rays fanning out from the lamp's
//! footprint for a point light. Each line is split into [`Seg`]s, one per piece
//! it crosses, sorted from the light outward. Along a line every facet is a
//! linear function of the line parameter `s`, so the relief seen by the line is
//! piecewise linear and the shadow test is exact:
//!
//! - **Parallel** (directional, elevation α): a point at `s` is lit iff
//!   `w(s) + s·tan α ≥ max_{s' ≤ s} (w(s') + s'·tan α)`, a running maximum. It
//!   never divides by `tan α`.
//! - **Radial** (point light at height `H` above the lamp footprint, `s = r`):
//!   lit iff `(H − w(r)) / r ≤ min_{r' ≤ r} (H − w(r')) / r'`, a running minimum.
//!
//! Within one segment the lit set is a single interval `[s_lit, s1]` (or empty),
//! found in closed form. Only the direction across the lines is sampled: lines
//! are `pitch` apart and each stands for a strip `pitch` wide (or an angular
//! wedge for radial lines). For a directional light along a square-grid axis and
//! facets whose gradient is parallel to the light, the relief is constant across
//! each row of cells, so one line per row is already exact.

use std::ops::Range;

use glam::DVec2;

use crate::relief::Partition;

/// Height tolerance (mm) for "lit": a surface that continues exactly at the
/// horizon height (no step) counts as lit.
pub const LIT_EPS: f64 = 1e-9;

/// The sweep rule of a line set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sweep {
    /// Directional light: running max of `w + s·tan α`.
    Parallel { tan_el: f64 },
    /// Point light at `height` above the plate plane: running min of `(H − w)/r`.
    Radial { height: f64 },
}

impl Sweep {
    /// Horizon state before anything has been seen.
    #[inline]
    pub fn init(&self) -> f64 {
        match self {
            Sweep::Parallel { .. } => f64::NEG_INFINITY,
            Sweep::Radial { .. } => f64::INFINITY,
        }
    }

    /// One segment `[s0, s1]` with heights `w0`, `w1` at its ends. Returns the start
    /// of the lit part (`[s_lit, s1]` is lit), or `None` when no part of positive
    /// length is lit, and advances the horizon `state`.
    #[inline]
    pub fn seg(&self, s0: f64, s1: f64, w0: f64, w1: f64, state: &mut f64) -> Option<f64> {
        match *self {
            Sweep::Parallel { tan_el } => {
                let u0 = w0 + s0 * tan_el;
                let u1 = w1 + s1 * tan_el;
                let k = *state;
                let lit = if u1 >= u0 {
                    if u0 >= k - LIT_EPS {
                        Some(s0)
                    } else if u1 < k - LIT_EPS {
                        None
                    } else {
                        // u0 < k ≤ u1 + eps, so u1 > u0.
                        let t = ((k - u0) / (u1 - u0)).clamp(0.0, 1.0);
                        Some(s0 + t * (s1 - s0))
                    }
                } else {
                    // Faces away from the light by more than grazing: only its
                    // first point can be lit, and its irradiance is 0 anyway.
                    None
                };
                *state = k.max(u0).max(u1);
                lit
            }
            Sweep::Radial { height } => {
                let g0 = height - w0;
                let g1 = height - w1;
                let m0 = if s0 > 0.0 { g0 / s0 } else { f64::INFINITY };
                let m1 = g1 / s1;
                let m_in = *state;
                // Decreasing m ⇔ the facet faces the lamp (for g > 0).
                let decreasing = g0 > 0.0 && g1 > 0.0 && g1 * s0 <= g0 * s1;
                let lit = if !decreasing {
                    None
                } else if m_in.is_infinite() {
                    Some(s0)
                } else {
                    // Lit where f(r) = g(r) − m_in·r ≤ eps; f is linear in r and
                    // decreasing wherever it is ≤ 0.
                    let f0 = g0 - m_in * s0;
                    let f1 = g1 - m_in * s1;
                    if f0 <= LIT_EPS {
                        Some(s0)
                    } else if f1 > LIT_EPS {
                        None
                    } else {
                        let t = (f0 / (f0 - f1)).clamp(0.0, 1.0);
                        Some(s0 + t * (s1 - s0))
                    }
                };
                *state = m_in.min(m0).min(m1);
                lit
            }
        }
    }

    /// Plate-plane area represented by the part `[a, b]` of a line with `weight`.
    #[inline]
    pub fn measure(&self, weight: f64, a: f64, b: f64) -> f64 {
        match self {
            Sweep::Parallel { .. } => weight * (b - a),
            Sweep::Radial { .. } => weight * 0.5 * (b * b - a * a),
        }
    }
}

/// One line: `p(s) = o + s·d`.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub o: DVec2,
    pub d: DVec2,
    /// Strip width (parallel) or wedge angle (radial) this line stands for.
    pub weight: f64,
    pub segs: Range<u32>,
}

/// The part of one line inside one piece.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seg {
    pub line: u32,
    pub piece: u32,
    pub facet: u32,
    pub s0: f64,
    pub s1: f64,
    pub p0: DVec2,
    pub p1: DVec2,
    /// Plate-plane area this segment stands for.
    pub meas: f64,
}

/// Lines and segments over a partition, plus per-facet indexes.
#[derive(Debug, Clone, PartialEq)]
pub struct LineSet {
    pub sweep: Sweep,
    pub lines: Vec<Line>,
    /// Line-major, sorted by `s0` within each line.
    pub segs: Vec<Seg>,
    /// CSR: segments of facet `f` are `facet_seg_idx[facet_seg_off[f]..facet_seg_off[f+1]]`,
    /// sorted by (line, s0).
    pub facet_seg_off: Vec<u32>,
    pub facet_seg_idx: Vec<u32>,
    /// Total sampled area per facet (`Σ seg.meas`).
    pub facet_meas: Vec<f64>,
    /// Spacing across lines (mm; at the farthest point for radial lines).
    pub pitch: f64,
}

/// How the lines are laid out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineGeom {
    /// Lines along unit vector `d` (direction of light travel).
    Parallel { d: DVec2, tan_el: f64 },
    /// Rays from the lamp footprint `origin`, lamp `height` above the plate plane.
    Radial { origin: DVec2, height: f64 },
}

/// Mean cell size `sqrt(outline area / pieces)`.
pub fn mean_cell(p: &Partition) -> f64 {
    (p.outline_area().abs() / p.pieces.len().max(1) as f64).sqrt()
}

fn ring_pts(p: &Partition, piece: usize) -> impl Iterator<Item = DVec2> + Clone + '_ {
    let r = &p.pieces[piece].ring;
    p.rings[r.start as usize..r.end as usize].iter().map(move |&v| p.verts[v as usize])
}

/// Clips the line `o + s·d` (normal `n`) against a convex polygon. Returns `[s0, s1]`.
///
/// Half-open rule: a vertex on the line counts as being on the `+n` side. A line
/// that runs exactly along an edge shared by two pieces therefore belongs to
/// exactly one of them (the piece on the `−n` side), so no area is counted twice
/// and no line sees two overlapping segments.
fn clip(pts: impl Iterator<Item = DVec2> + Clone, o: DVec2, d: DVec2, n: DVec2) -> Option<(f64, f64)> {
    let v: Vec<(f64, f64)> = pts.map(|q| ((q - o).dot(n), (q - o).dot(d))).collect();
    let k = v.len();
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for i in 0..k {
        let (ti, si) = v[i];
        let (tj, sj) = v[(i + 1) % k];
        if (ti >= 0.0) != (tj >= 0.0) {
            // One side is < 0 and the other ≥ 0, so tj ≠ ti.
            let s = if ti == 0.0 {
                si
            } else if tj == 0.0 {
                sj
            } else {
                si + (0.0 - ti) / (tj - ti) * (sj - si)
            };
            lo = lo.min(s);
            hi = hi.max(s);
        }
    }
    (hi - lo > 1e-12).then_some((lo, hi))
}

fn point_in_convex(pts: impl Iterator<Item = DVec2> + Clone, q: DVec2) -> bool {
    let v: Vec<DVec2> = pts.collect();
    let k = v.len();
    (0..k).all(|i| (v[(i + 1) % k] - v[i]).perp_dot(q - v[i]) >= 0.0)
}

fn point_in_polygon(v: &[DVec2], q: DVec2) -> bool {
    let mut inside = false;
    let k = v.len();
    for i in 0..k {
        let (a, b) = (v[i], v[(i + 1) % k]);
        if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

impl LineSet {
    /// Builds the lines for `geom` with spacing `pitch` (mm) across them.
    /// `n_facets` sizes the per-facet indexes.
    pub fn build(p: &Partition, n_facets: usize, geom: LineGeom, pitch: f64) -> LineSet {
        let outline: Vec<DVec2> = p.outline.iter().map(|&v| p.verts[v as usize]).collect();
        let mut raw: Vec<(u32, f64, f64, u32)> = Vec::new();
        let (lines, sweep, pitch_out) = match geom {
            LineGeom::Parallel { d, tan_el } => {
                let n = d.perp();
                let (tmin, tmax) =
                    outline.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| (a.min(q.dot(n)), b.max(q.dot(n))));
                let count = (((tmax - tmin) / pitch) * (1.0 - 1e-9)).ceil().max(1.0) as u32;
                let pitch = (tmax - tmin) / count as f64;
                let t_of = |k: u32| tmin + (k as f64 + 0.5) * pitch;
                for (pi, _) in p.pieces.iter().enumerate() {
                    let pts = ring_pts(p, pi);
                    let (a, b) = pts.clone().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| (a.min(q.dot(n)), b.max(q.dot(n))));
                    // One extra line each side: `clip` decides exactly (the
                    // pre-filter only saves work and must not drop a line).
                    let k0 = (((a - tmin) / pitch - 0.5).ceil() as i64 - 1).max(0);
                    let k1 = (((b - tmin) / pitch - 0.5).floor() as i64 + 1).min(count as i64 - 1);
                    for k in k0..=k1 {
                        let o = n * t_of(k as u32);
                        if let Some((s0, s1)) = clip(pts.clone(), o, d, n) {
                            raw.push((k as u32, s0, s1, pi as u32));
                        }
                    }
                }
                let lines: Vec<Line> =
                    (0..count).map(|k| Line { o: n * t_of(k), d, weight: pitch, segs: 0..0 }).collect();
                (lines, Sweep::Parallel { tan_el }, pitch)
            }
            LineGeom::Radial { origin, height } => {
                let r_max = outline.iter().map(|q| (*q - origin).length()).fold(0.0, f64::max).max(1e-9);
                let inside = point_in_polygon(&outline, origin);
                let tau = std::f64::consts::TAU;
                // Angular range: the full circle, or the outline's span around the
                // direction to its centroid (no wrap for a convex outline).
                let (phi0, span) = if inside {
                    (0.0, tau)
                } else {
                    let c = outline.iter().copied().sum::<DVec2>() / outline.len() as f64 - origin;
                    let base = c.y.atan2(c.x);
                    let rel = |q: DVec2| {
                        let v = q - origin;
                        let (cs, sn) = (base.cos(), base.sin());
                        (v.y * cs - v.x * sn).atan2(v.x * cs + v.y * sn)
                    };
                    let (lo, hi) = outline.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| {
                        let r = rel(*q);
                        (a.min(r), b.max(r))
                    });
                    (base + lo, (hi - lo).max(1e-9))
                };
                let dphi_want = pitch / r_max;
                let count = ((span / dphi_want) * (1.0 - 1e-9)).ceil().max(1.0) as u32;
                let dphi = span / count as f64;
                let phi_of = |k: u32| phi0 + (k as f64 + 0.5) * dphi;
                for (pi, _) in p.pieces.iter().enumerate() {
                    let pts = ring_pts(p, pi);
                    let mut ks: Vec<u32> = Vec::new();
                    if point_in_convex(pts.clone(), origin) {
                        ks.extend(0..count);
                    } else {
                        let c = pts.clone().sum::<DVec2>() / (p.pieces[pi].ring.len() as f64) - origin;
                        let base = c.y.atan2(c.x);
                        let (cs, sn) = (base.cos(), base.sin());
                        let (lo, hi) = pts.clone().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| {
                            let v = q - origin;
                            let r = (v.y * cs - v.x * sn).atan2(v.x * cs + v.y * sn);
                            (a.min(r), b.max(r))
                        });
                        // Piece centre direction in the outline's angular frame.
                        let mut c_rel = (base - phi0).rem_euclid(tau);
                        if !inside && c_rel > span + 1.0 {
                            c_rel -= tau;
                        }
                        let (alo, ahi) = (c_rel + lo, c_rel + hi);
                        // Widened by one line each side; `clip` decides exactly.
                        let k0 = (alo / dphi - 0.5).ceil() as i64 - 1;
                        let k1 = (ahi / dphi - 0.5).floor() as i64 + 1;
                        for k in k0..=k1 {
                            let kk = if inside { k.rem_euclid(count as i64) } else { k };
                            if (0..count as i64).contains(&kk) {
                                ks.push(kk as u32);
                            }
                        }
                        ks.sort_unstable();
                        ks.dedup();
                    }
                    for k in ks {
                        let ph = phi_of(k);
                        let d = DVec2::new(ph.cos(), ph.sin());
                        if let Some((s0, s1)) = clip(pts.clone(), origin, d, d.perp()) {
                            let s0 = s0.max(0.0);
                            if s1 - s0 > 1e-12 {
                                raw.push((k, s0, s1, pi as u32));
                            }
                        }
                    }
                }
                let lines: Vec<Line> = (0..count)
                    .map(|k| {
                        let ph = phi_of(k);
                        Line { o: origin, d: DVec2::new(ph.cos(), ph.sin()), weight: dphi, segs: 0..0 }
                    })
                    .collect();
                (lines, Sweep::Radial { height }, dphi * r_max)
            }
        };
        Self::assemble(p, n_facets, lines, sweep, raw, pitch_out)
    }

    fn assemble(
        p: &Partition,
        n_facets: usize,
        mut lines: Vec<Line>,
        sweep: Sweep,
        mut raw: Vec<(u32, f64, f64, u32)>,
        pitch: f64,
    ) -> LineSet {
        raw.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.3.cmp(&b.3)));
        let mut segs = Vec::with_capacity(raw.len());
        let mut start = 0usize;
        for (li, line) in lines.iter_mut().enumerate() {
            while start < raw.len() && (raw[start].0 as usize) < li {
                start += 1;
            }
            let s = segs.len() as u32;
            let mut k = start;
            while k < raw.len() && raw[k].0 as usize == li {
                let (_, s0, s1, piece) = raw[k];
                segs.push(Seg {
                    line: li as u32,
                    piece,
                    facet: p.pieces[piece as usize].facet,
                    s0,
                    s1,
                    p0: line.o + line.d * s0,
                    p1: line.o + line.d * s1,
                    meas: sweep.measure(line.weight, s0, s1),
                });
                k += 1;
            }
            start = k;
            line.segs = s..segs.len() as u32;
        }
        let mut count = vec![0u32; n_facets + 1];
        let mut facet_meas = vec![0.0; n_facets];
        for s in &segs {
            count[s.facet as usize + 1] += 1;
            facet_meas[s.facet as usize] += s.meas;
        }
        for f in 0..n_facets {
            count[f + 1] += count[f];
        }
        let facet_seg_off = count.clone();
        let mut fill = count;
        let mut facet_seg_idx = vec![0u32; segs.len()];
        for (i, s) in segs.iter().enumerate() {
            let f = s.facet as usize;
            facet_seg_idx[fill[f] as usize] = i as u32;
            fill[f] += 1;
        }
        LineSet { sweep, lines, segs, facet_seg_off, facet_seg_idx, facet_meas, pitch }
    }

    /// Segment indexes of facet `f`, sorted by (line, s0).
    #[inline]
    pub fn facet_segs(&self, f: usize) -> &[u32] {
        &self.facet_seg_idx[self.facet_seg_off[f] as usize..self.facet_seg_off[f + 1] as usize]
    }

    /// Number of facets the indexes were built for.
    pub fn n_facets(&self) -> usize {
        self.facet_meas.len()
    }

    /// Index of the line nearest to plate point `q` (for rendering), if any.
    pub fn line_at(&self, q: DVec2) -> Option<usize> {
        let first = self.lines.first()?;
        let n = self.lines.len();
        match self.sweep {
            Sweep::Parallel { .. } => {
                let nrm = first.d.perp();
                let t0 = first.o.dot(nrm) - 0.5 * first.weight;
                let k = ((q.dot(nrm) - t0) / first.weight).floor();
                (k >= 0.0 && (k as usize) < n).then_some(k as usize)
            }
            Sweep::Radial { .. } => {
                let v = q - first.o;
                let ph0 = first.d.y.atan2(first.d.x) - 0.5 * first.weight;
                let ph = v.y.atan2(v.x);
                let mut rel = (ph - ph0).rem_euclid(std::f64::consts::TAU);
                if n as f64 * first.weight >= std::f64::consts::TAU - 1e-9 {
                    rel = rel.rem_euclid(n as f64 * first.weight);
                }
                let k = (rel / first.weight).floor();
                (k >= 0.0 && (k as usize) < n).then_some(k as usize)
            }
        }
    }

    /// Line parameter of plate point `q` on line `k`.
    #[inline]
    pub fn param_on(&self, k: usize, q: DVec2) -> f64 {
        match self.sweep {
            Sweep::Parallel { .. } => (q - self.lines[k].o).dot(self.lines[k].d),
            Sweep::Radial { .. } => (q - self.lines[k].o).length(),
        }
    }
}
