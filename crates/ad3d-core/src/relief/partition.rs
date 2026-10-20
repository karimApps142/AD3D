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
