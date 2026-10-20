//! Forward model and optimiser benchmarks: `shade` (exact horizon sweep) at
//! 50×63 and 100×126, an oblique light, a point light, one optimiser sweep, and
//! a preview render.

use std::f64::consts::PI;
use std::hint::black_box;

use ad3d_core::CancelToken;
use ad3d_core::image::{cell_brightness, synthetic_target};
use ad3d_core::relief::square_grid;
use ad3d_core::sim::render::{RenderOpts, render};
use ad3d_core::sim::{Light, Response, shade};
use ad3d_core::squama::optimise::{Limits, OptimiserOpts, lambert_facets, optimise_from};
use criterion::{Criterion, criterion_group, criterion_main};
use glam::{DVec2, DVec3};

fn bench_sim(c: &mut Criterion) {
    let img = synthetic_target(500, 630);
    let lim = Limits { max_tilt: 10f64.to_radians(), base_mm: 3.0, max_relief_mm: None };
    let resp = Response::Lambert { ambient: 0.0 };
    let cancel = CancelToken::new();
    for (nx, ny, cell) in [(50u32, 63u32, 6.0), (100, 126, 3.0)] {
        let cb = cell_brightness(&img, nx, ny, None).unwrap();
        let part = square_grid(nx, ny, cell, DVec2::ZERO);
        let left = Light::Directional { az: PI, el: 5f64.to_radians() };
        let oblique = Light::Directional { az: 2.5, el: 5f64.to_radians() };
        let point = Light::Point { pos_mm: DVec3::new(-400.0, ny as f64 * cell / 2.0, 60.0) };
        for (name, light) in [("left", left), ("oblique", oblique), ("point", point)] {
            let f = lambert_facets(&cb, &part, &light, &lim, &resp).unwrap();
            c.bench_function(&format!("shade {name} {nx}x{ny}"), |b| {
                b.iter(|| shade(black_box(&part), black_box(&f), &light, &resp, 0).unwrap())
            });
        }
        let f = lambert_facets(&cb, &part, &left, &lim, &resp).unwrap();
        let mut g = c.benchmark_group("optimiser");
        g.sample_size(10);
        let one = OptimiserOpts { max_sweeps: 1, time_budget: None, min_gain: 0.0, ..Default::default() };
        g.bench_function(format!("optimise 1 sweep left {nx}x{ny}"), |b| {
            b.iter(|| optimise_from(&cb, &part, black_box(&f), &left, &lim, &resp, &one, &cancel).unwrap())
        });
        g.finish();
        let ro = RenderOpts { px_per_mm: 2.0, supersample: 2, white: None };
        c.bench_function(&format!("render 2px/mm ss2 {nx}x{ny}"), |b| {
            b.iter(|| render(black_box(&part), black_box(&f), &left, &resp, &ro).unwrap())
        });
    }
}

criterion_group!(benches, bench_sim);
criterion_main!(benches);
