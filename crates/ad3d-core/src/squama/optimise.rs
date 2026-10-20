//! Shadow-aware facet optimiser (design §15.1).
//!
//! The plain Lambert inverse (§3) picks each facet's tilt as if it stood alone.
//! On the real plate a facet's raised edge throws a shadow `step / tan α` long
//! onto the facets after it, so bright regions come out darker and patchy
//! (research: match 0.89 at α = 5°, S = 10°). This module chooses per-facet
//! tilt `θ_k` (and, when allowed, pivot height `h_k`) to minimise the
//! area-weighted squared error between the simulated appearance
//! ([`crate::sim`], cast shadows included) and the absolute target
//! `lo + g·(hi − lo)` of the response (§15.2), subject to
//! `T ≤ corner ≤ T + max_relief` at every facet corner (§4 general rule).
//!
//! Algorithm: block coordinate descent over facets, starting from the Lambert
//! inverse at the centre pivot.
//! - A move changes one facet's `(θ, h)`. Heights stay in `[floor, ceil]`, so no
//!   point more than `D = (ceil − floor) / tan α_min` upstream can shadow
//!   anything; a move is therefore scored exactly by re-sweeping only its own
//!   light lines from `D` before it to `D` after it.
//! - Per visit: a coarse tilt grid plus a shrinking local step, then height
//!   candidates for the best tilt, then a final tilt refinement. The best
//!   candidate is accepted only if it lowers the error, so the error never rises.
//! - Facets are visited in light order (upstream first). Facets that share no
//!   light line are independent: each group of connected lines (for a light
//!   along a square-grid axis, each row) is one task, run in parallel with
//!   rayon. When there are few such groups (any oblique light), each group is
//!   cut into strips along the light at least `3·extent + D` wide; even strips
//!   run in parallel, then odd strips, so no two concurrent tasks read or write
//!   the same facets.
//! - Each task reads a snapshot taken at the start of its phase plus its own
//!   updates, so the result does not depend on the thread count.
//! - `cancel` is checked once per sweep and per phase. The time budget is
//!   checked before each sweep, so the output is always the result of a whole
//!   number of sweeps (reported) and is bit-identical for the same sweep count.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use glam::DVec2;
use rayon::prelude::*;
use serde::Serialize;

use crate::cancel::CancelToken;
use crate::error::{Error, Result};
use crate::image::CellBrightness;
use crate::relief::{Facet, Partition, Tol, square_grid};
use crate::sim::{
    FacetLight, Light, LineSet, MatchScore, Response, Sweep, absolute_targets, facet_count, facet_lights,
    invert_direct, line_set, match_score, prepare_response, shade_lines, target_range, ResponseCurve,
};

/// Geometric limits on facets (§12), shared with the solver. For the optimiser,
/// `max_relief_mm: None` means the centre-pivot relief `2·e_max·tan S`, so the
/// plate is no thicker than the plain solve.
pub use super::solve::Limits;

/// Optimiser settings (§12.1 `optimiser: { kind: 'shadow', maxSweeps }`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OptimiserOpts {
    /// Coordinate-descent sweeps. 0 = return the Lambert inverse (Preview).
    pub max_sweeps: u32,
    /// Allow each facet its own pivot height within the relief budget.
    pub vary_height: bool,
    /// Do not start a new sweep after this much time.
    pub time_budget: Option<Duration>,
    /// Light lines per mean cell width; 0 = automatic (exact single line per row
    /// for a light along a square-grid axis, else 4).
    pub lines_per_cell: u32,
    /// The coarse tilt grid has `2·tilt_steps + 1` values over `[−S, S]`.
    pub tilt_steps: u32,
    /// Stop when a sweep lowers the error by less than this fraction.
    pub min_gain: f64,
}

impl Default for OptimiserOpts {
    fn default() -> Self {
        OptimiserOpts {
            max_sweeps: 8,
            vary_height: true,
            time_budget: Some(Duration::from_secs(30)),
            lines_per_cell: 0,
            tilt_steps: 8,
            min_gain: 1e-4,
        }
    }
}

/// Per-facet target brightness source: linear `g ∈ [0, 1]` per facet, indexed
/// like `Facet`s (the §16 `Target`, plain slices, `CellBrightness`).
pub trait FacetTargets {
    fn facet_g(&self) -> &[f32];
}

impl FacetTargets for [f32] {
    fn facet_g(&self) -> &[f32] {
        self
    }
}

impl FacetTargets for Vec<f32> {
    fn facet_g(&self) -> &[f32] {
        self
    }
}

impl FacetTargets for crate::image::Target {
    fn facet_g(&self) -> &[f32] {
        &self.g
    }
}

impl FacetTargets for CellBrightness {
    fn facet_g(&self) -> &[f32] {
        &self.g
    }
}

/// What an optimiser run did.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OptimiseReport {
    #[serde(skip)]
    pub facets: Vec<Facet>,
    /// Score of the starting facets (the plain Lambert inverse for [`optimise`]).
    pub before: MatchScore,
    pub after: MatchScore,
    /// Area-weighted squared error after 0, 1, 2, … sweeps.
    pub sse: Vec<f64>,
    pub sweeps: u32,
    pub moves: u64,
    /// Stopped because the time budget ran out.
    pub timed_out: bool,
    pub elapsed_ms: f64,
    /// Target brightness range `(lo, hi)`.
    pub target_range: (f64, f64),
    /// Allowed corner heights `(floor, ceil)` in mm.
    pub height_range: (f64, f64),
    /// Parallel tasks per phase (`[even, odd]`).
    pub tasks: [usize; 2],
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Param {
    th: f64,
    h: f64,
}

/// Area centroid of each facet's pieces (the §4 pivot for unclipped cells). For
/// hex cells clipped by the plate edge the §4 pivot is the full hexagon's
/// centre: `pipeline::solve_job` therefore starts the optimiser from the
/// solver's facets ([`optimise_from`]), whose pivots come from the layout.
pub fn facet_centroids(p: &Partition) -> Vec<DVec2> {
    let nf = facet_count(p);
    let mut a = vec![0.0; nf];
    let mut c = vec![DVec2::ZERO; nf];
    for piece in &p.pieces {
        let r = &p.rings[piece.ring.start as usize..piece.ring.end as usize];
        let f = piece.facet as usize;
        for k in 0..r.len() {
            let (p0, p1) = (p.verts[r[k] as usize], p.verts[r[(k + 1) % r.len()] as usize]);
            let x = p0.perp_dot(p1);
            a[f] += x;
            c[f] += (p0 + p1) * x;
        }
    }
    (0..nf).map(|f| if a[f].abs() > 0.0 { c[f] / (3.0 * a[f]) } else { DVec2::ZERO }).collect()
}

/// `min/max a·(corner − pivot)` over all corners of each facet.
fn corner_extents(p: &Partition, pivots: &[DVec2], a: &[DVec2]) -> Vec<(f64, f64)> {
    let mut e = vec![(f64::INFINITY, f64::NEG_INFINITY); pivots.len()];
    for piece in &p.pieces {
        let f = piece.facet as usize;
        for &v in &p.rings[piece.ring.start as usize..piece.ring.end as usize] {
            let x = a[f].dot(p.verts[v as usize] - pivots[f]);
            e[f].0 = e[f].0.min(x);
            e[f].1 = e[f].1.max(x);
        }
    }
    for x in &mut e {
        if !x.0.is_finite() {
            *x = (0.0, 0.0);
        }
    }
    e
}

/// Light direction per facet from the pivot alone (horizontal part only).
fn horizontal_dirs(pivots: &[DVec2], light: &Light) -> Vec<DVec2> {
    match *light {
        Light::Directional { az, .. } => vec![crate::sim::azimuth_vec(az); pivots.len()],
        Light::Point { pos_mm } => pivots
            .iter()
            .map(|q| {
                let v = DVec2::new(pos_mm.x, pos_mm.y) - *q;
                if v.length() > 1e-12 { v.normalize() } else { DVec2::X }
            })
            .collect(),
    }
}

struct Geometry {
    pivots: Vec<DVec2>,
    a: Vec<DVec2>,
    ext: Vec<(f64, f64)>,
    lights: Vec<FacetLight>,
    floor: f64,
    ceil: f64,
    /// Centre-pivot common height.
    c_common: f64,
    s: f64,
}

fn check_limits(lim: &Limits) -> Result<()> {
    if !(lim.max_tilt.is_finite() && lim.max_tilt > 0.0 && lim.max_tilt < 1.2) {
        return Err(Error::InvalidParams(format!("max tilt {} rad out of range", lim.max_tilt)));
    }
    if !(lim.base_mm.is_finite() && lim.base_mm >= 0.0) {
        return Err(Error::InvalidParams(format!("base {} mm must be ≥ 0", lim.base_mm)));
    }
    if let Some(r) = lim.max_relief_mm {
        if !(r.is_finite() && r > 0.0) {
            return Err(Error::InvalidParams(format!("max relief {r} mm must be > 0")));
        }
    }
    Ok(())
}

fn geometry(p: &Partition, pivots: Vec<DVec2>, light: &Light, lim: &Limits) -> Result<Geometry> {
    check_limits(lim)?;
    let q = Tol::default().q;
    let a = horizontal_dirs(&pivots, light);
    let ext = corner_extents(p, &pivots, &a);
    let e_max = ext.iter().map(|e| e.0.abs().max(e.1.abs())).fold(0.0, f64::max);
    let s = lim.max_tilt;
    // `+ q` keeps every corner ≥ T after rounding to µm (§4).
    let floor = lim.base_mm + q;
    let ceil = match lim.max_relief_mm {
        Some(r) => lim.base_mm + r,
        None => floor + 2.0 * e_max * s.tan(),
    };
    if ceil <= floor {
        return Err(Error::InvalidParams(format!("max relief must exceed {q} mm")));
    }
    let c_common = (floor + e_max * s.tan()).min(0.5 * (floor + ceil)).max(floor);
    let centres: Vec<(DVec2, f64)> = pivots.iter().map(|&q| (q, c_common)).collect();
    let lights = facet_lights(p, &centres, light)?;
    Ok(Geometry { pivots, a, ext, lights, floor, ceil, c_common, s })
}

impl Geometry {
    /// Feasible pivot heights for tilt `th`, or `None`.
    #[inline]
    fn h_range(&self, f: usize, th: f64) -> Option<(f64, f64)> {
        let t = th.tan();
        let (e_lo, e_hi) = self.ext[f];
        let (o1, o2) = (-t * e_hi, -t * e_lo);
        let lo = self.floor - o1.min(o2);
        let hi = self.ceil - o1.max(o2);
        (lo <= hi + 1e-12).then_some((lo, hi.max(lo)))
    }

    /// Largest feasible |θ|.
    fn th_cap(&self, f: usize) -> f64 {
        let (e_lo, e_hi) = self.ext[f];
        let span = e_hi - e_lo;
        if span <= 0.0 { self.s } else { ((self.ceil - self.floor) / span).atan().min(self.s) }
    }

    fn facet(&self, f: usize, p: Param) -> Facet {
        Facet { pivot: self.pivots[f], h: p.h, grad: -p.th.tan() * self.a[f] }
    }

    /// Moves a parameter into the feasible set (and onto the common height when
    /// heights may not vary).
    fn project(&self, f: usize, p: Param, vary_height: bool) -> Param {
        let cap = self.th_cap(f);
        let th = p.th.clamp(-cap, cap);
        let (lo, hi) = self.h_range(f, th).unwrap_or((self.floor, self.floor));
        let h = if vary_height { p.h.clamp(lo, hi) } else { self.c_common.clamp(lo, hi) };
        Param { th, h }
    }
}

/// The Lambert-inverse facets at the centre pivot (§3, §4; point light §15.4)
/// for targets `g`, with pivots at the facet centroids.
pub fn lambert_facets(
    t: &(impl FacetTargets + ?Sized),
    p: &Partition,
    light: &Light,
    lim: &Limits,
    resp: &Response,
) -> Result<Vec<Facet>> {
    resp.check()?;
    let g = t.facet_g();
    let nf = facet_count(p);
    if g.len() < nf {
        return Err(Error::InvalidParams(format!("{} targets for {nf} facets", g.len())));
    }
    let geo = geometry(p, facet_centroids(p), light, lim)?;
    let curve = prepare_response(p, light, resp, lim.base_mm)?;
    let range = target_range(&geo.lights, geo.s, &curve)?;
    let tg = absolute_targets(&g[..nf], range);
    let sh = curve.shadowed();
    Ok((0..nf)
        .map(|f| {
            let th = invert_direct(&geo.lights[f], geo.s, &curve, tg[f] - sh);
            let pr = geo.project(f, Param { th, h: geo.c_common }, false);
            geo.facet(f, pr)
        })
        .collect())
}

/// §15.1 contract: Lambert inverse, then the shadow-aware optimiser.
pub fn optimise(
    t: &(impl FacetTargets + ?Sized),
    p: &Partition,
    light: &Light,
    lim: &Limits,
    resp: &Response,
    opt: &OptimiserOpts,
    cancel: &CancelToken,
) -> Result<Vec<Facet>> {
    optimise_report(t, p, light, lim, resp, opt, cancel).map(|r| r.facets)
}

/// [`optimise`] with scores and statistics.
pub fn optimise_report(
    t: &(impl FacetTargets + ?Sized),
    p: &Partition,
    light: &Light,
    lim: &Limits,
    resp: &Response,
    opt: &OptimiserOpts,
    cancel: &CancelToken,
) -> Result<OptimiseReport> {
    let init = lambert_facets(t, p, light, lim, resp)?;
    optimise_from(t, p, &init, light, lim, resp, opt, cancel)
}

/// Optimises starting from `init` (pivots are kept; each facet's tilt is read
/// along the light and its gradient reset to be parallel to the light).
#[allow(clippy::too_many_arguments)]
pub fn optimise_from(
    t: &(impl FacetTargets + ?Sized),
    p: &Partition,
    init: &[Facet],
    light: &Light,
    lim: &Limits,
    resp: &Response,
    opt: &OptimiserOpts,
    cancel: &CancelToken,
) -> Result<OptimiseReport> {
    let t0 = Instant::now();
    resp.check()?;
    let g = t.facet_g();
    let nf = facet_count(p);
    if g.len() < nf || init.len() < nf {
        return Err(Error::InvalidParams(format!("{} targets / {} facets for {nf} facets", g.len(), init.len())));
    }
    let geo = geometry(p, init[..nf].iter().map(|f| f.pivot).collect(), light, lim)?;
    // The same prepared curve (reference elevation at the plate centre, base
    // height) as `solve_facets_ex`, so the optimiser aims at the solver's response.
    let curve = prepare_response(p, light, resp, lim.base_mm)?;
    let resp = &curve;
    let range = target_range(&geo.lights, geo.s, resp)?;
    let target = absolute_targets(&g[..nf], range);
    let mut params: Vec<Param> = (0..nf)
        .map(|f| {
            let th = (-init[f].grad.dot(geo.a[f])).atan();
            geo.project(f, Param { th, h: init[f].h }, opt.vary_height)
        })
        .collect();
    let facets_of = |ps: &[Param]| -> Vec<Facet> { (0..nf).map(|f| geo.facet(f, ps[f])).collect() };
    let start = facets_of(&params);
    let ls = line_set(p, &start, light, opt.lines_per_cell)?;
    let tan_min = match ls.sweep {
        Sweep::Parallel { tan_el } => tan_el,
        Sweep::Radial { height } => {
            let o = ls.lines.first().map_or(DVec2::ZERO, |l| l.o);
            let r_max = p.outline.iter().map(|&v| (p.verts[v as usize] - o).length()).fold(1e-9, f64::max);
            (height - geo.ceil) / r_max
        }
    };
    if !(tan_min > 0.0) {
        return Err(Error::InvalidParams("lamp must be higher than the relief".into()));
    }
    let dr = (geo.ceil - geo.floor) / tan_min + 1e-6;
    let sse_of = |fs: &[Facet]| -> (f64, Vec<f64>) {
        let sh = shade_lines(&ls, fs, &geo.lights, resp);
        let e = (0..nf).map(|f| ls.facet_meas[f] * (sh.brightness[f] - target[f]).powi(2)).sum();
        (e, sh.brightness)
    };
    let (e0, b0) = sse_of(&start);
    let before = match_score(&b0, &target, &ls.facet_meas, range);
    let mut sse = vec![e0];

    let plan = plan_tasks(&ls, &geo, nf, dr);
    let ctx = Ctx::new(&ls, &geo, &target, resp, dr, opt.vary_height);
    let mut sweeps = 0;
    let mut moves = 0u64;
    let mut timed_out = false;
    let mut b_last = b0;
    for k in 0..opt.max_sweeps {
        cancel.check()?;
        if let Some(b) = opt.time_budget {
            if t0.elapsed() >= b {
                timed_out = true;
                break;
            }
        }
        for phase in &plan.phases {
            cancel.check()?;
            let snap = &params;
            let results: Vec<(Vec<(u32, Param)>, u64)> =
                phase.par_iter().map(|task| ctx.run_task(task, snap, &plan.owner, &plan.slot, k, opt)).collect();
            for (upd, m) in results {
                moves += m;
                for (f, pr) in upd {
                    params[f as usize] = pr;
                }
            }
        }
        sweeps += 1;
        let (e, b) = sse_of(&facets_of(&params));
        let prev = *sse.last().unwrap();
        sse.push(e);
        b_last = b;
        if prev - e <= opt.min_gain * prev {
            break;
        }
    }
    let facets = facets_of(&params);
    let after = match_score(&b_last, &target, &ls.facet_meas, range);
    Ok(OptimiseReport {
        facets,
        before,
        after,
        sse,
        sweeps,
        moves,
        timed_out,
        elapsed_ms: t0.elapsed().as_secs_f64() * 1e3,
        target_range: range,
        height_range: (geo.floor, geo.ceil),
        tasks: [plan.phases[0].len(), plan.phases[1].len()],
    })
}

/// Square-grid convenience for the M0 pipeline: the squama plate's partition,
/// directional light, Lambert response with `ambient`, and the default relief
/// budget. The app and exports reach the optimiser through `pipeline::solve_job`.
pub fn optimise_squama(
    sp: &super::SquamaParams,
    cb: &CellBrightness,
    ambient: f64,
    opt: &OptimiserOpts,
    cancel: &CancelToken,
) -> Result<(Partition, OptimiseReport)> {
    let cell = (sp.cell_mm * 1000.0).round() / 1000.0;
    if cb.nx == 0 || cb.ny == 0 || cb.g.len() != (cb.nx * cb.ny) as usize || !(cell > 0.0) {
        return Err(Error::InvalidParams("bad brightness grid or cell".into()));
    }
    let part = square_grid(cb.nx, cb.ny, cell, DVec2::ZERO);
    let light = Light::Directional { az: sp.light_azimuth, el: sp.light_elevation.max(1f64.to_radians()) };
    let lim = Limits { max_tilt: sp.max_tilt, base_mm: sp.base_mm, max_relief_mm: None };
    let r = optimise_report(cb, &part, &light, &lim, &Response::Lambert { ambient }, opt, cancel)?;
    Ok((part, r))
}

// ---------------------------------------------------------------------------
// Task planning

struct Plan {
    /// `phases[0]` run in parallel, then `phases[1]`.
    phases: [Vec<Vec<u32>>; 2],
    owner: Vec<u32>,
    slot: Vec<u32>,
}

fn find(uf: &mut [u32], mut x: u32) -> u32 {
    while uf[x as usize] != x {
        uf[x as usize] = uf[uf[x as usize] as usize];
        x = uf[x as usize];
    }
    x
}

/// Light-order coordinate of a facet (its pivot along the light).
fn order_coord(ls: &LineSet, geo: &Geometry, f: usize) -> f64 {
    let q = geo.pivots[f];
    match ls.sweep {
        Sweep::Parallel { .. } => q.dot(ls.lines[0].d),
        Sweep::Radial { .. } => (q - ls.lines[0].o).length(),
    }
}

fn plan_tasks(ls: &LineSet, geo: &Geometry, nf: usize, dr: f64) -> Plan {
    let mut uf: Vec<u32> = (0..ls.lines.len() as u32).collect();
    let mut ext = 0.0f64;
    for f in 0..nf {
        let segs = ls.facet_segs(f);
        if segs.is_empty() {
            continue;
        }
        let l0 = ls.segs[segs[0] as usize].line;
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for &i in segs {
            let s = &ls.segs[i as usize];
            let (a, b) = (find(&mut uf, l0), find(&mut uf, s.line));
            if a != b {
                uf[a.max(b) as usize] = a.min(b);
            }
            lo = lo.min(s.s0);
            hi = hi.max(s.s1);
        }
        ext = ext.max(hi - lo);
    }
    let comp_of = |uf: &mut [u32], f: usize| -> Option<u32> {
        let segs = ls.facet_segs(f);
        segs.first().map(|&i| find(uf, ls.segs[i as usize].line))
    };
    let mut roots: Vec<u32> = (0..nf).filter_map(|f| comp_of(&mut uf, f)).collect();
    roots.sort_unstable();
    roots.dedup();
    let many = roots.len() >= 8;
    let width = 3.0 * ext + dr + 1e-6;
    let cmin = (0..nf).map(|f| order_coord(ls, geo, f)).fold(f64::INFINITY, f64::min);
    let mut groups: BTreeMap<(u32, i64), Vec<(f64, u32)>> = BTreeMap::new();
    for f in 0..nf {
        let Some(root) = comp_of(&mut uf, f) else { continue };
        let c = order_coord(ls, geo, f);
        let strip = if many { 0 } else { ((c - cmin) / width).floor() as i64 };
        groups.entry((root, strip)).or_default().push((c, f as u32));
    }
    let mut phases: [Vec<Vec<u32>>; 2] = [Vec::new(), Vec::new()];
    let mut owner = vec![u32::MAX; nf];
    let mut slot = vec![u32::MAX; nf];
    let mut tid = 0u32;
    for ((_, strip), mut v) in groups {
        v.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let list: Vec<u32> = v.into_iter().map(|x| x.1).collect();
        for (k, &f) in list.iter().enumerate() {
            owner[f as usize] = tid;
            slot[f as usize] = k as u32;
        }
        phases[(strip.rem_euclid(2)) as usize].push(list);
        tid += 1;
    }
    Plan { phases, owner, slot }
}

// ---------------------------------------------------------------------------
// Local scoring

struct Ctx<'a> {
    ls: &'a LineSet,
    geo: &'a Geometry,
    target: &'a [f64],
    resp: &'a ResponseCurve,
    shadowed: f64,
    dr: f64,
    vary_height: bool,
    /// `a_f · (p − pivot_f)` at both ends of every segment.
    x: Vec<(f64, f64)>,
}

/// A task's view: the phase snapshot plus its own updates.
struct View<'a> {
    snap: &'a [Param],
    owner: &'a [u32],
    slot: &'a [u32],
    task: u32,
    local: Vec<Param>,
}

impl View<'_> {
    #[inline]
    fn get(&self, f: usize) -> Param {
        if self.owner[f] == self.task { self.local[self.slot[f] as usize] } else { self.snap[f] }
    }
}

struct Win {
    first: usize,
    end: usize,
    state: f64,
}

impl<'a> Ctx<'a> {
    fn new(ls: &'a LineSet, geo: &'a Geometry, target: &'a [f64], resp: &'a ResponseCurve, dr: f64, vary_height: bool) -> Self {
        let x = ls
            .segs
            .iter()
            .map(|s| {
                let f = s.facet as usize;
                (geo.a[f].dot(s.p0 - geo.pivots[f]), geo.a[f].dot(s.p1 - geo.pivots[f]))
            })
            .collect();
        Ctx { ls, geo, target, resp, shadowed: resp.shadowed(), dr, vary_height, x }
    }

    #[inline]
    fn direct(&self, f: usize, th: f64) -> f64 {
        let fl = &self.geo.lights[f];
        self.resp.direct(fl.cos_tilt(th), fl.irr)
    }

    /// Lit measure of segment `i` given the horizon `state` (advanced).
    #[inline]
    fn seg_lit(&self, i: usize, pr: Param, state: &mut f64) -> f64 {
        let s = &self.ls.segs[i];
        let t = pr.th.tan();
        let (x0, x1) = self.x[i];
        let (w0, w1) = (pr.h - t * x0, pr.h - t * x1);
        match self.ls.sweep.seg(s.s0, s.s1, w0, w1, state) {
            Some(a) => self.ls.sweep.measure(self.ls.lines[s.line as usize].weight, a, s.s1),
            None => 0.0,
        }
    }

    #[inline]
    fn param(&self, v: &View, i: usize, ovr: Option<(usize, Param)>) -> Param {
        let f = self.ls.segs[i].facet as usize;
        match ovr {
            Some((g, p)) if g == f => p,
            _ => v.get(f),
        }
    }

    /// Advances the horizon over segments `i0..i1`.
    fn advance(&self, v: &View, i0: usize, i1: usize, mut state: f64) -> f64 {
        for i in i0..i1 {
            let pr = self.param(v, i, None);
            self.seg_lit(i, pr, &mut state);
        }
        state
    }

    /// Groups facet `f`'s segments by line: `(first, last)` segment indexes.
    fn line_groups(&self, f: usize) -> Vec<(usize, usize)> {
        let fs = self.ls.facet_segs(f);
        let mut out = Vec::new();
        let mut i = 0;
        while i < fs.len() {
            let line = self.ls.segs[fs[i] as usize].line;
            let mut j = i;
            while j + 1 < fs.len() && self.ls.segs[fs[j + 1] as usize].line == line {
                j += 1;
            }
            out.push((fs[i] as usize, fs[j] as usize));
            i = j + 1;
        }
        out
    }

    /// First segment whose horizon matters for a point at `s_lo` on `line`.
    fn window_start(&self, first: usize) -> usize {
        let s = &self.ls.segs[first];
        let lr = &self.ls.lines[s.line as usize].segs;
        let mut init = first;
        while init > lr.start as usize && self.ls.segs[init - 1].s1 > s.s0 - self.dr {
            init -= 1;
        }
        init
    }

    /// Current total lit measure of facet `m`.
    fn lit_total(&self, v: &View, m: usize) -> f64 {
        let mut tot = 0.0;
        for (first, last) in self.line_groups(m) {
            let mut st = self.advance(v, self.window_start(first), first, self.ls.sweep.init());
            for i in first..=last {
                let pr = self.param(v, i, None);
                let l = self.seg_lit(i, pr, &mut st);
                if self.ls.segs[i].facet as usize == m {
                    tot += l;
                }
            }
        }
        tot
    }

    fn run_task(&self, task: &[u32], snap: &[Param], owner: &[u32], slot: &[u32], sweep: u32, opt: &OptimiserOpts) -> (Vec<(u32, Param)>, u64) {
        let tid = owner[task[0] as usize];
        let mut v = View { snap, owner, slot, task: tid, local: task.iter().map(|&f| snap[f as usize]).collect() };
        let mut moves = 0;
        for &f in task {
            if self.visit(&mut v, f as usize, sweep, opt) {
                moves += 1;
            }
        }
        let upd = task.iter().enumerate().filter(|(k, f)| v.local[*k] != snap[**f as usize]).map(|(k, &f)| (f, v.local[k])).collect();
        (upd, moves)
    }

    /// One coordinate-descent step on facet `f`. Returns whether it moved.
    fn visit(&self, v: &mut View, f: usize, sweep: u32, opt: &OptimiserOpts) -> bool {
        let ls = self.ls;
        let groups = self.line_groups(f);
        if groups.is_empty() {
            return false;
        }
        // Windows: from f's first segment on each line to D past its last.
        let mut wins = Vec::with_capacity(groups.len());
        for &(first, last) in &groups {
            let lr = &ls.lines[ls.segs[first].line as usize].segs;
            let s_hi = ls.segs[last].s1;
            let mut end = last + 1;
            while end < lr.end as usize && ls.segs[end].s0 < s_hi + self.dr {
                end += 1;
            }
            let state = self.advance(v, self.window_start(first), first, ls.sweep.init());
            wins.push(Win { first, end, state });
        }
        // Affected facets and each window segment's index into them.
        let mut aff: Vec<usize> = wins.iter().flat_map(|w| (w.first..w.end).map(|i| ls.segs[i].facet as usize)).collect();
        aff.sort_unstable();
        aff.dedup();
        let seg_aff: Vec<usize> = wins
            .iter()
            .flat_map(|w| (w.first..w.end).map(|i| aff.binary_search(&(ls.segs[i].facet as usize)).unwrap()))
            .collect();
        let fi = aff.binary_search(&f).unwrap();
        let lit_tot: Vec<f64> = aff.iter().map(|&m| self.lit_total(v, m)).collect();
        let cur: Vec<Param> = aff.iter().map(|&m| v.get(m)).collect();
        let direct_cur: Vec<f64> = aff.iter().zip(&cur).map(|(&m, p)| self.direct(m, p.th)).collect();
        let mut old_lit = Vec::with_capacity(seg_aff.len());
        for w in &wins {
            let mut st = w.state;
            for i in w.first..w.end {
                let pr = self.param(v, i, None);
                old_lit.push(self.seg_lit(i, pr, &mut st));
            }
        }
        let meas = &ls.facet_meas;
        let b_of = |k: usize, direct: f64, lit: f64| {
            let m = aff[k];
            if meas[m] > 0.0 { self.shadowed + direct * (lit / meas[m]).clamp(0.0, 1.0) } else { self.shadowed }
        };
        let sse_old: f64 = (0..aff.len())
            .map(|k| meas[aff[k]] * (b_of(k, direct_cur[k], lit_tot[k]) - self.target[aff[k]]).powi(2))
            .sum();
        let mut delta = vec![0.0; aff.len()];
        let mut score = |cand: Param| -> f64 {
            delta.iter_mut().for_each(|d| *d = 0.0);
            let mut pos = 0;
            for w in &wins {
                let mut st = w.state;
                for i in w.first..w.end {
                    let pr = self.param(v, i, Some((f, cand)));
                    let l = self.seg_lit(i, pr, &mut st);
                    delta[seg_aff[pos]] += l - old_lit[pos];
                    pos += 1;
                }
            }
            let d_f = self.direct(f, cand.th);
            (0..aff.len())
                .map(|k| {
                    let d = if k == fi { d_f } else { direct_cur[k] };
                    meas[aff[k]] * (b_of(k, d, lit_tot[k] + delta[k]) - self.target[aff[k]]).powi(2)
                })
                .sum()
        };

        let geo = self.geo;
        let p0 = cur[fi];
        let cap = geo.th_cap(f);
        let ts = opt.tilt_steps.max(1) as f64;
        let step = geo.s / ts * 0.5f64.powi(sweep as i32 + 1);
        let fit_h = |th: f64, h: f64| -> Option<Param> {
            let (lo, hi) = geo.h_range(f, th)?;
            Some(Param { th, h: if self.vary_height { h.clamp(lo, hi) } else { geo.c_common.clamp(lo, hi) } })
        };
        let mut best = (sse_old, p0);
        let consider = |c: Option<Param>, best: &mut (f64, Param), score: &mut dyn FnMut(Param) -> f64| {
            if let Some(c) = c {
                if c != best.1 {
                    let e = score(c);
                    if e < best.0 {
                        *best = (e, c);
                    }
                }
            }
        };
        // A: tilt (coarse grid + local steps) at the current height.
        let n = opt.tilt_steps.max(1) as i64;
        for i in -n..=n {
            let th = (geo.s * i as f64 / ts).clamp(-cap, cap);
            consider(fit_h(th, p0.h), &mut best, &mut score);
        }
        for k in [-2.0, -1.0, 1.0, 2.0] {
            consider(fit_h((p0.th + k * step).clamp(-cap, cap), p0.h), &mut best, &mut score);
        }
        // B: height for the best tilt.
        if self.vary_height {
            let th = best.1.th;
            if let Some((lo, hi)) = geo.h_range(f, th) {
                let hstep = (geo.ceil - geo.floor) / 16.0 * 0.5f64.powi(sweep as i32);
                let h0 = best.1.h;
                for h in [lo, hi, 0.5 * (lo + hi), h0 - hstep, h0 + hstep, h0 - 2.0 * hstep, h0 + 2.0 * hstep] {
                    consider(Some(Param { th, h: h.clamp(lo, hi) }), &mut best, &mut score);
                }
            }
        }
        // C: refine the tilt at the chosen height.
        let b1 = best.1;
        for k in [-1.0, -0.5, 0.5, 1.0] {
            consider(fit_h((b1.th + k * step).clamp(-cap, cap), b1.h), &mut best, &mut score);
        }
        if best.1 != p0 && best.0 < sse_old * (1.0 - 1e-12) {
            v.local[v.slot[f] as usize] = best.1;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::squama::{SquamaParams, solve_facets as squama_solve};

    #[test]
    fn lambert_matches_squama_solver() {
        let cb = CellBrightness { nx: 7, ny: 5, g: (0..35).map(|i| (i as f32 * 0.37).fract()).collect() };
        for az in [0.0, 1.0, std::f64::consts::PI, 4.0] {
            let sp = SquamaParams { width_mm: 42.0, cell_mm: 6.0, light_azimuth: az, ..Default::default() };
            let l = crate::squama::Layout {
                kind: crate::squama::LayoutKind::Square,
                w_mm: 42.0,
                h_mm: 30.0,
                cell_mm: 6.0,
                nx: 7,
                ny: 5,
                crop: None,
            };
            let want = squama_solve(&cb, &l, &sp, &Tol::default());
            let part = square_grid(7, 5, 6.0, DVec2::ZERO);
            let light = Light::Directional { az, el: sp.light_elevation };
            let lim = Limits { max_tilt: sp.max_tilt, base_mm: sp.base_mm, max_relief_mm: None };
            let got = lambert_facets(&cb, &part, &light, &lim, &Response::default()).unwrap();
            for (a, b) in want.iter().zip(&got) {
                assert!((a.pivot - b.pivot).length() < 1e-9);
                assert!((a.h - b.h).abs() < 1e-9, "{} vs {}", a.h, b.h);
                assert!((a.grad - b.grad).length() < 1e-9);
            }
        }
    }
}
