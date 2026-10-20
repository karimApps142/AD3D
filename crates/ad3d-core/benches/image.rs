//! Image → target benchmarks (design §13, §16): a 12 MP photo to 100 × 126 targets.
//! Stages: working copy (12 MP → 2048 px), tone + SAT build, and the per-drag lookup
//! (budget ≤ 2 ms with the SAT cached), for square and hex cells. Hex lookups are
//! exact polygon coverage (cost ∝ pixels crossed by the cell outlines) and run on the
//! rayon pool; they only rerun on tone/crop changes, never on light drags.

use std::f64::consts::PI;
use std::hint::black_box;

use ad3d_core::image::sat::clip_to_rect;
use ad3d_core::image::working::{DEFAULT_MAX_SIDE, DRAG_MAX_SIDE};
use ad3d_core::image::{ImageOpts, LinearLuma, TargetSampler, ToneLut, build_working, centre_crop, synthetic_target};
use criterion::{Criterion, criterion_group, criterion_main};
use glam::DVec2;

/// Pointy hex cells (§6.2 phase) clipped to a `w × h` plate.
fn hex_pieces(cell: f64, w: f64, h: f64) -> Vec<Vec<DVec2>> {
    let r = cell / 3f64.sqrt();
    let pitch = 1.5 * r;
    let mut out = Vec::new();
    for j in -1..=((h / pitch) as i32 + 1) {
        for i in -1..=((w / cell) as i32 + 1) {
            let c = DVec2::new((i as f64 + 0.5 * j.rem_euclid(2) as f64) * cell, j as f64 * pitch);
            let hex: Vec<DVec2> = (0..6).map(|k| c + r * DVec2::from_angle(PI / 6.0 + k as f64 * PI / 3.0)).collect();
            let cl = clip_to_rect(&hex, 0.0, 0.0, w, h);
            // Drop zero-area slivers left where a hex only touches the plate edge.
            let area2: f64 = (0..cl.len()).map(|k| cl[k].perp_dot(cl[(k + 1) % cl.len()])).sum();
            if cl.len() >= 3 && area2.abs() > 1e-9 {
                out.push(cl);
            }
        }
    }
    out
}

fn bench_image(c: &mut Criterion) {
    // 12 MP portrait photo, cropped to the 100 : 126 plate aspect.
    let img: LinearLuma = synthetic_target(3000, 4000);
    let crop = centre_crop(img.w, img.h, 100.0, 126.0);
    let opts = ImageOpts { crop: Some(crop), gamma: 1.4, contrast: 1.2, auto_levels: Some([0.01, 0.99]), ..Default::default() };
    let mut g = c.benchmark_group("image");
    g.sample_size(20);

    g.bench_function("working 12MP -> 2048", |b| b.iter(|| build_working(black_box(&img), &opts, DEFAULT_MAX_SIDE).unwrap()));
    let wk = build_working(&img, &opts, DEFAULT_MAX_SIDE).unwrap();
    let wk_drag = build_working(&img, &opts, DRAG_MAX_SIDE).unwrap();

    g.bench_function("tone+SAT 2048", |b| {
        b.iter(|| {
            let lut = ToneLut::new(&opts, Some(&wk.hist));
            TargetSampler::new(black_box(&wk), &lut)
        })
    });
    g.bench_function("tone+SAT 1024 (drag)", |b| {
        b.iter(|| {
            let lut = ToneLut::new(&opts, Some(&wk_drag.hist));
            TargetSampler::new(black_box(&wk_drag), &lut)
        })
    });

    let s = TargetSampler::new(&wk, &ToneLut::new(&opts, Some(&wk.hist)));
    g.bench_function("lookup square 100x126 (cached SAT)", |b| b.iter(|| black_box(&s).square_cells(100, 126).unwrap()));

    // Hex at the same density: 300 mm plate, 3 mm across flats.
    let (pw, ph) = (300.0, 378.0);
    let hex = hex_pieces(3.0, pw, ph);
    let n = hex.len();
    g.bench_function(format!("lookup hex {n} cells (cached SAT)"), |b| {
        b.iter(|| black_box(&s).polygon_means((pw, ph), hex.iter().enumerate().map(|(k, p)| (k as u32, p.as_slice())), n).unwrap())
    });
    let sd = TargetSampler::new(&wk_drag, &ToneLut::new(&opts, Some(&wk_drag.hist)));
    g.bench_function(format!("lookup hex {n} cells (cached SAT 1024)"), |b| {
        b.iter(|| black_box(&sd).polygon_means((pw, ph), hex.iter().enumerate().map(|(k, p)| (k as u32, p.as_slice())), n).unwrap())
    });
    g.finish();
}

criterion_group!(benches, bench_image);
criterion_main!(benches);
