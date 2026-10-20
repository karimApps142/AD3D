//! Shadow-aware optimiser (§15.1): improvement over the plain Lambert inverse on
//! several images, constraints, determinism across thread counts, cancellation,
//! and a randomised check that optimised facets still mesh watertight.
//!
//! The full-size numbers are printed by the ignored `report_*` tests:
//! `cargo test --release -p ad3d-core --test sim_optimise -- --ignored --nocapture`.

use std::f64::consts::PI;
use std::time::Duration;

use ad3d_core::CancelToken;
use ad3d_core::error::Error;
use ad3d_core::image::{CellBrightness, LinearLuma, cell_brightness, synthetic_target};
use ad3d_core::mesh::validate::{ValidateOpts, validate};
use ad3d_core::relief::{Facet, Partition, PieceSet, Tol, mesh_relief, quantise, square_grid};
use ad3d_core::sim::{Light, Response};
use ad3d_core::squama::optimise::{
    Limits, OptimiseReport, OptimiserOpts, lambert_facets, optimise_from, optimise_report,
};
use glam::{DVec2, DVec3};
use proptest::prelude::*;

/// Test images in linear light, 500×630 like the synthetic target.
fn images() -> Vec<(&'static str, LinearLuma)> {
    let (w, h) = (500u32, 630u32);
    let make = |f: &dyn Fn(f64, f64) -> f64| LinearLuma {
        w,
        h,
        px: (0..w * h)
            .map(|k| {
                let (x, y) = ((k % w) as f64 / w as f64, (k / w) as f64 / h as f64);
                f(x, y).clamp(0.0, 1.0) as f32
            })
            .collect(),
    };
    vec![
        ("synthetic", synthetic_target(w, h)),
        // A face-like layout: bright oval, dark eyes and mouth, soft background.
        (
            "portrait",
            make(&|x, y| {
                let oval = ((x - 0.5) / 0.32).powi(2) + ((y - 0.5) / 0.4).powi(2);
                let mut v = if oval < 1.0 { 0.75 - 0.25 * oval } else { 0.12 + 0.1 * y };
                for ex in [0.38, 0.62] {
                    if ((x - ex) / 0.06).powi(2) + ((y - 0.4) / 0.035).powi(2) < 1.0 {
                        v = 0.05;
                    }
                }
                if ((x - 0.5) / 0.12).powi(2) + ((y - 0.68) / 0.03).powi(2) < 1.0 {
                    v = 0.08;
                }
                v
            }),
        ),
        ("radial", make(&|x, y| 1.0 - 1.6 * ((x - 0.5).powi(2) + (y - 0.5).powi(2)).sqrt())),
        ("waves", make(&|x, y| 0.5 + 0.45 * (9.0 * x).sin() * (7.0 * y).cos())),
        ("bars", make(&|x, y| if ((x * 6.0) as i32 + (y * 3.0) as i32) % 2 == 0 { 0.85 } else { 0.08 })),
    ]
}

fn opts(sweeps: u32) -> OptimiserOpts {
    OptimiserOpts { max_sweeps: sweeps, time_budget: None, ..Default::default() }
}

fn run(
    cb: &CellBrightness,
    cell: f64,
    light: Light,
    s_deg: f64,
    relief: Option<f64>,
    o: &OptimiserOpts,
) -> (Partition, OptimiseReport) {
    let part = square_grid(cb.nx, cb.ny, cell, DVec2::ZERO);
    let lim = Limits { max_tilt: s_deg.to_radians(), base_mm: 3.0, max_relief_mm: relief };
    let r = optimise_report(cb, &part, &light, &lim, &Response::Lambert { ambient: 0.0 }, o, &CancelToken::new())
        .unwrap();
    (part, r)
}

fn left(el_deg: f64) -> Light {
    Light::Directional { az: PI, el: el_deg.to_radians() }
}

fn assert_monotone(r: &OptimiseReport) {
    for w in r.sse.windows(2) {
        assert!(w[1] <= w[0] * (1.0 + 1e-9) + 1e-15, "error rose: {:?}", r.sse);
    }
}

fn corners_ok(part: &Partition, f: &[Facet], r: &OptimiseReport) {
    let (lo, hi) = r.height_range;
    for piece in &part.pieces {
        let fc = &f[piece.facet as usize];
        for &v in &part.rings[piece.ring.start as usize..piece.ring.end as usize] {
            let w = fc.eval(part.verts[v as usize]);
            assert!(w >= lo - 1e-9 && w <= hi + 1e-9, "corner {w} outside [{lo}, {hi}]");
        }
    }
}

#[test]
fn improves_on_lambert_for_several_images() {
    // 30×38 cells of 6 mm at α 5°, S 10° (the research's hard case).
    for (name, img) in images() {
        let cb = cell_brightness(&img, 30, 38, None).unwrap();
        let (part, r) = run(&cb, 6.0, left(5.0), 10.0, None, &opts(4));
        println!(
            "{name:10} r {:.3} → {:.3}  rmse_rel {:.3} → {:.3}  sweeps {} moves {}",
            r.before.pearson, r.after.pearson, r.before.rmse_rel, r.after.rmse_rel, r.sweeps, r.moves
        );
        assert_monotone(&r);
        corners_ok(&part, &r.facets, &r);
        // Cast shadows put the brightest absolute targets out of reach of any
        // tilt, so RMSE against the absolute target falls only a little; the
        // visible gain is in the correlation (see `report_full_size`).
        assert!(r.after.rmse < 0.97 * r.before.rmse, "{name}: rmse {} → {}", r.before.rmse, r.after.rmse);
        assert!(r.after.pearson >= r.before.pearson + 0.02, "{name}: r {} → {}", r.before.pearson, r.after.pearson);
    }
}

#[test]
fn improves_with_oblique_and_point_lights() {
    let img = synthetic_target(500, 630);
    let cb = cell_brightness(&img, 20, 25, None).unwrap();
    for light in [
        Light::Directional { az: 2.4, el: 6f64.to_radians() },
        Light::Point { pos_mm: DVec3::new(-250.0, 75.0, 40.0) },
    ] {
        let (part, r) = run(&cb, 6.0, light, 10.0, None, &opts(3));
        println!("{light:?}: r {:.3} → {:.3}, rmse_rel {:.3} → {:.3}, tasks {:?}", r.before.pearson, r.after.pearson,
            r.before.rmse_rel, r.after.rmse_rel, r.tasks);
        assert_monotone(&r);
        corners_ok(&part, &r.facets, &r);
        assert!(r.after.rmse < r.before.rmse);
    }
}

#[test]
fn fixed_height_and_relief_budget() {
    let img = synthetic_target(500, 630);
    let cb = cell_brightness(&img, 20, 25, None).unwrap();
    let fixed = OptimiserOpts { vary_height: false, ..opts(3) };
    let (part, r) = run(&cb, 6.0, left(5.0), 10.0, None, &fixed);
    assert_monotone(&r);
    corners_ok(&part, &r.facets, &r);
    let h0 = r.facets[0].h;
    assert!(r.facets.iter().all(|f| (f.h - h0).abs() < 1e-12), "heights must stay at the centre pivot");
    assert!(r.after.rmse < r.before.rmse);
    // A larger budget gives the optimiser more room.
    let (part, r2) = run(&cb, 6.0, left(5.0), 10.0, Some(3.0), &opts(3));
    corners_ok(&part, &r2.facets, &r2);
    assert!((r2.height_range.1 - 6.0).abs() < 1e-12);
}

#[test]
fn deterministic_across_thread_counts() {
    let img = synthetic_target(500, 630);
    let cb = cell_brightness(&img, 18, 22, None).unwrap();
    for light in [left(5.0), Light::Directional { az: 2.0, el: 7f64.to_radians() }] {
        let go = |threads: usize| {
            rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap()
                .install(|| run(&cb, 6.0, light, 10.0, None, &opts(2)).1)
        };
        let (a, b) = (go(1), go(4));
        assert_eq!(a.facets, b.facets);
        assert_eq!(a.sse, b.sse);
    }
}

#[test]
fn zero_sweeps_is_lambert_and_cancel_works() {
    let cb = CellBrightness { nx: 8, ny: 6, g: (0..48).map(|i| (i as f32 * 0.61).fract()).collect() };
    let part = square_grid(8, 6, 6.0, DVec2::ZERO);
    let lim = Limits { max_tilt: 0.17, base_mm: 3.0, max_relief_mm: None };
    let resp = Response::default();
    let light = left(5.0);
    let init = lambert_facets(&cb, &part, &light, &lim, &resp).unwrap();
    let r = optimise_from(&cb, &part, &init, &light, &lim, &resp, &opts(0), &CancelToken::new()).unwrap();
    assert_eq!(r.sweeps, 0);
    for (a, b) in r.facets.iter().zip(&init) {
        assert!((a.h - b.h).abs() < 1e-12 && (a.grad - b.grad).length() < 1e-12);
    }
    let budget0 = OptimiserOpts { time_budget: Some(Duration::ZERO), ..opts(5) };
    let r = optimise_from(&cb, &part, &init, &light, &lim, &resp, &budget0, &CancelToken::new()).unwrap();
    assert!(r.timed_out && r.sweeps == 0);
    let c = CancelToken::new();
    c.cancel();
    assert_eq!(optimise_from(&cb, &part, &init, &light, &lim, &resp, &opts(3), &c).unwrap_err(), Error::Cancelled);
}

fn mesh_check(part: &Partition, facets: &[Facet], base: f64) {
    let tol = Tol::default();
    let h = quantise(part, facets, base, &tol).unwrap_or_else(|e| panic!("quantise: {e}"));
    let m = mesh_relief(part, PieceSet::All, &h, &[], &tol, &CancelToken::new()).unwrap();
    let r = validate(
        &m,
        &ValidateOpts { min_facet_w: Some(base - tol.q / 2.0), back_area: Some(part.outline_area()), ..Default::default() },
    );
    assert!(r.ok, "validate failed:\n{}", r.summary());
}

#[test]
fn optimised_plate_meshes_watertight() {
    let img = synthetic_target(500, 630);
    let cb = cell_brightness(&img, 20, 25, None).unwrap();
    let (part, r) = run(&cb, 6.0, left(5.0), 10.0, None, &opts(3));
    mesh_check(&part, &r.facets, 3.0);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 24, ..ProptestConfig::default() })]

    /// Random grids, lights, limits and targets: the optimiser never raises the
    /// error, keeps every corner in budget, and its facets mesh watertight.
    #[test]
    fn random_optimised_plates_are_valid(
        nx in 2u32..9, ny in 2u32..9,
        cell_um in 1500i64..8000,
        az in 0.0f64..360.0, el in 1.0f64..30.0, s in 2.0f64..20.0,
        relief in prop::option::of(0.3f64..3.0),
        vary in any::<bool>(),
        point in any::<bool>(),
        seed in any::<u64>(),
    ) {
        let cell = cell_um as f64 / 1000.0;
        let mut x = seed | 1;
        let g: Vec<f32> = (0..nx * ny).map(|_| { x ^= x << 13; x ^= x >> 7; x ^= x << 17; (x >> 40) as f32 / (1u64 << 24) as f32 }).collect();
        let cb = CellBrightness { nx, ny, g };
        let light = if point {
            let (w, h) = (nx as f64 * cell, ny as f64 * cell);
            let d = 3.0 * w.max(h);
            Light::Point { pos_mm: DVec3::new(w / 2.0 + d * az.to_radians().cos(), h / 2.0 + d * az.to_radians().sin(), 10.0 + d * el.to_radians().tan()) }
        } else {
            Light::Directional { az: az.to_radians(), el: el.to_radians() }
        };
        let part = square_grid(nx, ny, cell, DVec2::ZERO);
        let lim = Limits { max_tilt: s.to_radians(), base_mm: 0.4 + (seed % 5) as f64, max_relief_mm: relief };
        let o = OptimiserOpts { max_sweeps: 2, vary_height: vary, time_budget: None, ..Default::default() };
        let r = optimise_report(&cb, &part, &light, &lim, &Response::Lambert { ambient: 0.05 }, &o, &CancelToken::new());
        let r = match r { Ok(r) => r, Err(Error::InvalidParams(_)) => return Ok(()), Err(e) => panic!("{e}") };
        assert_monotone(&r);
        corners_ok(&part, &r.facets, &r);
        // The mesher needs a visible relief; tiny budgets only test the solver.
        if r.height_range.1 - r.height_range.0 >= 0.15 {
            mesh_check(&part, &r.facets, lim.base_mm);
        }
    }
}

/// Full-size before/after table (synthetic target, 50×63 cells of 6 mm, light
/// from the left), plus the other test images at α 5°, S 10°.
#[test]
#[ignore]
fn report_full_size() {
    let img = synthetic_target(500, 630);
    let cb = cell_brightness(&img, 50, 63, None).unwrap();
    println!("synthetic 50×63, cell 6, light Left, ambient 0, default relief budget");
    println!("α/S      | Lambert r  rmse_rel contrast | optimised r  rmse_rel contrast | sweeps  ms");
    for (al, s) in [(5.0, 10.0), (5.0, 20.0), (10.0, 10.0), (10.0, 20.0), (20.0, 20.0)] {
        let (_, r) = run(&cb, 6.0, left(al), s, None, &OptimiserOpts { max_sweeps: 10, time_budget: None, ..Default::default() });
        println!(
            "{al:>2}°/{s:>2}°  | {:.3}  {:.3}  {:5.2} | {:.3}  {:.3}  {:5.2} | {:>2} {:>6.0}",
            r.before.pearson, r.before.rmse_rel, r.before.contrast, r.after.pearson, r.after.rmse_rel, r.after.contrast,
            r.sweeps, r.elapsed_ms
        );
    }
    println!("α 5°/S 10°, relief budget 3 mm:");
    let (_, r) = run(&cb, 6.0, left(5.0), 10.0, Some(3.0), &OptimiserOpts { max_sweeps: 10, time_budget: None, ..Default::default() });
    println!("  r {:.3} → {:.3}, rmse_rel {:.3} → {:.3}", r.before.pearson, r.after.pearson, r.before.rmse_rel, r.after.rmse_rel);
    println!("other images, 50×63, α 5°/S 10°:");
    for (name, img) in images() {
        let cb = cell_brightness(&img, 50, 63, None).unwrap();
        let (_, r) = run(&cb, 6.0, left(5.0), 10.0, None, &OptimiserOpts { max_sweeps: 10, time_budget: None, ..Default::default() });
        println!("  {name:10} r {:.3} → {:.3}, rmse_rel {:.3} → {:.3}, sweeps {}, {:.0} ms", r.before.pearson, r.after.pearson,
            r.before.rmse_rel, r.after.rmse_rel, r.sweeps, r.elapsed_ms);
    }
    let cb = cell_brightness(&img, 100, 126, None).unwrap();
    let (_, r) = run(&cb, 3.0, left(5.0), 10.0, None, &OptimiserOpts { max_sweeps: 10, time_budget: None, ..Default::default() });
    println!("synthetic 100×126 cell 3, 5°/10°: r {:.3} → {:.3}, rmse_rel {:.3} → {:.3}, sweeps {}, {:.0} ms", r.before.pearson,
        r.after.pearson, r.before.rmse_rel, r.after.rmse_rel, r.sweeps, r.elapsed_ms);
    let cb = cell_brightness(&img, 50, 63, None).unwrap();
    let (_, r) = run(&cb, 6.0, Light::Directional { az: 2.5, el: 5f64.to_radians() }, 10.0, None,
        &OptimiserOpts { max_sweeps: 10, time_budget: None, ..Default::default() });
    println!("synthetic 50×63 oblique az 143°, 5°/10°: r {:.3} → {:.3}, rmse_rel {:.3} → {:.3}, sweeps {}, {:.0} ms, tasks {:?}",
        r.before.pearson, r.after.pearson, r.before.rmse_rel, r.after.rmse_rel, r.sweeps, r.elapsed_ms, r.tasks);
}
