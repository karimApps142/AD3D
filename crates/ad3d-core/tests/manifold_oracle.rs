//! Independent oracle: manifold 3.5.4 must accept every generated Squama plate as a
//! valid 2-manifold (status NoError) without merging, with the same volume and genus 0.
//! Run with `cargo test -p ad3d-core --features manifold-oracle --release --test manifold_oracle`.

use ad3d_core::CancelToken;
use ad3d_core::image::CellBrightness;
use ad3d_core::squama::{self, GenOptions, ImageSource, SquamaParams};
use ad3d_manifold::{Manifold, MeshGL, Status};

fn check(name: &str, mesh: &ad3d_core::Mesh) {
    let gl = MeshGL::new(&mesh.positions, &mesh.indices).expect("MeshGL");
    let m = Manifold::from_meshgl(&gl);
    let status = m.status();
    assert_eq!(status, Status::NoError, "{name}: manifold rejected the plate ({status:?})");
    assert!(!m.is_empty(), "{name}: empty manifold");
    assert_eq!(m.genus(), 0, "{name}: genus");
    assert_eq!(m.num_tri(), mesh.triangle_count(), "{name}: triangle count changed on import");
    let (ours, theirs) = (mesh.signed_volume(), m.volume());
    assert!(
        ((ours - theirs) / ours).abs() < 1e-6,
        "{name}: volume {theirs} vs signed volume {ours}"
    );
}

fn synthetic(p: &SquamaParams) -> ad3d_core::Mesh {
    squama::run(p, ImageSource::Synthetic, &GenOptions::default(), &CancelToken::new()).unwrap().mesh
}

#[test]
fn luban_default_plate() {
    check("50x63 synthetic", &synthetic(&SquamaParams::default()));
}

#[test]
fn spike_plate_100x126() {
    check("100x126 synthetic", &synthetic(&SquamaParams { cell_mm: 3.0, ..SquamaParams::default() }));
}

/// Deterministic xorshift so the test needs no extra dependency.
fn rng(seed: &mut u64) -> f64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    (*seed >> 11) as f64 / (1u64 << 53) as f64
}

#[test]
fn random_params_and_noise_images() {
    let mut s = 0x9E37_79B9_7F4A_7C15u64;
    let mut checked = 0;
    for case in 0..24 {
        let p = SquamaParams {
            width_mm: 40.0 + 200.0 * rng(&mut s),
            height_mm: None,
            cell_mm: [3.0, 4.5, 6.0, 7.3][case % 4],
            light_azimuth: std::f64::consts::TAU * rng(&mut s),
            light_elevation: (2.0 + 40.0 * rng(&mut s)).to_radians(),
            max_tilt: (3.0 + 17.0 * rng(&mut s)).to_radians(),
            base_mm: 0.6 + 4.0 * rng(&mut s),
        };
        let l = squama::layout(&p, (4, 5)).unwrap();
        let g = (0..l.nx * l.ny).map(|_| rng(&mut s) as f32).collect();
        let cb = CellBrightness { nx: l.nx, ny: l.ny, g };
        let out = squama::generate_ex(&p, &cb, &GenOptions::default(), &CancelToken::new());
        let Ok(out) = out else { continue }; // rejected params are covered elsewhere
        check(&format!("case {case} {}x{} {p:?}", l.nx, l.ny), &out.mesh);
        checked += 1;
    }
    assert!(checked >= 16, "only {checked} of 24 random cases produced a plate");
}
