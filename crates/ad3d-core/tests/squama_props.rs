//! Squama generator: fixed cases, images, extreme params, and property tests
//! (validate passes in all three stages, bounds and volume are sane, deterministic).

mod common;

use std::f64::consts::PI;

use ad3d_core::CancelToken;
use ad3d_core::error::Error;
use ad3d_core::image::{CellBrightness, LinearLuma, cell_brightness, synthetic_target};
use ad3d_core::mesh::validate::{ValidateOpts, validate};
use ad3d_core::orient::{Orientation, QuarterTurns, export_xform};
use ad3d_core::relief::Tol;
use ad3d_core::squama::{
    GenOptions, ImageSource, SYNTHETIC_PX, SquamaOutput, SquamaParams, e_max, generate, generate_ex, run, validate_opts,
};
use common::fingerprint;
use proptest::prelude::*;

fn params(nx: u32, cell_um: i64) -> SquamaParams {
    SquamaParams { width_mm: (nx as i64 * cell_um) as f64 / 1000.0, cell_mm: cell_um as f64 / 1000.0, ..Default::default() }
}

fn no_guard() -> GenOptions {
    GenOptions { relief_guard: false, ..Default::default() }
}

/// Validates in plate space and in a standing orientation; checks bounds and volume.
fn check(p: &SquamaParams, out: &SquamaOutput, quarter: QuarterTurns) {
    // Plate space with the self-intersection stage on (audit L1).
    let r = validate(
        &out.mesh,
        &ValidateOpts { check_self_intersection: true, ..validate_opts(out, ad3d_core::orient::Xform::IDENTITY) },
    );
    assert!(r.ok, "plate-space validate failed for {p:?}:\n{}", r.summary());
    assert_eq!(r.self_intersecting_pairs, Some(0));
    let (x, _) = export_xform(Orientation::Standing(quarter), out.layout.w_mm, out.layout.h_mm, p.light_azimuth);
    let r2 = validate(&out.mesh, &validate_opts(out, x));
    assert!(r2.ok, "standing validate failed for {p:?}:\n{}", r2.summary());

    let tol = Tol::default();
    let el = p.light_elevation.to_degrees().max(1.0).to_radians();
    let _ = el;
    let b = out.mesh.bounds().unwrap();
    let (w, h) = (out.layout.w_mm, out.layout.h_mm);
    let c = p.base_mm + e_max(out.layout.cell_mm, p.light_azimuth) * p.max_tilt.tan() + tol.q;
    let zmax = c + e_max(out.layout.cell_mm, p.light_azimuth) * p.max_tilt.tan() + (tol.tau_q as f64 + 1.0) * tol.q;
    assert_eq!(b.min, [0.0, 0.0, 0.0]);
    assert!((b.max[0] as f64 - w).abs() < 1e-4 && (b.max[1] as f64 - h).abs() < 1e-4, "{b:?} vs {w}x{h}");
    assert!((b.max[2] as f64) <= zmax + 1e-4, "zmax {} > {zmax}", b.max[2]);
    let vol = r.written.signed_volume;
    assert!(vol >= w * h * p.base_mm * (1.0 - 1e-6), "volume {vol} below W·H·T");
    assert!(vol <= w * h * zmax * (1.0 + 1e-6), "volume {vol} above W·H·zmax");
}

fn gen_ok(p: &SquamaParams, cb: &CellBrightness) -> SquamaOutput {
    generate_ex(p, cb, &no_guard(), &CancelToken::new()).unwrap_or_else(|e| panic!("{e} for {p:?}"))
}

#[test]
fn luban_default_synthetic() {
    let p = SquamaParams::default();
    let out = run(&p, ImageSource::Synthetic, &GenOptions::default(), &CancelToken::new()).unwrap();
    assert_eq!((out.layout.nx, out.layout.ny), (50, 63));
    assert_eq!(out.mesh.triangle_count(), 2 * out.mesh.vertex_count() - 4);
    check(&p, &out, QuarterTurns::Auto);
}

#[test]
fn hundred_by_126() {
    let p = SquamaParams { cell_mm: 3.0, ..Default::default() };
    let out = run(&p, ImageSource::Synthetic, &GenOptions::default(), &CancelToken::new()).unwrap();
    assert_eq!((out.layout.nx, out.layout.ny), (100, 126));
    check(&p, &out, QuarterTurns::Auto);
}

#[test]
fn image_kinds() {
    let (nx, ny) = (12u32, 9u32);
    let p = params(nx, 6000);
    let n = (nx * ny) as usize;
    let mut rng = 12345u64;
    let mut rnd = || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng >> 11) as f32 / (1u64 << 53) as f32
    };
    let kinds: Vec<(&str, Vec<f32>)> = vec![
        ("black", vec![0.0; n]),
        ("white", vec![1.0; n]),
        ("checker", (0..n).map(|k| ((k as u32 % nx + k as u32 / nx) % 2) as f32).collect()),
        ("noise", (0..n).map(|_| rnd()).collect()),
        ("near-grey", (0..n).map(|_| 0.5 + 1e-4 * (rnd() - 0.5)).collect()),
        ("nan", (0..n).map(|k| if k % 3 == 0 { f32::NAN } else { 0.7 }).collect()),
    ];
    for (name, g) in kinds {
        let cb = CellBrightness { nx, ny, g };
        let out = gen_ok(&p, &cb);
        eprintln!("{name}: saddles {} crossings {}", out.stats.mesh.saddles, out.stats.mesh.crossings);
        check(&p, &out, QuarterTurns::Auto);
    }
}

#[test]
fn one_by_n_and_n_by_one() {
    for (nx, ny) in [(1u32, 9u32), (9, 1), (1, 1)] {
        let p = params(nx, 4000);
        let cb = CellBrightness { nx, ny, g: (0..nx * ny).map(|k| (k % 3) as f32 / 2.0).collect() };
        check(&p, &gen_ok(&p, &cb), QuarterTurns::Q0);
    }
}

#[test]
fn extreme_params() {
    let (nx, ny) = (8u32, 7u32);
    let g: Vec<f32> = (0..nx * ny).map(|k| ((k * 37) % 11) as f32 / 10.0).collect();
    let cases = [
        (500, 1.0, 1.0, 0.4, 0.0),    // smallest cell, tilt and elevation
        (500, 45.0, 20.0, 0.4, 45.0), // steepest
        (10000, 1.0, 20.0, 5.0, 123.0),
        (10000, 45.0, 1.0, 0.4, 270.0),
        (3000, 0.0, 10.0, 3.0, 90.0), // elevation 0 → raised to 1°
    ];
    for (cell_um, el, tilt, base, az) in cases {
        let mut p = params(nx, cell_um);
        p.light_elevation = (el as f64).to_radians();
        p.max_tilt = (tilt as f64).to_radians();
        p.base_mm = base;
        p.light_azimuth = (az as f64).to_radians();
        let cb = CellBrightness { nx, ny, g: g.clone() };
        check(&p, &gen_ok(&p, &cb), QuarterTurns::Auto);
    }
}

#[test]
fn large_plate_2048() {
    let p = SquamaParams { width_mm: 2048.0, cell_mm: 32.0, ..Default::default() };
    let cb = CellBrightness { nx: 64, ny: 64, g: (0..64 * 64).map(|k| ((k * 7919) % 101) as f32 / 100.0).collect() };
    check(&p, &gen_ok(&p, &cb), QuarterTurns::Auto);
}

#[test]
fn invalid_params_are_rejected() {
    let cb = CellBrightness::uniform(50, 63, 0.5);
    let bad = [
        SquamaParams { cell_mm: 0.3, ..Default::default() },
        SquamaParams { max_tilt: 25f64.to_radians(), ..Default::default() },
        SquamaParams { max_tilt: 0.5f64.to_radians(), ..Default::default() },
        SquamaParams { light_elevation: 50f64.to_radians(), ..Default::default() },
        SquamaParams { base_mm: 0.2, ..Default::default() },
        SquamaParams { width_mm: 3000.0, ..Default::default() },
        SquamaParams { width_mm: f64::NAN, ..Default::default() },
    ];
    for p in bad {
        assert!(matches!(generate(&p, &cb, &CancelToken::new()), Err(Error::InvalidParams(_))), "{p:?}");
    }
    // Relief guard: cell 0.5 at 1° tilt → relief 0.017 mm.
    let p = SquamaParams { width_mm: 25.0, cell_mm: 0.5, max_tilt: 1f64.to_radians(), ..Default::default() };
    let cb = CellBrightness::uniform(50, 10, 0.5);
    assert!(matches!(generate(&p, &cb, &CancelToken::new()), Err(Error::InvalidParams(_))));
    // Grid mismatch.
    let cb = CellBrightness::uniform(49, 63, 0.5);
    assert!(generate(&SquamaParams::default(), &cb, &CancelToken::new()).is_err());
}

#[test]
fn warnings() {
    let p = SquamaParams { cell_mm: 2.0, light_elevation: 2f64.to_radians(), ..Default::default() };
    let out = run(&p, ImageSource::Synthetic, &GenOptions::default(), &CancelToken::new()).unwrap();
    let codes: Vec<_> = out.warnings.iter().map(|w| w.code).collect();
    assert!(codes.contains(&"small_cell") && codes.contains(&"low_elevation"), "{codes:?}");
}

#[test]
fn cancellation_returns_cancelled() {
    let c = CancelToken::new();
    c.cancel();
    let cb = CellBrightness::uniform(50, 63, 0.5);
    assert_eq!(generate(&SquamaParams::default(), &cb, &c), Err(Error::Cancelled));
}

#[test]
fn deterministic() {
    let p = SquamaParams { cell_mm: 3.0, light_azimuth: 0.7, ..Default::default() };
    let a = run(&p, ImageSource::Synthetic, &GenOptions::default(), &CancelToken::new()).unwrap();
    let b = run(&p, ImageSource::Synthetic, &GenOptions::default(), &CancelToken::new()).unwrap();
    assert_eq!(fingerprint(&a.mesh), fingerprint(&b.mesh));
}

#[test]
fn image_source_with_height_crops() {
    let img = synthetic_target(400, 400);
    let p = SquamaParams { width_mm: 120.0, height_mm: Some(60.0), cell_mm: 6.0, ..Default::default() };
    let out = run(&p, ImageSource::Luma(&img), &GenOptions::default(), &CancelToken::new()).unwrap();
    assert_eq!((out.layout.nx, out.layout.ny), (20, 10));
    check(&p, &out, QuarterTurns::Auto);
}

fn image_strategy() -> impl Strategy<Value = (u32, u32, Vec<f32>)> {
    (1u32..13, 1u32..13, 0usize..6, prop::collection::vec(0f32..=1.0, 144)).prop_map(|(nx, ny, kind, noise)| {
        let n = (nx * ny) as usize;
        let g = (0..n)
            .map(|k| {
                let (i, j) = (k as u32 % nx, k as u32 / nx);
                match kind {
                    0 => noise[k],
                    1 => (noise[k] > 0.5) as u8 as f32,
                    2 => ((i + j) % 2) as f32,
                    3 => 0.0,
                    4 => 1.0,
                    _ => 0.5 + 1e-4 * (noise[k] - 0.5),
                }
            })
            .collect();
        (nx, ny, g)
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 300, .. ProptestConfig::default() })]

    #[test]
    fn random_params_and_images(
        (nx, ny, g) in image_strategy(),
        cell_um in prop::sample::select(vec![500i64, 1000, 2000, 3000, 6000, 10000, 777, 4321]),
        az in prop::sample::select(vec![0.0f64, 90.0, 180.0, 270.0, 45.0, 135.0, 12.3, 301.7]),
        el in prop::sample::select(vec![1.0f64, 5.0, 20.0, 45.0, 13.7]),
        tilt in prop::sample::select(vec![1.0f64, 10.0, 20.0, 7.3]),
        base in prop::sample::select(vec![0.4f64, 3.0, 1.234]),
        quarter in prop::sample::select(vec![QuarterTurns::Auto, QuarterTurns::Q0, QuarterTurns::Q1, QuarterTurns::Q2, QuarterTurns::Q3]),
    ) {
        let mut p = params(nx, cell_um);
        p.light_azimuth = az.to_radians();
        p.light_elevation = el.to_radians();
        p.max_tilt = tilt.to_radians();
        p.base_mm = base;
        let cb = CellBrightness { nx, ny, g };
        let out = gen_ok(&p, &cb);
        check(&p, &out, quarter);
        let again = gen_ok(&p, &cb);
        prop_assert_eq!(fingerprint(&out.mesh), fingerprint(&again.mesh));
    }

    #[test]
    fn random_images_through_sampling(
        w in 1u32..40, h in 1u32..40, cell_um in prop::sample::select(vec![2000i64, 3000, 6000]),
        px in prop::collection::vec(0f32..=1.0, 1600),
        width_cells in 1u32..15,
    ) {
        let img = LinearLuma { w, h, px: px[..(w * h) as usize].to_vec() };
        let p = params(width_cells, cell_um);
        let l = ad3d_core::squama::layout(&p, (w, h)).unwrap();
        let res = run(&p, ImageSource::Luma(&img), &GenOptions::default(), &CancelToken::new());
        if l.h_mm > 2048.0 {
            // Tall thin images can ask for a plate taller than the 2048 mm limit.
            prop_assert!(matches!(res, Err(Error::InvalidParams(_))));
            return Ok(());
        }
        let out = res.unwrap();
        prop_assert_eq!(out.layout.nx, width_cells);
        check(&p, &out, QuarterTurns::Auto);
    }
}

#[test]
fn synthetic_aspect() {
    let img = synthetic_target(SYNTHETIC_PX.0, SYNTHETIC_PX.1);
    let cb = cell_brightness(&img, 50, 63, None).unwrap();
    assert!(cb.g.iter().all(|g| (0.0..=1.0).contains(g)));
    let _ = PI;
}
