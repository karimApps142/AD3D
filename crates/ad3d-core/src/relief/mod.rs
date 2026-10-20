//! Generic convex-cell relief mesher (design §4–§8).
//!
//! Input: a 2D partition into convex pieces, one integer (µm) height per
//! (piece, corner), and one apex height per piece. Output: a closed, consistently
//! wound 2-manifold whose top is the piecewise-planar relief, with vertical walls
//! between pieces, boundary walls and a flat back at `w = 0`.
//!
//! All topology comes from indices and integer heights; nothing depends on
//! welding floats.

pub mod partition;

use glam::DVec2;

use crate::cancel::CancelToken;
use crate::error::{Error, Result};
use crate::mesh::{FaceKind, Mesh};
pub use partition::{Partition, PartitionKind, Piece, is_saddle, square_grid};

/// Tolerances (design §1.1). Tests may vary them; the app does not.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tol {
    /// Height quantum in mm. Heights are `i64` multiples of `q` (1 µm).
    pub q: f64,
    /// Minimum distance between distinct output vertices (mm).
    pub delta: f64,
    /// Height-cluster radius, in units of `q`.
    pub tau_q: i64,
    /// Saddle-split offset (mm).
    pub eps: f64,
}

impl Default for Tol {
    fn default() -> Self {
        Tol { q: 0.001, delta: 0.05, tau_q: 50, eps: 0.05 }
    }
}

impl Tol {
    /// `ceil(T / q)`, robust to `T / q` landing a hair above an integer.
    pub fn base_q(&self, base_mm: f64) -> i64 {
        (base_mm / self.q - 1e-6).ceil() as i64
    }
}

/// A logical facet plane: `w(p) = h + grad · (p − pivot)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Facet {
    pub pivot: DVec2,
    pub h: f64,
    pub grad: DVec2,
}

impl Facet {
    #[inline]
    pub fn eval(&self, p: DVec2) -> f64 {
        self.h + self.grad.dot(p - self.pivot)
    }
}

/// Quantised heights for a partition.
#[derive(Debug, Clone, PartialEq)]
pub struct Heights {
    /// One height per ring slot (parallel to `Partition::rings`), in units of `q`.
    pub corner: Vec<i64>,
    /// Apex height (mm) per piece, at the piece's vertex average.
    pub apex_w: Vec<f64>,
}

/// Back-face pocket or rib (M1, §14.3). The spike accepts none.
#[derive(Debug, Clone, PartialEq)]
pub struct BackRegion {
    pub poly: Vec<DVec2>,
    pub w_mm: f64,
}

/// Which pieces to mesh.
#[derive(Debug, Clone, Copy)]
pub enum PieceSet<'a> {
    All,
    Subset(&'a [u32]),
}

/// Vertex average of a piece's ring (strictly inside a convex piece).
pub fn piece_apex(p: &Partition, piece: usize) -> DVec2 {
    let r = &p.pieces[piece].ring;
    let mut s = DVec2::ZERO;
    for k in r.start..r.end {
        s += p.verts[p.rings[k as usize] as usize];
    }
    s / (r.end - r.start) as f64
}

/// §5 + §5.1: evaluates every facet once per (piece, corner), rounds half away
/// from zero to `q`, checks `corner ≥ ceil(T/q)`, then clusters heights at each
/// vertex (snap up by at most τ).
pub fn quantise(p: &Partition, facets: &[Facet], base_mm: f64, tol: &Tol) -> Result<Heights> {
    let mut corner = vec![0i64; p.rings.len()];
    let mut apex_w = Vec::with_capacity(p.pieces.len());
    let t_q = tol.base_q(base_mm);
    for (pi, piece) in p.pieces.iter().enumerate() {
        let f = facets
            .get(piece.facet as usize)
            .ok_or_else(|| Error::Internal(format!("piece {pi} references missing facet {}", piece.facet)))?;
        for s in piece.ring.start..piece.ring.end {
            let w = f.eval(p.verts[p.rings[s as usize] as usize]);
            if !w.is_finite() {
                return Err(Error::Internal(format!("non-finite height at piece {pi}")));
            }
            let h = (w / tol.q).round() as i64;
            if h < t_q {
                return Err(Error::InvalidParams(format!(
                    "corner height {:.4} mm below base {base_mm} mm at piece {pi}",
                    h as f64 * tol.q
                )));
            }
            corner[s as usize] = h;
        }
        let a = f.eval(piece_apex(p, pi));
        if !a.is_finite() {
            return Err(Error::Internal(format!("non-finite apex at piece {pi}")));
        }
        apex_w.push(a);
    }
    cluster_heights(p, &mut corner, tol.tau_q);
    Ok(Heights { corner, apex_w })
}

/// §5.1 clustering in place: at every vertex, sort the incident corner heights
/// descending; each opens a cluster with top `t` or, if `h ≥ t − τ`, is raised to `t`.
pub fn cluster_heights(p: &Partition, corner: &mut [i64], tau_q: i64) {
    let nv = p.verts.len();
    let mut start = vec![0u32; nv + 1];
    for &v in &p.rings {
        start[v as usize + 1] += 1;
    }
    for k in 0..nv {
        start[k + 1] += start[k];
    }
    let mut fill = start.clone();
    let mut slots = vec![0u32; p.rings.len()];
    for (s, &v) in p.rings.iter().enumerate() {
        slots[fill[v as usize] as usize] = s as u32;
        fill[v as usize] += 1;
    }
    let mut hs: Vec<i64> = Vec::with_capacity(8);
    for v in 0..nv {
        let inc = &slots[start[v] as usize..start[v + 1] as usize];
        if inc.len() < 2 {
            continue;
        }
        hs.clear();
        hs.extend(inc.iter().map(|&s| corner[s as usize]));
        hs.sort_unstable_by(|a, b| b.cmp(a));
        hs.dedup();
        if hs.len() < 2 {
            continue;
        }
        // Map each distinct height to its cluster top.
        let mut top = hs[0];
        let mut map: [(i64, i64); 16] = [(0, 0); 16];
        let mut mapped: Vec<(i64, i64)> = Vec::new();
        let small = hs.len() <= 16;
        for (k, &h) in hs.iter().enumerate() {
            if h < top - tau_q {
                top = h;
            }
            if small {
                map[k] = (h, top);
            } else {
                mapped.push((h, top));
            }
        }
        let table: &[(i64, i64)] = if small { &map[..hs.len()] } else { &mapped };
        for &s in inc {
            let h = corner[s as usize];
            if let Some(&(_, t)) = table.iter().find(|(x, _)| *x == h) {
                corner[s as usize] = t;
            }
        }
    }
}

/// Statistics from one mesher run.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize)]
pub struct MeshStats {
    pub saddles: u32,
    pub crossings: u32,
    pub walls: u32,
    pub outline_vertices: u32,
}

const NONE: u32 = u32::MAX;
/// A wall crossing sits at least `CROSSING_MARGIN · δ` from either end of its edge.
pub const CROSSING_MARGIN: f64 = 1.05;
/// Edges carrying a crossing must be at least this many δ long (> 2 × margin).
pub const CROSSING_MIN_EDGE: f64 = 2.2;

/// §6.1 + §7: meshes the selected pieces into one closed part.
pub fn mesh_relief(
    p: &Partition,
    pieces: PieceSet<'_>,
    h: &Heights,
    back: &[BackRegion],
    tol: &Tol,
    cancel: &CancelToken,
) -> Result<Mesh> {
    mesh_relief_stats(p, pieces, h, back, tol, cancel).map(|(m, _)| m)
}

/// [`mesh_relief`] that also returns [`MeshStats`].
pub fn mesh_relief_stats(
    p: &Partition,
    pieces: PieceSet<'_>,
    h: &Heights,
    back: &[BackRegion],
    tol: &Tol,
    cancel: &CancelToken,
) -> Result<(Mesh, MeshStats)> {
    if !back.is_empty() {
        return Err(Error::InvalidParams("back regions are not supported until M1".into()));
    }
    if h.corner.len() != p.rings.len() || h.apex_w.len() != p.pieces.len() {
        return Err(Error::Internal("heights do not match the partition".into()));
    }
    // §6 requirements (convex CCW pieces, edges ≥ 4δ), in release too: the generic
    // mesher is public and must return an error, never panic, on a bad partition
    // (audit A2). A square grid is built from a lattice (and the code below already
    // relies on its layout), so it gets an O(1) shape and cell-size check instead of
    // the 0.4 ms full pass at 100×126. The crossing code also refuses edges < 2.2δ.
    match p.kind {
        PartitionKind::SquareGrid { nx, ny } => {
            let (nxu, nyu) = (nx as usize, ny as usize);
            if nx == 0
                || ny == 0
                || p.verts.len() != (nxu + 1) * (nyu + 1)
                || p.rings.len() != 4 * nxu * nyu
                || p.pieces.len() != nxu * nyu
            {
                return Err(Error::InvalidParams("square grid partition has inconsistent sizes".into()));
            }
            let (v0, v1, vn) = (p.verts[0], p.verts[1], p.verts[nxu + 1]);
            let (dx, dy) = (v1 - v0, vn - v0);
            let min_len = 4.0 * tol.delta - 1e-12;
            if !(dx.x >= min_len && dy.y >= min_len && dx.y == 0.0 && dy.x == 0.0) {
                return Err(Error::InvalidParams(format!(
                    "square grid cell {:?} x {:?} is not axis-aligned with sides ≥ 4δ ({:.4} mm)",
                    dx,
                    dy,
                    4.0 * tol.delta
                )));
            }
        }
        PartitionKind::General => p.check(tol)?,
    }
    let mut stats = MeshStats::default();
    let npieces = p.pieces.len();
    let selected: Vec<u32> = match pieces {
        PieceSet::All => (0..npieces as u32).collect(),
        PieceSet::Subset(s) => {
            let mut v = s.to_vec();
            v.sort_unstable();
            v.dedup();
            if v.last().is_some_and(|&x| x as usize >= npieces) {
                return Err(Error::InvalidParams("piece index out of range".into()));
            }
            v
        }
    };
    if selected.is_empty() {
        return Err(Error::InvalidParams("no pieces to mesh".into()));
    }
    let mut is_sel = vec![false; npieces];
    for &s in &selected {
        is_sel[s as usize] = true;
    }
    let check_every = match p.kind {
        PartitionKind::SquareGrid { nx, .. } => nx.max(1) as usize,
        PartitionKind::General => 4096,
    };

    // ---- §6.1: effective partition with saddle splits ----
    let mut verts: Vec<DVec2> = p.verts.clone();
    // Per original slot: up to two replacement vertices.
    let mut repl: Vec<[u32; 2]> = Vec::new();
    if let PartitionKind::SquareGrid { nx, ny } = p.kind {
        let pid = |i: u32, j: u32| (j * nx + i) as usize;
        for j in 1..ny {
            for i in 1..nx {
                let (a, b, c, d) = (pid(i - 1, j), pid(i, j), pid(i, j - 1), pid(i - 1, j - 1));
                if !(is_sel[a] && is_sel[b] && is_sel[c] && is_sel[d]) {
                    continue;
                }
                let sa = p.pieces[a].ring.start as usize + 1;
                let sb = p.pieces[b].ring.start as usize;
                let sc = p.pieces[c].ring.start as usize + 3;
                let sd = p.pieces[d].ring.start as usize + 2;
                if !is_saddle(h.corner[sa], h.corner[sb], h.corner[sc], h.corner[sd]) {
                    continue;
                }
                if repl.is_empty() {
                    repl = vec![[NONE, NONE]; p.rings.len()];
                }
                let pv = p.verts[p.rings[sa] as usize];
                let p1 = verts.len() as u32;
                verts.push(pv + DVec2::new(-tol.eps, -tol.eps));
                let p2 = p1 + 1;
                verts.push(pv + DVec2::new(tol.eps, tol.eps));
                repl[sa] = [p1, p2];
                repl[sc] = [p2, p1];
                repl[sb] = [p2, NONE];
                repl[sd] = [p1, NONE];
                stats.saddles += 1;
            }
        }
    }
    let nv2 = verts.len();
    let mut ring: Vec<u32> = Vec::with_capacity(p.rings.len() + 2 * stats.saddles as usize);
    let mut ring_h: Vec<i64> = Vec::with_capacity(ring.capacity());
    let mut piece_start: Vec<u32> = Vec::with_capacity(selected.len() + 1);
    for &pi in &selected {
        piece_start.push(ring.len() as u32);
        let r = &p.pieces[pi as usize].ring;
        for s in r.start as usize..r.end as usize {
            let hh = h.corner[s];
            if !repl.is_empty() && repl[s][0] != NONE {
                ring.push(repl[s][0]);
                ring_h.push(hh);
                if repl[s][1] != NONE {
                    ring.push(repl[s][1]);
                    ring_h.push(hh);
                }
            } else {
                ring.push(p.rings[s]);
                ring_h.push(hh);
            }
        }
    }
    piece_start.push(ring.len() as u32);
    let nslots = ring.len();
    let mut next = vec![0u32; nslots];
    for k in 0..selected.len() {
        let (s0, s1) = (piece_start[k], piece_start[k + 1]);
        for s in s0..s1 {
            next[s as usize] = if s + 1 == s1 { s0 } else { s + 1 };
        }
    }

    // ---- adjacency: outgoing slots per vertex, twins ----
    let mut out_start = vec![0u32; nv2 + 1];
    for &v in &ring {
        out_start[v as usize + 1] += 1;
    }
    for k in 0..nv2 {
        out_start[k + 1] += out_start[k];
    }
    let mut out_slots = vec![0u32; nslots];
    {
        let mut fill = out_start.clone();
        for (s, &v) in ring.iter().enumerate() {
            out_slots[fill[v as usize] as usize] = s as u32;
            fill[v as usize] += 1;
        }
    }
    let mut twin = vec![NONE; nslots];
    let mut on_outline = vec![false; nv2];
    for s in 0..nslots {
        let a = ring[s];
        let b = ring[next[s] as usize];
        let mut found = NONE;
        for &t in &out_slots[out_start[b as usize] as usize..out_start[b as usize + 1] as usize] {
            if ring[next[t as usize] as usize] == a {
                if found != NONE {
                    return Err(Error::Internal("edge shared by more than two pieces".into()));
                }
                found = t;
            }
        }
        twin[s] = found;
        if found == NONE {
            on_outline[a as usize] = true;
            on_outline[b as usize] = true;
        }
    }
    for v in 0..nv2 {
        let val = out_start[v + 1] - out_start[v];
        if on_outline[v] && val > 2 {
            return Err(Error::Internal(format!("outline vertex {v} has {val} pieces (needs §6.3)")));
        }
        if !on_outline[v] && val >= 4 {
            // Square-grid vertices of valence 4 are fine after §6.1 (non-saddle).
            let grid_vertex = matches!(p.kind, PartitionKind::SquareGrid { .. }) && v < p.verts.len() && val == 4;
            if !grid_vertex {
                return Err(Error::Internal(format!("interior vertex {v} has valence {val} (needs §6.3)")));
            }
        }
    }

    // ---- §7.1 vertical-line vertices ----
    let est_v = nslots + nslots / 2 + selected.len();
    let mut pos: Vec<[f64; 3]> = Vec::with_capacity(est_v);
    let mut line_start = vec![0u32; nv2 + 1];
    let mut line_h: Vec<i64> = Vec::with_capacity(nslots + 64);
    let mut corner_vid = vec![0u32; nslots];
    let mut hs: Vec<i64> = Vec::with_capacity(8);
    for v in 0..nv2 {
        line_start[v] = pos.len() as u32;
        let outs = &out_slots[out_start[v] as usize..out_start[v + 1] as usize];
        if outs.is_empty() {
            continue;
        }
        hs.clear();
        hs.extend(outs.iter().map(|&s| ring_h[s as usize]));
        if on_outline[v] {
            hs.push(0);
        }
        hs.sort_unstable();
        hs.dedup();
        if hs[0] < 0 || (hs[0] == 0 && !on_outline[v]) || (on_outline[v] && hs.len() > 1 && hs[1] <= 0) {
            return Err(Error::Internal(format!("height <= 0 at vertex {v}")));
        }
        let xy = verts[v];
        let base = pos.len() as u32;
        for &hh in &hs {
            pos.push([xy.x, xy.y, hh as f64 * tol.q]);
            line_h.push(hh);
        }
        for &s in outs {
            let k = hs.iter().position(|&x| x == ring_h[s as usize]).unwrap();
            corner_vid[s as usize] = base + k as u32;
        }
    }
    line_start[nv2] = pos.len() as u32;
    let n_line = pos.len();

    // ---- §7.2 crossings ----
    let mut cross = vec![NONE; nslots];
    for s in 0..nslots {
        let t = twin[s];
        if t == NONE || (t as usize) < s {
            continue;
        }
        let ns = next[s] as usize;
        let nt = next[t as usize] as usize;
        let d0 = ring_h[s] - ring_h[nt];
        let d1 = ring_h[ns] - ring_h[t as usize];
        if (d0 < 0 && d1 > 0) || (d0 > 0 && d1 < 0) {
            let a = verts[ring[s] as usize];
            let b = verts[ring[ns] as usize];
            let len = (b - a).length();
            // Keep the crossing a little more than δ from both ends: a crossing at
            // exactly δ from a corner whose lower piece is flat along the edge lands
            // under the δ gate once both points are rounded to f32 (audit A1). The
            // 5% margin (2.5 µm at δ = 50 µm) is ~20 f32 ulps at 2048 mm.
            if len < CROSSING_MIN_EDGE * tol.delta {
                return Err(Error::InvalidParams(format!(
                    "edge of {:.4} mm with a height crossing is shorter than {CROSSING_MIN_EDGE}δ ({:.4} mm)",
                    len,
                    CROSSING_MIN_EDGE * tol.delta
                )));
            }
            let mut tt = d0 as f64 / (d0 - d1) as f64;
            let lo = CROSSING_MARGIN * tol.delta / len;
            tt = tt.clamp(lo, 1.0 - lo);
            let xy = a + (b - a) * tt;
            let w = (ring_h[s] as f64 + tt * (ring_h[ns] - ring_h[s]) as f64) * tol.q;
            let id = pos.len() as u32;
            pos.push([xy.x, xy.y, w]);
            cross[s] = id;
            cross[t as usize] = id;
            stats.crossings += 1;
        }
    }

    let mut tris: Vec<[u32; 3]> = Vec::with_capacity(2 * est_v);
    let mut kinds: Vec<FaceKind> = Vec::with_capacity(2 * est_v);

    // ---- §7.3 facets ----
    let mut loop_ids: Vec<u32> = Vec::with_capacity(16);
    for (k, &pi) in selected.iter().enumerate() {
        if k % check_every == 0 {
            cancel.check()?;
        }
        loop_ids.clear();
        for s in piece_start[k]..piece_start[k + 1] {
            loop_ids.push(corner_vid[s as usize]);
            if cross[s as usize] != NONE {
                loop_ids.push(cross[s as usize]);
            }
        }
        let c = piece_apex(p, pi as usize);
        let cid = pos.len() as u32;
        pos.push([c.x, c.y, h.apex_w[pi as usize]]);
        let m = loop_ids.len();
        for q in 0..m {
            tris.push([cid, loop_ids[q], loop_ids[(q + 1) % m]]);
            kinds.push(FaceKind::Facet);
        }
    }

    // ---- §7.4 interior walls, §7.5 boundary walls ----
    let side = |v: u32, lo: i64, hi: i64| -> (u32, u32) {
        let (s, e) = (line_start[v as usize], line_start[v as usize + 1]);
        let mut first = NONE;
        let mut last = NONE;
        for id in s..e {
            let hh = line_h[id as usize];
            if hh >= lo && hh <= hi {
                if first == NONE {
                    first = id;
                }
                last = id;
            }
        }
        (first, last)
    };
    for s in 0..nslots {
        if s % (check_every * 4) == 0 {
            cancel.check()?;
        }
        let t = twin[s];
        let ns = next[s] as usize;
        let (a, b) = (ring[s], ring[ns]);
        if t == NONE {
            // Boundary wall: from the back level up to the piece's corner.
            let l = side(a, 0, ring_h[s]);
            let r = side(b, 0, ring_h[ns]);
            ladder(&pos, l, r, true, &mut tris);
            kinds.resize(tris.len(), FaceKind::Wall);
            stats.walls += 1;
            continue;
        }
        if (t as usize) < s {
            continue;
        }
        let nt = next[t as usize] as usize;
        let (ha_a, ha_b) = (ring_h[s], ring_h[ns]);
        let (hb_a, hb_b) = (ring_h[nt], ring_h[t as usize]);
        let d0 = ha_a - hb_a;
        let d1 = ha_b - hb_b;
        if d0 == 0 && d1 == 0 {
            continue;
        }
        let l = side(a, ha_a.min(hb_a), ha_a.max(hb_a));
        let r = side(b, ha_b.min(hb_b), ha_b.max(hb_b));
        let x = cross[s];
        if x != NONE {
            ladder(&pos, l, (x, x), d0 > 0, &mut tris);
            ladder(&pos, (x, x), r, d1 > 0, &mut tris);
            stats.walls += 2;
        } else {
            ladder(&pos, l, r, d0 + d1 > 0, &mut tris);
            stats.walls += 1;
        }
        kinds.resize(tris.len(), FaceKind::Wall);
    }

    // ---- §7.6 back face ----
    let mut bnext = vec![NONE; nv2];
    let mut nb = 0usize;
    let mut first = NONE;
    for s in 0..nslots {
        if twin[s] == NONE {
            let a = ring[s];
            if bnext[a as usize] != NONE {
                return Err(Error::Internal("outline visits a vertex twice".into()));
            }
            bnext[a as usize] = ring[next[s] as usize];
            nb += 1;
            if first == NONE {
                first = a;
            }
        }
    }
    let mut outline = Vec::with_capacity(nb);
    let mut v = first;
    loop {
        outline.push(v);
        v = bnext[v as usize];
        if v == first || v == NONE || outline.len() > nb {
            break;
        }
    }
    if outline.len() != nb || v != first {
        return Err(Error::Internal(format!("outline is not one loop ({} of {nb} edges)", outline.len())));
    }
    stats.outline_vertices = nb as u32;
    let pts: Vec<DVec2> = outline.iter().map(|&v| verts[v as usize]).collect();
    for tri in triangulate_outline(&pts)? {
        let ids = tri.map(|k| line_start[outline[k as usize] as usize]);
        // CDT triangles are CCW from +w; the back faces −w.
        tris.push([ids[0], ids[2], ids[1]]);
        kinds.push(FaceKind::Back);
    }
    debug_assert!(outline.iter().all(|&v| line_h[line_start[v as usize] as usize] == 0));
    let _ = n_line;

    // ---- output ----
    let mut mesh = Mesh {
        positions: Vec::with_capacity(pos.len() * 3),
        indices: Vec::with_capacity(tris.len() * 3),
        face_kind: kinds,
    };
    for q in &pos {
        mesh.positions.extend_from_slice(&[q[0] as f32, q[1] as f32, q[2] as f32]);
    }
    for t in &tris {
        mesh.indices.extend_from_slice(t);
    }
    Ok((mesh, stats))
}

/// Ladder between two vertical sides (inclusive id ranges, bottom to top), §7.4.
/// Advance R iff `|L_i R_{j+1}| ≤ |L_{i+1} R_j|`; `a_higher` picks the facing.
#[inline]
fn ladder(pos: &[[f64; 3]], l: (u32, u32), r: (u32, u32), a_higher: bool, out: &mut Vec<[u32; 3]>) {
    let d2 = |x: u32, y: u32| {
        let (p, q) = (pos[x as usize], pos[y as usize]);
        let (dx, dy, dz) = (p[0] - q[0], p[1] - q[1], p[2] - q[2]);
        dx * dx + dy * dy + dz * dz
    };
    let (mut i, mut j) = (l.0, r.0);
    while i < l.1 || j < r.1 {
        let adv_r = if i == l.1 {
            true
        } else if j == r.1 {
            false
        } else {
            d2(i, j + 1) <= d2(i + 1, j)
        };
        let t = if adv_r {
            j += 1;
            [i, j - 1, j]
        } else {
            i += 1;
            [i - 1, j, i]
        };
        out.push(if a_higher { t } else { [t[0], t[2], t[1]] });
    }
}

/// Triangulates a simple CCW outline polygon without Steiner points (§7.6).
/// Returns CCW triangles as indices into `pts`.
///
/// Convex outlines (every M0 plate and every M1 tile is a rectangle) use an O(n)
/// two-chain strip advanced by normalised arc length, which never makes a triangle
/// from three collinear outline points, never folds, and on a rectangle of square
/// cells keeps every back triangle's altitude near `cell/√2`. Anything else goes
/// through a constrained Delaunay triangulation (spade).
pub fn triangulate_outline(pts: &[DVec2]) -> Result<Vec<[u32; 3]>> {
    if let Some(t) = triangulate_convex_strip(pts) {
        return Ok(t);
    }
    triangulate_outline_cdt(pts)
}

/// Two-chain strip triangulation of a convex CCW polygon whose collinear runs are
/// kept. Returns `None` when the polygon is not convex or has fewer than two strict
/// corners on each chain.
pub fn triangulate_convex_strip(pts: &[DVec2]) -> Option<Vec<[u32; 3]>> {
    let n = pts.len();
    if n < 3 {
        return None;
    }
    let turn = |k: usize| (pts[k] - pts[(k + n - 1) % n]).perp_dot(pts[(k + 1) % n] - pts[k]);
    let mut corners = Vec::new();
    for k in 0..n {
        let t = turn(k);
        if t < 0.0 {
            return None;
        }
        if t > 0.0 {
            corners.push(k);
        }
    }
    if corners.len() < 3 {
        return None;
    }
    // s and e: strict corners about half the perimeter (in vertex count) apart.
    let s = corners[0];
    let e = *corners.iter().min_by_key(|&&c| ((c + n - s) % n).abs_diff(n / 2)).unwrap();
    let la = (e + n - s) % n; // steps along A (CCW) from s to e
    let lb = n - la; // steps along B (CW) from s to e
    if la < 2 || lb < 2 {
        return None;
    }
    let a = |i: usize| (s + i) % n; // i in 0..=la
    let b = |j: usize| (s + n - j) % n; // j in 0..=lb
    // Cumulative arc length along each chain from s. The strip advances the chain
    // whose next vertex is at the smaller fraction of its chain's length, so each
    // diagonal joins points at about the same fraction of the way from s to e. The
    // shortest-diagonal rule used before ran one chain far ahead on large square
    // outlines and ended in a fan of needles of altitude ≈ cell²/width (audit A5).
    let mut cum_a = vec![0.0f64; la + 1];
    for i in 1..=la {
        cum_a[i] = cum_a[i - 1] + (pts[a(i)] - pts[a(i - 1)]).length();
    }
    let mut cum_b = vec![0.0f64; lb + 1];
    for j in 1..=lb {
        cum_b[j] = cum_b[j - 1] + (pts[b(j)] - pts[b(j - 1)]).length();
    }
    let (len_a, len_b) = (cum_a[la], cum_b[lb]);
    let mut out = Vec::with_capacity(n - 2);
    out.push([s as u32, a(1) as u32, b(1) as u32]);
    let (mut i, mut j) = (1usize, 1usize);
    while i < la - 1 || j < lb - 1 {
        let adv_a = if i == la - 1 {
            false
        } else if j == lb - 1 {
            true
        } else {
            cum_a[i + 1] * len_b <= cum_b[j + 1] * len_a
        };
        if adv_a {
            out.push([a(i) as u32, a(i + 1) as u32, b(j) as u32]);
            i += 1;
        } else {
            out.push([a(i) as u32, b(j + 1) as u32, b(j) as u32]);
            j += 1;
        }
    }
    out.push([a(la - 1) as u32, e as u32, b(lb - 1) as u32]);
    Some(out)
}

/// Constrained Delaunay triangulation of a simple CCW polygon (no Steiner points).
pub fn triangulate_outline_cdt(pts: &[DVec2]) -> Result<Vec<[u32; 3]>> {
    use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};
    let n = pts.len();
    if n < 3 {
        return Err(Error::Internal("outline has fewer than 3 vertices".into()));
    }
    let vertices: Vec<Point2<f64>> = pts.iter().map(|p| Point2::new(p.x, p.y)).collect();
    let edges: Vec<[usize; 2]> = (0..n).map(|k| [k, (k + 1) % n]).collect();
    // bulk_load_cdt keeps the vertex order, so handle index k is pts[k].
    let cdt: ConstrainedDelaunayTriangulation<Point2<f64>> = ConstrainedDelaunayTriangulation::bulk_load_cdt(vertices, edges)
        .map_err(|e| Error::Internal(format!("CDT: {e:?}")))?;
    if cdt.num_vertices() != n {
        return Err(Error::Internal("outline has duplicate vertices".into()));
    }
    let convex = (0..n).all(|k| (pts[(k + 1) % n] - pts[k]).perp_dot(pts[(k + 2) % n] - pts[(k + 1) % n]) >= 0.0);
    let mut out = Vec::with_capacity(n);
    for f in cdt.inner_faces() {
        let vs = f.vertices();
        let t = [vs[0].fix().index() as u32, vs[1].fix().index() as u32, vs[2].fix().index() as u32];
        if !convex {
            let c = (pts[t[0] as usize] + pts[t[1] as usize] + pts[t[2] as usize]) / 3.0;
            if !point_in_polygon(c, pts) {
                continue;
            }
        }
        out.push(t);
    }
    Ok(out)
}

fn point_in_polygon(c: DVec2, pts: &[DVec2]) -> bool {
    let mut inside = false;
    let n = pts.len();
    for k in 0..n {
        let (a, b) = (pts[k], pts[(k + 1) % n]);
        if (a.y > c.y) != (b.y > c.y) && c.x < a.x + (c.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}
