//! Synthetic meshes for transfer benchmarks (spike C4).

use crate::mesh::{FaceKind, Mesh};

/// A closed, consistently wound rippled torus with at least `min_tris` triangles
/// (exactly `2·m·n`, genus 1, Euler 0). About 100 mm across.
pub fn torus(min_tris: usize) -> Mesh {
    let quads = min_tris.div_ceil(2).max(12);
    // m around the ring, n around the tube, m ≈ 2n.
    let mut n = ((quads as f64 / 2.0).sqrt().ceil() as usize).max(3);
    let mut m = quads.div_ceil(n).max(3);
    if m * n < quads {
        n += 1;
        m = quads.div_ceil(n);
    }
    let (big_r, small_r) = (40.0f64, 12.0f64);
    let mut mesh = Mesh::with_capacity(m * n, 2 * m * n);
    let tau = std::f64::consts::TAU;
    for i in 0..m {
        let u = tau * i as f64 / m as f64;
        for j in 0..n {
            let v = tau * j as f64 / n as f64;
            let r = small_r * (1.0 + 0.05 * (7.0 * u).sin() * (5.0 * v).cos());
            let x = (big_r + r * v.cos()) * u.cos();
            let y = (big_r + r * v.cos()) * u.sin();
            let z = r * v.sin();
            mesh.push_vertex([x as f32, y as f32, z as f32]);
        }
    }
    let id = |i: usize, j: usize| ((i % m) * n + (j % n)) as u32;
    for i in 0..m {
        for j in 0..n {
            let (a, b, c, d) = (id(i, j), id(i + 1, j), id(i + 1, j + 1), id(i, j + 1));
            mesh.push_triangle([a, b, c], FaceKind::Other);
            mesh.push_triangle([a, c, d], FaceKind::Other);
        }
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::validate::{ValidateOpts, validate};

    #[test]
    fn torus_is_closed_genus_one() {
        let m = torus(5000);
        assert!(m.triangle_count() >= 5000);
        let opts = ValidateOpts { expect_euler: Some(0), min_distance: None, ..Default::default() };
        let r = validate(&m, &opts);
        assert!(r.ok, "{}", r.summary());
    }
}
