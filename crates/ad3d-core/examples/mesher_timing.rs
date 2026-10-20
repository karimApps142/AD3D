//! Minimum-of-N timing of the Squama stages (robust to a busy machine).
//! `cargo run --release -p ad3d-core --example mesher_timing [iterations]`

use std::time::Instant;

use ad3d_core::CancelToken;
use ad3d_core::image::{cell_brightness, synthetic_target};
use ad3d_core::relief::{PieceSet, Tol, mesh_relief, quantise};
use ad3d_core::squama::{SYNTHETIC_PX, SquamaParams, build_partition, generate, layout, solve_facets};

fn min_ms(n: usize, mut f: impl FnMut()) -> (f64, f64) {
    let mut v: Vec<f64> = (0..n)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (v[0], v[v.len() / 2])
}

fn main() {
    let n: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(200);
    let img = synthetic_target(SYNTHETIC_PX.0, SYNTHETIC_PX.1);
    let tol = Tol::default();
    let cancel = CancelToken::new();
    for cell in [6.0, 3.0, 2.0] {
        let p = SquamaParams { cell_mm: cell, ..Default::default() };
        let l = layout(&p, SYNTHETIC_PX).unwrap();
        let cb = cell_brightness(&img, l.nx, l.ny, None).unwrap();
        let facets = solve_facets(&cb, &l, &p, &tol);
        let part = build_partition(&l, &[]).unwrap();
        let h = quantise(&part, &facets, p.base_mm, &tol).unwrap();
        let (a, am) = min_ms(n, || {
            std::hint::black_box(mesh_relief(&part, PieceSet::All, &h, &[], &tol, &cancel).unwrap());
        });
        let (b, bm) = min_ms(n, || {
            std::hint::black_box(generate(&p, &cb, &cancel).unwrap());
        });
        let outline: Vec<glam::DVec2> = part.outline.iter().map(|&v| part.verts[v as usize]).collect();
        let (c, cm) = min_ms(n, || {
            std::hint::black_box(ad3d_core::relief::triangulate_outline(&outline).unwrap());
        });
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        let noise = ad3d_core::image::CellBrightness {
            nx: l.nx,
            ny: l.ny,
            g: (0..l.nx * l.ny)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    (x >> 40) as f32 / (1u64 << 24) as f32
                })
                .collect(),
        };
        let nm = generate(&p, &noise, &cancel).unwrap();
        let (d, dm) = min_ms(n, || {
            std::hint::black_box(generate(&p, &noise, &cancel).unwrap());
        });
        println!(
            "{}x{} noise: generate min {d:.3} med {dm:.3} ms ({} verts, {} tris)",
            l.nx,
            l.ny,
            nm.vertex_count(),
            nm.triangle_count()
        );
        println!(
            "{}x{}: mesh_relief min {a:.3} med {am:.3} ms | generate min {b:.3} med {bm:.3} ms | CDT min {c:.3} med {cm:.3} ms",
            l.nx, l.ny
        );
    }
}
