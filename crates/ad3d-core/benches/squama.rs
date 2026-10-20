//! Squama benchmarks: mesher only (quantise + mesh_relief), end to end from cell
//! brightness (tilts + quantise + mesh), and cell-brightness sampling from 12 MP.

use std::hint::black_box;

use ad3d_core::CancelToken;
use ad3d_core::image::{cell_brightness, synthetic_target};
use ad3d_core::payload;
use ad3d_core::relief::{PieceSet, Tol, mesh_relief, quantise};
use ad3d_core::squama::{SYNTHETIC_PX, SquamaParams, build_partition, generate, layout, solve_facets};
use criterion::{Criterion, criterion_group, criterion_main};

fn grids() -> Vec<(u32, u32, f64)> {
    // (nx, ny, cell) on a 300 mm wide plate from the 500×630 synthetic target.
    vec![(50, 63, 6.0), (100, 126, 3.0), (150, 189, 2.0)]
}

fn bench_squama(c: &mut Criterion) {
    let img = synthetic_target(SYNTHETIC_PX.0, SYNTHETIC_PX.1);
    let tol = Tol::default();
    let cancel = CancelToken::new();
    for (nx, ny, cell) in grids() {
        let p = SquamaParams { cell_mm: cell, ..Default::default() };
        let l = layout(&p, SYNTHETIC_PX).unwrap();
        assert_eq!((l.nx, l.ny), (nx, ny));
        let cb = cell_brightness(&img, l.nx, l.ny, None).unwrap();
        let facets = solve_facets(&cb, &l, &p, &tol);
        let part = build_partition(&l, &[]).unwrap();
        let heights = quantise(&part, &facets, p.base_mm, &tol).unwrap();

        let outline: Vec<glam::DVec2> = part.outline.iter().map(|&v| part.verts[v as usize]).collect();
        c.bench_function(&format!("back-face CDT {nx}x{ny}"), |b| {
            b.iter(|| ad3d_core::relief::triangulate_outline(black_box(&outline)).unwrap())
        });
        c.bench_function(&format!("mesh_relief {nx}x{ny}"), |b| {
            b.iter(|| mesh_relief(black_box(&part), PieceSet::All, black_box(&heights), &[], &tol, &cancel).unwrap())
        });
        c.bench_function(&format!("quantise+mesh {nx}x{ny}"), |b| {
            b.iter(|| {
                let h = quantise(black_box(&part), black_box(&facets), p.base_mm, &tol).unwrap();
                mesh_relief(&part, PieceSet::All, &h, &[], &tol, &cancel).unwrap()
            })
        });
        c.bench_function(&format!("end-to-end generate {nx}x{ny}"), |b| {
            b.iter(|| generate(black_box(&p), black_box(&cb), &cancel).unwrap())
        });
        // Worst-case-like input: independent uniform noise per cell (many walls,
        // crossings and saddles; about the Python reference's 59k vertices at 100×126).
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        let noise = ad3d_core::image::CellBrightness {
            nx,
            ny,
            g: (0..nx * ny)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    (x >> 40) as f32 / (1u64 << 24) as f32
                })
                .collect(),
        };
        c.bench_function(&format!("end-to-end generate {nx}x{ny} noise"), |b| {
            b.iter(|| generate(black_box(&p), black_box(&noise), &cancel).unwrap())
        });
        let mesh = generate(&p, &cb, &cancel).unwrap();
        c.bench_function(&format!("payload encode {nx}x{ny}"), |b| b.iter(|| payload::encode(black_box(&mesh), 1, 0.0)));
    }
}

fn bench_sampling(c: &mut Criterion) {
    // 12 MP (4000×3000) synthetic image → 100×75 cells (cell 3 on a 300 mm plate).
    let img = synthetic_target(4000, 3000);
    let mut g = c.benchmark_group("sampling");
    g.sample_size(20);
    g.bench_function("cell_brightness 12MP -> 100x75", |b| {
        b.iter(|| cell_brightness(black_box(&img), 100, 75, None).unwrap())
    });
    g.bench_function("cell_brightness 12MP -> 150x113", |b| {
        b.iter(|| cell_brightness(black_box(&img), 150, 113, None).unwrap())
    });
    g.finish();
}

criterion_group!(benches, bench_squama, bench_sampling);
criterion_main!(benches);
