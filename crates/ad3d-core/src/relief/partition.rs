//! 2D cell partitions (design §6) and the square-grid saddle split (§6.1).

use std::ops::Range;

use glam::DVec2;

use crate::error::{Error, Result};
use crate::relief::Tol;

/// One convex polygon of the partition, carrying one facet.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub facet: u32,
    /// Range into [`Partition::rings`].
    pub ring: Range<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionKind {
    /// `nx × ny` unit squares: vertex `(i, j)` has id `j·(nx+1) + i`, piece `(i, j)` has
    /// id `j·nx + i` and ring `[(i,j), (i+1,j), (i+1,j+1), (i,j+1)]` at `4·piece`.
    SquareGrid { nx: u32, ny: u32 },
    /// Any conforming convex partition whose interior vertices have valence 3.
    General,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Partition {
    pub verts: Vec<DVec2>,
    /// Concatenated CCW vertex loops (seen from +w).
    pub rings: Vec<u32>,
    pub pieces: Vec<Piece>,
    /// CCW boundary loop.
    pub outline: Vec<u32>,
    pub kind: PartitionKind,
}

impl Partition {
    /// Area of the outline polygon (shoelace).
    pub fn outline_area(&self) -> f64 {
        polygon_area(self.outline.iter().map(|&v| self.verts[v as usize]))
    }

    /// Checks the §6 requirements that do not depend on heights: every piece strictly
    /// convex and CCW, edges at least `4δ` long, distinct vertices at least δ apart
    /// (checked between ring neighbours), every edge shared by at most two pieces as
    /// a twin pair. Called by `mesh_relief` on every run (release too).
    pub fn check(&self, tol: &Tol) -> Result<()> {
        // Hot enough to matter (it runs on every mesh): no modulo, squared lengths,
        // and the error text is only built on failure.
        let min_len = 4.0 * tol.delta - 1e-12;
        let min_len2 = min_len * min_len;
        for (pi, p) in self.pieces.iter().enumerate() {
            let ring = self
                .rings
                .get(p.ring.start as usize..p.ring.end as usize)
                .ok_or_else(|| Error::InvalidParams(format!("piece {pi} ring out of range")))?;
            let n = ring.len();
            if n < 3 {
                return Err(Error::InvalidParams(format!("piece {pi} has {n} corners")));
            }
            let vert = |i: u32| {
                self.verts.get(i as usize).copied().ok_or_else(|| Error::InvalidParams(format!("piece {pi} vertex out of range")))
            };
            let mut a = vert(ring[n - 2])?;
            let mut b = vert(ring[n - 1])?;
            for (k, &ic) in ring.iter().enumerate() {
                let c = vert(ic)?;
                let (ab, bc) = (b - a, c - b);
                // `!(x > 0)` also catches NaN.
                if !(ab.perp_dot(bc) > 0.0) {
                    let corner = (k + n - 1) % n;
                    return Err(Error::InvalidParams(if ab.perp_dot(bc).is_finite() {
                        format!("piece {pi} not strictly convex/CCW at corner {corner}")
                    } else {
                        format!("piece {pi} has a non-finite vertex")
                    }));
                }
                if !(ab.length_squared() >= min_len2) {
                    return Err(Error::InvalidParams(format!(
                        "piece {pi} edge {:.4} mm shorter than 4δ ({:.4} mm)",
                        ab.length(),
                        4.0 * tol.delta
                    )));
                }
                a = b;
                b = c;
            }
        }
        Ok(())
    }
}

pub(crate) fn polygon_area(pts: impl Iterator<Item = DVec2> + Clone) -> f64 {
    let v: Vec<DVec2> = pts.collect();
    let n = v.len();
    let mut s = 0.0;
    for k in 0..n {
        s += v[k].perp_dot(v[(k + 1) % n]);
    }
    s * 0.5
}

/// Square grid of `nx × ny` cells of side `cell`, lower-left corner at `origin`.
/// Vertex coordinates are computed from integer lattice indices.
pub fn square_grid(nx: u32, ny: u32, cell: f64, origin: DVec2) -> Partition {
    let (nxu, nyu) = (nx as usize, ny as usize);
    let mut verts = Vec::with_capacity((nxu + 1) * (nyu + 1));
    for j in 0..=ny {
        for i in 0..=nx {
            verts.push(DVec2::new(origin.x + i as f64 * cell, origin.y + j as f64 * cell));
        }
    }
    let vid = |i: u32, j: u32| j * (nx + 1) + i;
    let mut rings = Vec::with_capacity(nxu * nyu * 4);
    let mut pieces = Vec::with_capacity(nxu * nyu);
    for j in 0..ny {
        for i in 0..nx {
            let s = rings.len() as u32;
            rings.extend_from_slice(&[vid(i, j), vid(i + 1, j), vid(i + 1, j + 1), vid(i, j + 1)]);
            pieces.push(Piece { facet: j * nx + i, ring: s..s + 4 });
        }
    }
    let mut outline = Vec::with_capacity(2 * (nxu + nyu));
    for i in 0..nx {
        outline.push(vid(i, 0));
    }
    for j in 0..ny {
        outline.push(vid(nx, j));
    }
    for i in (1..=nx).rev() {
        outline.push(vid(i, ny));
    }
    for j in (1..=ny).rev() {
        outline.push(vid(0, j));
    }
    Partition { verts, rings, pieces, outline, kind: PartitionKind::SquareGrid { nx, ny } }
}

/// The §6.1 saddle test on clustered heights at one grid vertex: A upper-left,
/// B upper-right, C lower-right, D lower-left.
#[inline]
pub fn is_saddle(h_a: i64, h_b: i64, h_c: i64, h_d: i64) -> bool {
    h_a.min(h_c) > h_b.max(h_d) || h_b.min(h_d) > h_a.max(h_c)
}

impl Partition {
    /// Pieces whose vertex average lies inside the closed rectangle `[min, max]`.
    /// With cut lines in the partition (§14.1) every piece lies wholly on one side of
    /// each cut, so this selects exactly one tile's pieces.
    pub fn pieces_in_rect(&self, min: DVec2, max: DVec2) -> Vec<u32> {
        let mut out = Vec::new();
        for (pi, p) in self.pieces.iter().enumerate() {
            let mut s = DVec2::ZERO;
            for k in p.ring.start..p.ring.end {
                s += self.verts[self.rings[k as usize] as usize];
            }
            let c = s / (p.ring.end - p.ring.start) as f64;
            if c.x >= min.x && c.x <= max.x && c.y >= min.y && c.y <= max.y {
                out.push(pi as u32);
            }
        }
        out
    }

    /// Area of one piece (shoelace).
    pub fn piece_area(&self, piece: usize) -> f64 {
        let r = &self.pieces[piece].ring;
        polygon_area(self.rings[r.start as usize..r.end as usize].iter().map(|&v| self.verts[v as usize]))
    }
}

// ---------------------------------------------------------------------------
// Hex lattice (§6.2)
// ---------------------------------------------------------------------------

/// The §6.2 hex lattice in integer units.
///
/// Construction space `(s, t)`: `s` runs across the flats in units of `cell/2`,
/// `t` runs along the rows in units of `R/2` (`R = cell/√3`, row pitch `3·R/2`).
/// Hex `(j, uc)` has its centre at lattice `(uc, 3j)` with `uc ≡ j (mod 2)`; its
/// corners are `(uc, 3j ± 2)` and `(uc ± 1, 3j ± 1)`. The plate is
/// `s ∈ [0, m]`, `t ∈ [0, 3n]`, so a hex centre sits at the origin, every outline
/// runs along a centre line or a hex edge, and every clip point is a lattice point.
///
/// Pointy-top: `u = s·cell/2`, `v = t·R/2`. Flat-top swaps the axes:
/// `u = t·R/2`, `v = s·cell/2` (rings are reversed to stay CCW).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HexLattice {
    /// Plate extent across the flats, in half-cells (`≥ 2`).
    pub m: u32,
    /// Plate extent along the rows, in row pitches (`≥ 1`).
    pub n: u32,
    /// Width across flats (mm).
    pub cell: f64,
    /// Flat-top (axes swapped) instead of pointy-top.
    pub flat: bool,
}

/// Lattice offsets of a full hex's corners from its centre, CCW in `(s, t)`.
pub const HEX_CORNERS: [(i64, i64); 6] = [(0, -2), (1, -1), (1, 1), (0, 2), (-1, 1), (-1, -1)];

impl HexLattice {
    /// `R/2` in mm, the `t` unit.
    #[inline]
    pub fn t_unit(&self) -> f64 {
        self.cell / (2.0 * 3f64.sqrt())
    }
    /// `cell/2` in mm, the `s` unit.
    #[inline]
    pub fn s_unit(&self) -> f64 {
        self.cell / 2.0
    }
    /// Row pitch `(√3/2)·cell` in mm.
    #[inline]
    pub fn pitch(&self) -> f64 {
        3.0 * self.t_unit()
    }
    fn row_lens(&self) -> (u32, u32) {
        (self.m / 2 + 1, (self.m - 1) / 2 + 1)
    }
    /// Number of hexes with positive area inside the plate (= number of facets).
    pub fn facet_count(&self) -> u32 {
        let (l0, l1) = self.row_lens();
        let rows = self.n + 1;
        (rows / 2) * (l0 + l1) + (rows % 2) * l0
    }
    /// Facet index of hex `(j, uc)`.
    pub fn facet_of(&self, j: u32, uc: u32) -> u32 {
        let (l0, l1) = self.row_lens();
        (j / 2) * (l0 + l1) + (j % 2) * l0 + (uc - j % 2) / 2
    }
    /// Lattice centre `(uc, 3j)` of facet `f`.
    pub fn centre_lattice(&self, f: u32) -> (i64, i64) {
        let (l0, l1) = self.row_lens();
        let pair = l0 + l1;
        let (jj, rem) = (2 * (f / pair), f % pair);
        let (j, k) = if rem < l0 { (jj, rem) } else { (jj + 1, rem - l0) };
        ((2 * k + j % 2) as i64, 3 * j as i64)
    }
    /// Lattice point → plate space (mm).
    #[inline]
    pub fn to_plate(&self, a: i64, b: i64) -> DVec2 {
        let (s, t) = (a as f64 * self.s_unit(), b as f64 * self.t_unit());
        if self.flat { DVec2::new(t, s) } else { DVec2::new(s, t) }
    }
    /// Plate size `(W, H)` in mm.
    pub fn size_mm(&self) -> (f64, f64) {
        let p = self.to_plate(self.m as i64, 3 * self.n as i64);
        (p.x, p.y)
    }
    /// Full-hex centre of facet `f` (the pivot, §4), plate space.
    pub fn centre(&self, f: u32) -> DVec2 {
        let (a, b) = self.centre_lattice(f);
        self.to_plate(a, b)
    }
    /// Full-hex corner offsets from the centre, plate space, CCW.
    pub fn corner_offsets(&self) -> [DVec2; 6] {
        let mut o = HEX_CORNERS.map(|(a, b)| self.to_plate(a, b));
        if self.flat {
            o.reverse();
        }
        o
    }
}

type LP = (i64, i64);

/// Clips a convex CCW lattice polygon to the half-plane `sign·(coord − k) ≥ 0`
/// (`axis` 0 = s, 1 = t). Intersections must be lattice points (they are for the
/// §6.2 lattice and its cut lines); anything else is an internal error.
fn clip_half(poly: &[LP], axis: usize, k: i64, sign: i64) -> Result<Vec<LP>> {
    let c = |p: &LP| if axis == 0 { p.0 } else { p.1 };
    let inside = |p: &LP| sign * (c(p) - k) >= 0;
    let mut out = Vec::with_capacity(poly.len() + 2);
    let n = poly.len();
    for i in 0..n {
        let (p, q) = (poly[i], poly[(i + 1) % n]);
        let (pin, qin) = (inside(&p), inside(&q));
        if pin {
            out.push(p);
        }
        if pin != qin {
            let (pc, qc) = (c(&p), c(&q));
            if pc == k || qc == k {
                // The crossing is an endpoint, already (or later) pushed as such.
                continue;
            }
            let (po, qo) = if axis == 0 { (p.1, q.1) } else { (p.0, q.0) };
            let num = (qo - po) * (k - pc);
            let den = qc - pc;
            if num % den != 0 {
                return Err(Error::Internal(format!("hex clip at {k} is not a lattice point")));
            }
            let o = po + num / den;
            out.push(if axis == 0 { (k, o) } else { (o, k) });
        }
    }
    Ok(out)
}

/// Removes repeated and collinear vertices of a convex lattice polygon.
fn clean_ring(mut r: Vec<LP>) -> Vec<LP> {
    loop {
        let n = r.len();
        if n < 3 {
            return Vec::new();
        }
        let mut drop = None;
        for i in 0..n {
            let (a, b, c) = (r[(i + n - 1) % n], r[i], r[(i + 1) % n]);
            let cross = (b.0 - a.0) * (c.1 - b.1) - (b.1 - a.1) * (c.0 - b.0);
            if b == a || cross == 0 {
                drop = Some(i);
                break;
            }
        }
        match drop {
            Some(i) => {
                r.remove(i);
            }
            None => return r,
        }
    }
}

/// §6.2 hex partition of the plate, with optional straight cuts (§14.1) along
/// lattice centre lines: `cuts_s` in half-cells (`0 < k < m`), `cuts_t` in `R/2`
/// units and multiples of 3 (`0 < k < 3n`). A hex crossed by a cut becomes one
/// piece per side, all carrying the hex's facet. Pieces are ordered by facet.
///
/// Without cuts every interior vertex has valence 3 and every outline vertex at
/// most 2 pieces, so `mesh_relief(PieceSet::All)` accepts it. A cut through a hex
/// adds interior vertices of valence 4 on the cut line; they become outline
/// vertices with 2 pieces when each tile is meshed on its own
/// (`PieceSet::Subset(partition.pieces_in_rect(..))`), which is how tiled plates
/// are meshed (§14.1).
pub fn hex_grid(lat: &HexLattice, cuts_s: &[i64], cuts_t: &[i64]) -> Result<Partition> {
    if lat.m < 2 || lat.n < 1 || !(lat.cell.is_finite() && lat.cell > 0.0) {
        return Err(Error::InvalidParams(format!("hex lattice {}×{} at cell {} is empty", lat.m, lat.n, lat.cell)));
    }
    let (m, tn) = (lat.m as i64, 3 * lat.n as i64);
    let mut cs: Vec<i64> = cuts_s.to_vec();
    cs.sort_unstable();
    cs.dedup();
    let mut ct: Vec<i64> = cuts_t.to_vec();
    ct.sort_unstable();
    ct.dedup();
    if cs.iter().any(|&k| k <= 0 || k >= m) {
        return Err(Error::InvalidParams("hex cut across the flats outside the plate".into()));
    }
    if ct.iter().any(|&k| k <= 0 || k >= tn || k % 3 != 0) {
        return Err(Error::InvalidParams("hex cut along the rows must be an interior row line".into()));
    }
    let cols: Vec<i64> = std::iter::once(0).chain(cs.iter().copied()).chain(std::iter::once(m)).collect();
    let rows: Vec<i64> = std::iter::once(0).chain(ct.iter().copied()).chain(std::iter::once(tn)).collect();

    let nf = lat.facet_count();
    // Dense lattice-point → vertex id table over s ∈ [0, m], t ∈ [0, 3n]: about
    // 6 entries per facet, less than the rings, and far faster than hashing.
    let (sw, th) = (m as usize + 1, tn as usize + 1);
    let mut vmap: Vec<u32> = vec![u32::MAX; sw * th];
    let mut lverts: Vec<LP> = Vec::with_capacity(nf as usize * 2 + 64);
    let mut rings: Vec<u32> = Vec::with_capacity(nf as usize * 6);
    let mut pieces: Vec<Piece> = Vec::with_capacity(nf as usize);
    let mut ids: Vec<u32> = Vec::with_capacity(8);
    let mut emit = |poly: &[LP], f: u32, rings: &mut Vec<u32>, pieces: &mut Vec<Piece>| {
        ids.clear();
        for &lp in poly {
            let slot = &mut vmap[lp.1 as usize * sw + lp.0 as usize];
            if *slot == u32::MAX {
                *slot = lverts.len() as u32;
                lverts.push(lp);
            }
            ids.push(*slot);
        }
        if lat.flat {
            ids.reverse();
        }
        let s = rings.len() as u32;
        rings.extend_from_slice(&ids);
        pieces.push(Piece { facet: f, ring: s..rings.len() as u32 });
    };
    for f in 0..nf {
        let (uc, vc) = lat.centre_lattice(f);
        let hex: [LP; 6] = HEX_CORNERS.map(|(a, b)| (uc + a, vc + b));
        // Fast path: a hex wholly inside one tile window needs no clipping (the
        // windows' interiors are disjoint, so it is in no other window).
        let ci = cols.partition_point(|&c| c < uc);
        let ri = rows.partition_point(|&r| r < vc - 1);
        if ci >= 1 && ci < cols.len() && cols[ci] > uc && ri >= 1 && ri < rows.len() && rows[ri] > vc + 1 {
            emit(&hex, f, &mut rings, &mut pieces);
            continue;
        }
        for cw in cols.windows(2) {
            if !(cw[0] < uc + 1 && cw[1] > uc - 1) {
                continue;
            }
            for rw in rows.windows(2) {
                if !(rw[0] < vc + 2 && rw[1] > vc - 2) {
                    continue;
                }
                let mut poly = clip_half(&hex, 0, cw[0], 1)?;
                poly = clip_half(&poly, 0, cw[1], -1)?;
                poly = clip_half(&poly, 1, rw[0], 1)?;
                poly = clip_half(&poly, 1, rw[1], -1)?;
                let poly = clean_ring(poly);
                if poly.len() < 3 {
                    continue;
                }
                emit(&poly, f, &mut rings, &mut pieces);
            }
        }
    }
    let verts: Vec<DVec2> = lverts.iter().map(|&(a, b)| lat.to_plate(a, b)).collect();
    // Outline: CCW in (s, t) along bottom, right, top, left.
    let mut sides: [Vec<(i64, u32)>; 4] = Default::default();
    for (id, &(a, b)) in lverts.iter().enumerate() {
        let id = id as u32;
        if b == 0 && a < m {
            sides[0].push((a, id));
        }
        if a == m && b < tn {
            sides[1].push((b, id));
        }
        if b == tn && a > 0 {
            sides[2].push((-a, id));
        }
        if a == 0 && b > 0 {
            sides[3].push((-b, id));
        }
    }
    let mut outline = Vec::with_capacity(sides.iter().map(Vec::len).sum());
    for side in sides.iter_mut() {
        side.sort_unstable();
        outline.extend(side.iter().map(|&(_, id)| id));
    }
    if lat.flat {
        outline.reverse();
    }
    Ok(Partition { verts, rings, pieces, outline, kind: PartitionKind::General })
}

#[cfg(test)]
mod hex_tests {
    use super::*;

    fn lat(m: u32, n: u32, flat: bool) -> HexLattice {
        HexLattice { m, n, cell: 6.0, flat }
    }

    fn total_area(p: &Partition) -> f64 {
        (0..p.pieces.len()).map(|k| p.piece_area(k)).sum()
    }

    #[test]
    fn facet_indexing_round_trips() {
        for &(m, n) in &[(2u32, 1u32), (3, 1), (7, 4), (8, 5), (2, 6)] {
            let l = lat(m, n, false);
            let mut f = 0;
            for j in 0..=n {
                let mut uc = j % 2;
                while uc <= m {
                    assert_eq!(l.facet_of(j, uc), f);
                    assert_eq!(l.centre_lattice(f), (uc as i64, 3 * j as i64));
                    f += 1;
                    uc += 2;
                }
            }
            assert_eq!(l.facet_count(), f);
        }
    }

    #[test]
    fn hex_partition_is_conforming() {
        let tol = Tol::default();
        for flat in [false, true] {
            for &(m, n) in &[(2u32, 1u32), (3, 1), (2, 2), (7, 4), (8, 5), (13, 9)] {
                let l = lat(m, n, flat);
                let p = hex_grid(&l, &[], &[]).unwrap();
                p.check(&tol).unwrap();
                let (w, h) = l.size_mm();
                assert!((p.outline_area() - w * h).abs() < 1e-6 * w * h, "outline area");
                assert!((total_area(&p) - w * h).abs() < 1e-6 * w * h, "piece areas");
                assert_eq!(p.pieces.len() as u32, l.facet_count());
                let mut val = vec![0u32; p.verts.len()];
                for &v in &p.rings {
                    val[v as usize] += 1;
                }
                let mut on_outline = vec![false; p.verts.len()];
                for &v in &p.outline {
                    on_outline[v as usize] = true;
                }
                for v in 0..p.verts.len() {
                    if on_outline[v] {
                        assert!(val[v] <= 2, "outline vertex with {} pieces", val[v]);
                    } else {
                        assert_eq!(val[v], 3);
                    }
                }
            }
        }
    }

    #[test]
    fn cuts_split_hexes_into_pieces_of_one_facet() {
        let tol = Tol::default();
        for flat in [false, true] {
            let l = lat(9, 6, flat);
            let p = hex_grid(&l, &[4, 5], &[9]).unwrap();
            p.check(&tol).unwrap();
            assert!(p.pieces.len() as u32 > l.facet_count());
            let (w, h) = l.size_mm();
            assert!((total_area(&p) - w * h).abs() < 1e-6 * w * h);
        }
        let l = lat(9, 6, false);
        assert!(hex_grid(&l, &[0], &[]).is_err());
        assert!(hex_grid(&l, &[], &[4]).is_err());
    }
}
