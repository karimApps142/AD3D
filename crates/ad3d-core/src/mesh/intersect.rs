//! Triangle-triangle self-intersection check (audit L1).
//!
//! `validate()`'s other checks are combinatorial (closed, consistently wound,
//! unpinched) plus zero area and signed volume; none of them sees two triangles
//! that pass through each other. This stage does: Shewchuk-style statically
//! filtered `orient3d`/`orient2d` (an uncertain sign counts as touching, never as
//! crossing), coplanar overlap, and folds about a shared vertex or edge, with a
//! uniform-grid broad phase of mean-edge-length cells.
//!
//! Cost is roughly linear in the triangle count for well-shaped meshes, but much
//! slower than the rest of `validate()`, so it is opt-in
//! (`ValidateOpts::check_self_intersection`). Degenerate or out-of-range
//! triangles are skipped here; the topology stage reports them.

use std::collections::HashMap;

use super::{FaceKind, Mesh};

const EPS: f64 = f64::EPSILON * 0.5;

/// Sign of orient3d(a, b, c, d) with Shewchuk's static error bound; 0 when the
/// sign cannot be certified (treated as "touching", never as crossing).
fn orient3d(a: [f64; 3], b: [f64; 3], c: [f64; 3], d: [f64; 3]) -> i32 {
    let (adx, ady, adz) = (a[0] - d[0], a[1] - d[1], a[2] - d[2]);
    let (bdx, bdy, bdz) = (b[0] - d[0], b[1] - d[1], b[2] - d[2]);
    let (cdx, cdy, cdz) = (c[0] - d[0], c[1] - d[1], c[2] - d[2]);
    let det = adx * (bdy * cdz - bdz * cdy) + bdx * (cdy * adz - cdz * ady) + cdx * (ady * bdz - adz * bdy);
    let perm = adx.abs() * ((bdy * cdz).abs() + (bdz * cdy).abs())
        + bdx.abs() * ((cdy * adz).abs() + (cdz * ady).abs())
        + cdx.abs() * ((ady * bdz).abs() + (adz * bdy).abs());
    let bound = (7.0 + 56.0 * EPS) * EPS * perm;
    if det > bound {
        1
    } else if det < -bound {
        -1
    } else {
        0
    }
}

fn orient2d(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> i32 {
    let l = (a[0] - c[0]) * (b[1] - c[1]);
    let r = (a[1] - c[1]) * (b[0] - c[0]);
    let det = l - r;
    let bound = (3.0 + 16.0 * EPS) * EPS * (l.abs() + r.abs());
    if det > bound {
        1
    } else if det < -bound {
        -1
    } else {
        0
    }
}

/// Segment pq crosses the interior of triangle abc properly.
fn seg_cross_tri(p: [f64; 3], q: [f64; 3], a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> bool {
    let sp = orient3d(a, b, c, p);
    let sq = orient3d(a, b, c, q);
    if sp == 0 || sq == 0 || sp == sq {
        return false;
    }
    let s1 = orient3d(p, q, a, b);
    let s2 = orient3d(p, q, b, c);
    let s3 = orient3d(p, q, c, a);
    s1 != 0 && s1 == s2 && s2 == s3
}

fn proj(p: [f64; 3], drop: usize) -> [f64; 2] {
    match drop {
        0 => [p[1], p[2]],
        1 => [p[0], p[2]],
        _ => [p[0], p[1]],
    }
}

/// Interiors of two coplanar triangles overlap (2D, after dropping `drop`).
fn coplanar_overlap(t1: [[f64; 3]; 3], t2: [[f64; 3]; 3], shared: &[(usize, usize)], drop: usize) -> bool {
    let a = t1.map(|p| proj(p, drop));
    let b = t2.map(|p| proj(p, drop));
    let s1 = orient2d(a[0], a[1], a[2]);
    let s2 = orient2d(b[0], b[1], b[2]);
    if s1 == 0 || s2 == 0 {
        return false;
    }
    let strictly_inside = |t: &[[f64; 2]; 3], s: i32, p: [f64; 2]| {
        (0..3).all(|k| orient2d(t[k], t[(k + 1) % 3], p) == s)
    };
    let is_shared1 = |k: usize| shared.iter().any(|&(x, _)| x == k);
    let is_shared2 = |k: usize| shared.iter().any(|&(_, y)| y == k);
    // Proper edge-edge crossings.
    for i in 0..3 {
        for j in 0..3 {
            let (p, q) = (a[i], a[(i + 1) % 3]);
            let (r, s) = (b[j], b[(j + 1) % 3]);
            let o1 = orient2d(p, q, r);
            let o2 = orient2d(p, q, s);
            let o3 = orient2d(r, s, p);
            let o4 = orient2d(r, s, q);
            if o1 != 0 && o2 != 0 && o3 != 0 && o4 != 0 && o1 != o2 && o3 != o4 {
                return true;
            }
        }
    }
    for k in 0..3 {
        if !is_shared2(k) && strictly_inside(&a, s1, b[k]) {
            return true;
        }
        if !is_shared1(k) && strictly_inside(&b, s2, a[k]) {
            return true;
        }
    }
    // Shared vertex: wedges overlap if an edge of one leaves the shared vertex
    // strictly inside the other's angle.
    if shared.len() == 1 {
        let (i, j) = shared[0];
        let v = a[i];
        for k in 1..3 {
            let c = b[(j + k) % 3];
            let (p, q) = (a[(i + 1) % 3], a[(i + 2) % 3]);
            if orient2d(v, p, c) == s1 && orient2d(q, v, c) == s1 {
                return true;
            }
            let c = a[(i + k) % 3];
            let (p, q) = (b[(j + 1) % 3], b[(j + 2) % 3]);
            if orient2d(v, p, c) == s2 && orient2d(q, v, c) == s2 {
                return true;
            }
        }
    }
    // Shared edge: folded onto itself if the third vertices are on the same side.
    if shared.len() == 2 {
        let (i0, j0) = shared[0];
        let (i1, _) = shared[1];
        let third1 = 3 - i0 - i1;
        let j1 = shared[1].1;
        let third2 = 3 - j0 - j1;
        let (u, v) = (a[i0], a[i1]);
        let o1 = orient2d(u, v, a[third1]);
        let o2 = orient2d(u, v, b[third2]);
        if o1 != 0 && o1 == o2 {
            return true;
        }
    }
    false
}

/// Number of intersecting triangle pairs in a mesh (beyond shared vertices and
/// shared edges) and up to `max_examples` descriptions. Vertices are matched by
/// index, so weld a soup first (see [`self_intersections_indexed`]).
pub fn self_intersections(m: &Mesh, max_examples: usize) -> (usize, Vec<String>) {
    let pos: Vec<[f64; 3]> = m.positions.chunks_exact(3).map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]).collect();
    let tri: Vec<[u32; 3]> = (0..m.triangle_count()).map(|t| m.triangle(t)).collect();
    self_intersections_indexed(&pos, &tri, &m.face_kind, max_examples)
}

/// [`self_intersections`] on raw positions and triangles. `face_kind` may be empty.
pub fn self_intersections_indexed(
    pos: &[[f64; 3]],
    tri: &[[u32; 3]],
    face_kind: &[FaceKind],
    max_examples: usize,
) -> (usize, Vec<String>) {
    let ntri = tri.len();
    let usable = |t: &[u32; 3]| {
        t.iter().all(|&v| (v as usize) < pos.len() && pos[v as usize].iter().all(|x| x.is_finite()))
            && t[0] != t[1]
            && t[1] != t[2]
            && t[0] != t[2]
    };
    // Bounding boxes; NaN boxes never overlap anything, so skipped triangles drop out.
    let mut boxes = Vec::with_capacity(ntri);
    let (mut tot, mut nused) = (0.0, 0usize);
    for t in tri {
        if !usable(t) {
            boxes.push(([f64::NAN; 3], [f64::NAN; 3]));
            continue;
        }
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for k in 0..3 {
            let (p, q) = (pos[t[k] as usize], pos[t[(k + 1) % 3] as usize]);
            tot += ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt();
            for d in 0..3 {
                lo[d] = lo[d].min(p[d]);
                hi[d] = hi[d].max(p[d]);
            }
        }
        boxes.push((lo, hi));
        nused += 1;
    }
    // Broad phase: grid of mean-edge-length cells, coarsened while long triangles
    // (back-face diagonals, big walls) would make too many cell entries.
    let mut cell = (tot / (3.0 * nused.max(1) as f64)).max(1e-3);
    let budget = 64.0 * nused as f64 + 1e6;
    loop {
        let inv = 1.0 / cell;
        let entries: f64 = boxes
            .iter()
            .filter(|b| b.0[0].is_finite())
            .map(|(lo, hi)| (0..3).map(|k| (hi[k] * inv).floor() - (lo[k] * inv).floor() + 1.0).product::<f64>())
            .sum();
        if entries <= budget {
            break;
        }
        cell *= 2.0;
    }
    let inv = 1.0 / cell;
    // Each triangle goes only into the cells its own surface touches (a padded
    // separating-axis test), not every cell of its bounding box: long thin back-face
    // and wall triangles have huge, heavily overlapping boxes, and bbox insertion
    // made a 78k-triangle plate take a minute.
    let mut grid: HashMap<[i64; 3], Vec<u32>> = HashMap::new();
    for (ti, (lo, hi)) in boxes.iter().enumerate() {
        if !lo[0].is_finite() {
            continue;
        }
        let tp = tri[ti].map(|v| pos[v as usize]);
        let c0 = lo.map(|x| (x * inv).floor() as i64);
        let c1 = hi.map(|x| (x * inv).floor() as i64);
        let single = c0 == c1;
        for x in c0[0]..=c1[0] {
            for y in c0[1]..=c1[1] {
                for z in c0[2]..=c1[2] {
                    if single || tri_touches_cell(&tp, [x, y, z], cell) {
                        grid.entry([x, y, z]).or_default().push(ti as u32);
                    }
                }
            }
        }
    }
    let mut pairs: Vec<(u32, u32)> = Vec::new();
    for list in grid.values() {
        for (ii, &t1) in list.iter().enumerate() {
            for &t2 in &list[ii + 1..] {
                let (a, b) = if t1 < t2 { (t1, t2) } else { (t2, t1) };
                let (b1, b2) = (boxes[a as usize], boxes[b as usize]);
                if (0..3).any(|k| b1.0[k] > b2.1[k] || b2.0[k] > b1.1[k]) {
                    continue;
                }
                pairs.push((a, b));
            }
        }
    }
    pairs.sort_unstable();
    pairs.dedup();
    let mut count = 0;
    let mut ex = Vec::new();
    for (a, b) in pairs {
        if tri_pair_intersects(pos, tri[a as usize], tri[b as usize]) {
            count += 1;
            if ex.len() < max_examples {
                let fp = |t: [u32; 3]| t.map(|v| pos[v as usize].map(|x| (x * 1e4).round() / 1e4));
                ex.push(format!(
                    "tri {a} {:?} kind {:?} x tri {b} {:?} kind {:?}",
                    fp(tri[a as usize]),
                    face_kind.get(a as usize),
                    fp(tri[b as usize]),
                    face_kind.get(b as usize)
                ));
            }
        }
    }
    (count, ex)
}

fn tri_pair_intersects(pos: &[[f64; 3]], t1: [u32; 3], t2: [u32; 3]) -> bool {
    let mut shared = Vec::new();
    for i in 0..3 {
        for j in 0..3 {
            if t1[i] == t2[j] {
                shared.push((i, j));
            }
        }
    }
    let p1 = t1.map(|v| pos[v as usize]);
    let p2 = t2.map(|v| pos[v as usize]);
    // Coplanar?
    let o: Vec<i32> = (0..3).map(|k| orient3d(p1[0], p1[1], p1[2], p2[k])).collect();
    let coplanar = o.iter().all(|&s| s == 0);
    if coplanar {
        let n = {
            let (u, v) = (
                [p1[1][0] - p1[0][0], p1[1][1] - p1[0][1], p1[1][2] - p1[0][2]],
                [p1[2][0] - p1[0][0], p1[2][1] - p1[0][1], p1[2][2] - p1[0][2]],
            );
            [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
        };
        let drop = (0..3).max_by(|&x, &y| n[x].abs().partial_cmp(&n[y].abs()).unwrap()).unwrap();
        return coplanar_overlap(p1, p2, &shared, drop);
    }
    match shared.len() {
        0 => (0..3).any(|k| seg_cross_tri(p1[k], p1[(k + 1) % 3], p2[0], p2[1], p2[2]))
            || (0..3).any(|k| seg_cross_tri(p2[k], p2[(k + 1) % 3], p1[0], p1[1], p1[2])),
        1 => {
            let (i, j) = shared[0];
            seg_cross_tri(p1[(i + 1) % 3], p1[(i + 2) % 3], p2[0], p2[1], p2[2])
                || seg_cross_tri(p2[(j + 1) % 3], p2[(j + 2) % 3], p1[0], p1[1], p1[2])
        }
        _ => false,
    }
}

/// Conservative triangle / grid-cell overlap (Akenine-Möller's separating-axis
/// test on a cell padded by 1e-6 of its size): may say "touches" for a near miss,
/// never "misses" for a touch, so no candidate pair is lost.
fn tri_touches_cell(t: &[[f64; 3]; 3], c: [i64; 3], cell: f64) -> bool {
    let h = cell * (0.5 + 1e-6);
    let centre = [(c[0] as f64 + 0.5) * cell, (c[1] as f64 + 0.5) * cell, (c[2] as f64 + 0.5) * cell];
    let v = t.map(|p| [p[0] - centre[0], p[1] - centre[1], p[2] - centre[2]]);
    let e = [sub3(v[1], v[0]), sub3(v[2], v[1]), sub3(v[0], v[2])];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    // The 9 edge × box-axis axes.
    for ei in e {
        for k in 0..3 {
            let mut axis = [0.0; 3];
            // axis = unit_k × ei
            axis[(k + 1) % 3] = -ei[(k + 2) % 3];
            axis[(k + 2) % 3] = ei[(k + 1) % 3];
            let r = h * (axis[0].abs() + axis[1].abs() + axis[2].abs());
            let p = v.map(|q| dot(q, axis));
            let (mn, mx) = (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
            let slack = 1e-9 * (mn.abs().max(mx.abs()) + r);
            if mn > r + slack || mx < -r - slack {
                return false;
            }
        }
    }
    // The 3 box axes (the caller's bbox loop already covers these, but cheap).
    for k in 0..3 {
        let (mn, mx) = (v[0][k].min(v[1][k]).min(v[2][k]), v[0][k].max(v[1][k]).max(v[2][k]));
        if mn > h || mx < -h {
            return false;
        }
    }
    // The triangle's plane.
    let n = cross3(e[0], e[1]);
    let r = h * (n[0].abs() + n[1].abs() + n[2].abs());
    let d = dot(n, v[0]);
    let slack = 1e-9 * (d.abs() + r);
    d.abs() <= r + slack
}

#[inline]
fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[inline]
fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brute(pos: &[[f64; 3]], tri: &[[u32; 3]]) -> usize {
        let mut n = 0;
        for a in 0..tri.len() {
            for b in a + 1..tri.len() {
                if tri_pair_intersects(pos, tri[a], tri[b]) {
                    n += 1;
                }
            }
        }
        n
    }

    /// The grid broad phase (cell-touch insertion, budget coarsening) finds exactly
    /// the pairs an all-pairs search finds, on random soups with long thin triangles.
    #[test]
    fn grid_matches_brute_force() {
        let mut seed = 0x2545F4914F6CDD1Du64;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut total = 0;
        for case in 0..60 {
            let n = 20 + case * 3;
            let mut pos = Vec::new();
            let mut tri = Vec::new();
            for k in 0..n {
                let base = [rnd() * 20.0, rnd() * 20.0, rnd() * 4.0];
                let long = k % 4 == 0;
                let s = if long { 15.0 } else { 2.0 };
                let i = pos.len() as u32;
                pos.push(base);
                pos.push([base[0] + (rnd() - 0.5) * s, base[1] + (rnd() - 0.5) * s, base[2] + (rnd() - 0.5) * 0.5]);
                pos.push([base[0] + (rnd() - 0.5) * 0.3, base[1] + (rnd() - 0.5) * 0.3, base[2] + (rnd() - 0.5) * 2.0]);
                tri.push([i, i + 1, i + 2]);
            }
            let want = brute(&pos, &tri);
            let (got, _) = self_intersections_indexed(&pos, &tri, &[], 0);
            assert_eq!(got, want, "case {case}");
            total += want;
        }
        assert!(total > 50, "random soups should intersect sometimes ({total})");
    }
}
