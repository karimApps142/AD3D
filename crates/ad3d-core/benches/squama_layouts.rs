//! M1 layout benchmarks (§13 drag budget): the facet solve and the whole-plate
//! pipeline for square and hex layouts under a directional light, a point light
//! and a measured response, at the drag-budget size (~100×126 cells, 300 mm wide).

use std::hint::black_box;

use ad3d_core::CancelToken;
use ad3d_core::image::synthetic_target;
use ad3d_core::relief::Tol;
use ad3d_core::squama::{
    HexOrientation, LayoutKind, Light, Limits, PlateSpec, Quality, Response, SYNTHETIC_PX,
    SolveOpts, SquamaParams, facet_brightness, generate_plate, layout_for, solve_facets_ex,
};
use criterion::{Criterion, criterion_group, criterion_main};
use glam::DVec3;

fn bench_layouts(c: &mut Criterion) {
    let img = synthetic_target(SYNTHETIC_PX.0, SYNTHETIC_PX.1);
    let tol = Tol::default();
    let cancel = CancelToken::new();
    let p = SquamaParams {
        cell_mm: 3.0,
        ..Default::default()
    };
    let kinds = [
        ("square", LayoutKind::Square),
        (
            "hex-pointy",
            LayoutKind::Hex {
                orientation: HexOrientation::Pointy,
            },
        ),
        (
            "hex-flat",
            LayoutKind::Hex {
                orientation: HexOrientation::Flat,
            },
        ),
    ];
    let lights = [
        (
            "directional",
            Light::Directional {
                az: 2.0,
                el: 5f64.to_radians(),
            },
        ),
        (
            "point",
            Light::Point {
                pos_mm: DVec3::new(-400.0, 190.0, 80.0),
            },
        ),
    ];
    let measured = Response::Measured {
        tilt_rad: [-30.0f64, -15.0, 0.0, 10.0, 20.0, 30.0]
            .iter()
            .map(|d| d.to_radians())
            .collect(),
        brightness: vec![0.02, 0.06, 0.2, 0.35, 0.55, 0.6],
    };
    let mut grp = c.benchmark_group("squama_layouts");
    grp.sample_size(30);
    for (kname, kind) in kinds {
        let l = layout_for(&p, kind, SYNTHETIC_PX).unwrap();
        let g = facet_brightness(&img, &l).unwrap();
        let nf = l.facet_count();
        for (lname, light) in lights {
            for (rname, resp) in [
                ("lambert", Response::default()),
                ("measured", measured.clone()),
            ] {
                let spec = PlateSpec {
                    layout: l,
                    light,
                    limits: Limits {
                        max_tilt: 10f64.to_radians(),
                        base_mm: 3.0,
                        max_relief_mm: None,
                    },
                    response: resp,
                    opts: SolveOpts::default(),
                    quality: Quality::Preview,
                    tol,
                };
                grp.bench_function(
                    format!("solve {kname} {lname} {rname} ({nf} facets)"),
                    |b| {
                        b.iter(|| {
                            solve_facets_ex(
                                black_box(&g),
                                &spec.layout,
                                &spec.light,
                                &spec.limits,
                                &spec.response,
                                &spec.opts,
                                spec.quality,
                                &tol,
                                &cancel,
                            )
                            .unwrap()
                        })
                    },
                );
                if rname == "lambert" {
                    grp.bench_function(
                        format!("generate_plate {kname} {lname} ({nf} facets)"),
                        |b| {
                            b.iter(|| {
                                generate_plate(black_box(&spec), black_box(&g), &cancel).unwrap()
                            })
                        },
                    );
                }
            }
        }
    }
    grp.finish();
}

criterion_group!(benches, bench_layouts);
criterion_main!(benches);
