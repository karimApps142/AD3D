//! Prints the three spike checks: a valid cube, cube minus cylinder volume,
//! and a broken mesh rejected with a status.
//!
//!     cargo run --release --example smoke

use ad3d_manifold::{MANIFOLD_VERSION, Manifold, MeshGL};

fn main() {
    #[rustfmt::skip]
    let p: Vec<f32> = vec![
        0.0, 0.0, 0.0,  20.0, 0.0, 0.0,  20.0, 20.0, 0.0,  0.0, 20.0, 0.0,
        0.0, 0.0, 20.0, 20.0, 0.0, 20.0, 20.0, 20.0, 20.0, 0.0, 20.0, 20.0,
    ];
    #[rustfmt::skip]
    let t: Vec<u32> = vec![
        0, 2, 1, 0, 3, 2,  4, 5, 6, 4, 6, 7,  0, 1, 5, 0, 5, 4,
        2, 3, 7, 2, 7, 6,  1, 2, 6, 1, 6, 5,  3, 0, 4, 3, 4, 7,
    ];
    println!("manifold {MANIFOLD_VERSION} on {}", std::env::consts::ARCH);

    let cube = Manifold::from_meshgl(&MeshGL::new(&p, &t).unwrap());
    println!(
        "cube:            status={} volume={} tris={} genus={}",
        cube.status(),
        cube.volume(),
        cube.num_tri(),
        cube.genus()
    );

    let n = 64;
    let part = cube
        .translate([-10.0, -10.0, -10.0])
        .difference(&Manifold::cylinder(40.0, 5.0, 5.0, n, true));
    let expected = 8000.0 - 0.5 * n as f64 * 25.0 * (2.0 * std::f64::consts::PI / n as f64).sin() * 20.0;
    println!(
        "cube - cylinder: status={} volume={:.6} expected={:.6} rel.err={:.1e} tris={} genus={}",
        part.status(),
        part.volume(),
        expected,
        (part.volume() - expected).abs() / expected,
        part.num_tri(),
        part.genus()
    );

    let broken = Manifold::from_meshgl(&MeshGL::new(&p, &t[..t.len() - 3]).unwrap());
    println!(
        "missing tri:     status={} is_empty={}",
        broken.status(),
        broken.is_empty()
    );
}
