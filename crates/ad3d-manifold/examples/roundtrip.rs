//! Times the FFI round trip on a large closed relief plate (about 1M
//! triangles by default): MeshGL in, Manifold import (validation), a boolean
//! that drills holes, and MeshGL out.
//!
//!     cargo run --release --example roundtrip -- [grid=707] [holes=16]

use std::time::Instant;

use ad3d_manifold::{MANIFOLD_VERSION, Manifold, MeshGL, Status};

/// A closed plate shaped like a Squama/lithophane part: an n x n height-field
/// top over a 100 x 100 mm base, side walls, and a flat back made as a fan
/// from the centre to the boundary (as in docs/design/squama-mesher.md §7).
/// 2 n^2 + 12 n triangles, counter-clockwise seen from outside.
fn relief_plate(n: u32) -> (Vec<f32>, Vec<u32>) {
    let size = 100.0f32;
    let row = n + 1;
    let top = |i: u32, j: u32| j * row + i;
    let xy = |i: u32, j: u32| (size * i as f32 / n as f32, size * j as f32 / n as f32);

    let mut pos = Vec::with_capacity(((row * row + 4 * n + 1) * 3) as usize);
    for j in 0..row {
        for i in 0..row {
            let (x, y) = xy(i, j);
            pos.extend_from_slice(&[x, y, 3.0 + 0.6 * (x * 0.37).sin() * (y * 0.23).cos()]);
        }
    }
    // Boundary loop, counter-clockwise seen from +z.
    let mut ring = Vec::with_capacity(4 * n as usize);
    ring.extend((0..n).map(|i| (i, 0)));
    ring.extend((0..n).map(|j| (n, j)));
    ring.extend((1..=n).rev().map(|i| (i, n)));
    ring.extend((1..=n).rev().map(|j| (0, j)));
    let bottom0 = row * row;
    for &(i, j) in &ring {
        let (x, y) = xy(i, j);
        pos.extend_from_slice(&[x, y, 0.0]);
    }
    let centre = bottom0 + ring.len() as u32;
    pos.extend_from_slice(&[size / 2.0, size / 2.0, 0.0]);

    let mut tri = Vec::with_capacity(((2 * n * n + 12 * n) * 3) as usize);
    for j in 0..n {
        for i in 0..n {
            let (a, b, c, d) = (top(i, j), top(i + 1, j), top(i + 1, j + 1), top(i, j + 1));
            tri.extend_from_slice(&[a, b, c, a, c, d]);
        }
    }
    let m = ring.len() as u32;
    for k in 0..m {
        let k1 = (k + 1) % m;
        let (t0, t1) = (top(ring[k as usize].0, ring[k as usize].1), top(ring[k1 as usize].0, ring[k1 as usize].1));
        let (b0, b1) = (bottom0 + k, bottom0 + k1);
        tri.extend_from_slice(&[b0, b1, t1, b0, t1, t0]); // wall, outward
        tri.extend_from_slice(&[centre, b1, b0]); // back face, -z
    }
    (pos, tri)
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

fn main() {
    let mut args = std::env::args().skip(1);
    let n: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(707);
    let holes: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(16);

    println!("manifold {MANIFOLD_VERSION}, arch {}", std::env::consts::ARCH);
    let (pos, tri) = relief_plate(n);
    println!("plate: {} vertices, {} triangles", pos.len() / 3, tri.len() / 3);

    let t = Instant::now();
    let mesh = MeshGL::new(&pos, &tri).unwrap();
    let t_meshgl = ms(t);

    let t = Instant::now();
    let plate = Manifold::from_meshgl(&mesh);
    let status = plate.status();
    let t_import = ms(t);
    assert_eq!(status, Status::NoError);
    let volume = plate.volume();
    assert!(volume > 0.0, "outward orientation gives a positive volume");

    let side = (holes as f64).sqrt().ceil() as u32;
    let mut drills = Manifold::empty();
    for h in 0..holes {
        let (i, j) = (h % side, h / side);
        let at = [(i as f64 + 0.5) * 100.0 / side as f64, (j as f64 + 0.5) * 100.0 / side as f64, -1.0];
        drills = drills.union(&Manifold::cylinder(10.0, 1.5, 1.5, 32, false).translate(at));
    }
    let t = Instant::now();
    let drilled = plate.difference(&drills);
    let status = drilled.status(); // forces evaluation
    let t_bool = ms(t);
    assert_eq!(status, Status::NoError);

    let t = Instant::now();
    let out = drilled.to_meshgl();
    let out_pos = out.positions();
    let out_tri = out.tri_verts();
    let t_out = ms(t);

    println!("MeshGL::new (copy in)      {t_meshgl:8.1} ms");
    println!("Manifold::from_meshgl      {t_import:8.1} ms  (volume {volume:.1} mm3)");
    println!("difference ({holes} holes)     {t_bool:8.1} ms  (genus {})", drilled.genus());
    println!("to_meshgl + copy out       {t_out:8.1} ms  ({} verts, {} tris)", out_pos.len() / 3, out_tri.len() / 3);
}
