use glam::{DVec2, DVec3};

use super::render::{RenderOpts, render};
use super::*;
use crate::relief::square_grid;

fn rng(seed: u64) -> impl FnMut() -> f64 {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Facets of a square grid with tilt `th[k]` toward the light `a` at the centre
/// pivot height `c`.
fn grid_facets(nx: u32, ny: u32, cell: f64, a: DVec2, th: &[f64], c: f64) -> Vec<Facet> {
    let mut out = Vec::new();
    for j in 0..ny {
        for i in 0..nx {
            let k = (j * nx + i) as usize;
            out.push(Facet {
                pivot: DVec2::new((i as f64 + 0.5) * cell, (j as f64 + 0.5) * cell),
                h: c,
                grad: -th[k].tan() * a,
            });
        }
    }
    out
}

fn lit_fractions(p: &Partition, f: &[Facet], light: &Light, lpc: u32) -> Vec<f64> {
    let ls = line_set(p, f, light, lpc).unwrap();
    let centres: Vec<(DVec2, f64)> = f.iter().map(|x| (x.pivot, x.h)).collect();
    let lights = facet_lights(p, &centres, light).unwrap();
    shade_lines(&ls, f, &lights, &Response::default().prepare(0.0).unwrap()).lit_fraction
}

#[test]
fn flat_plate_is_fully_lit() {
    let p = square_grid(6, 4, 5.0, DVec2::ZERO);
    let f = grid_facets(6, 4, 5.0, DVec2::X, &[0.0; 24], 3.0);
    for az in [0.0, 0.7, 2.0, std::f64::consts::PI, 5.0] {
        let el = 7f64.to_radians();
        let b = shade(&p, &f, &Light::Directional { az, el }, &Response::default(), 0).unwrap();
        for x in b {
            assert!((x - el.sin()).abs() < 1e-12, "az {az}: {x}");
        }
    }
}

#[test]
fn uniform_tilt_matches_closed_form() {
    // Light from the left; every facet tilted θ toward it. All but the first
    // column lose tan θ / (tan α + tan θ) of their width to the step shadow
    // (FEATURES PROJ-SQUAMA step 4), so they read sin α · cos θ.
    let (nx, ny, cell) = (10, 3, 6.0);
    let (al, th) = (5f64.to_radians(), 10f64.to_radians());
    let p = square_grid(nx, ny, cell, DVec2::ZERO);
    let f = grid_facets(nx, ny, cell, -DVec2::X, &vec![th; (nx * ny) as usize], 4.0);
    let light = Light::Directional { az: std::f64::consts::PI, el: al };
    let lf = lit_fractions(&p, &f, &light, 0);
    let want = al.tan() / (al.tan() + th.tan());
    for j in 0..ny {
        assert!((lf[(j * nx) as usize] - 1.0).abs() < 1e-12);
        for i in 1..nx {
            let got = lf[(j * nx + i) as usize];
            assert!((got - want).abs() < 1e-12, "({i},{j}) {got} vs {want}");
        }
    }
    let b = shade(&p, &f, &light, &Response::default(), 0).unwrap();
    assert!((b[5] - al.sin() * th.cos()).abs() < 1e-12);
    // 4 lines per cell give the same exact answer here.
    let lf4 = lit_fractions(&p, &f, &light, 4);
    for (a, b) in lf.iter().zip(&lf4) {
        assert!((a - b).abs() < 1e-12);
    }
}

/// Independent check: ray-march toward the light from a dense grid of points.
fn brute_lit(p: &Partition, nx: u32, ny: u32, cell: f64, f: &[Facet], light: &Light, n: u32) -> Vec<f64> {
    let surf = |q: DVec2| -> Option<f64> {
        if q.x < 0.0 || q.y < 0.0 || q.x >= nx as f64 * cell || q.y >= ny as f64 * cell {
            return None;
        }
        let (i, j) = ((q.x / cell) as u32, (q.y / cell) as u32);
        Some(f[(j * nx + i) as usize].eval(q))
    };
    let _ = p;
    let mut out = vec![0.0; f.len()];
    for j in 0..ny {
        for i in 0..nx {
            let mut lit = 0;
            for sy in 0..n {
                for sx in 0..n {
                    let q = DVec2::new(
                        (i as f64 + (sx as f64 + 0.5) / n as f64) * cell,
                        (j as f64 + (sy as f64 + 0.5) / n as f64) * cell,
                    );
                    let w0 = surf(q).unwrap();
                    let (dir, slope, dist) = match *light {
                        Light::Directional { az, el } => (DVec2::new(az.cos(), az.sin()), el.tan(), 1e9),
                        Light::Point { pos_mm } => {
                            let v = DVec2::new(pos_mm.x, pos_mm.y) - q;
                            (v.normalize(), (pos_mm.z - w0) / v.length(), v.length())
                        }
                    };
                    let step = 0.002;
                    let mut x = step;
                    let mut blocked = false;
                    while x < dist.min(60.0) {
                        match surf(q + dir * x) {
                            Some(w) if w > w0 + x * slope + 1e-9 => {
                                blocked = true;
                                break;
                            }
                            None => break,
                            _ => {}
                        }
                        x += step;
                    }
                    if !blocked {
                        lit += 1;
                    }
                }
            }
            out[(j * nx + i) as usize] = lit as f64 / (n * n) as f64;
        }
    }
    out
}

#[test]
fn matches_brute_force_ray_march() {
    let (nx, ny, cell) = (6u32, 5u32, 4.0);
    let p = square_grid(nx, ny, cell, DVec2::ZERO);
    let mut r = rng(7);
    let s = 15f64.to_radians();
    for (case, light) in [
        Light::Directional { az: std::f64::consts::PI, el: 6f64.to_radians() },
        Light::Directional { az: 2.6, el: 8f64.to_radians() },
        Light::Directional { az: 4.4, el: 12f64.to_radians() },
        Light::Point { pos_mm: DVec3::new(-60.0, 10.0, 14.0) },
        Light::Point { pos_mm: DVec3::new(12.0, 10.0, 30.0) }, // above the plate
    ]
    .into_iter()
    .enumerate()
    {
        let centres: Vec<DVec2> =
            (0..nx * ny).map(|k| DVec2::new(((k % nx) as f64 + 0.5) * cell, ((k / nx) as f64 + 0.5) * cell)).collect();
        let lights = facet_lights(&p, &centres.iter().map(|&c| (c, 4.0)).collect::<Vec<_>>(), &light).unwrap();
        let f: Vec<Facet> = (0..(nx * ny) as usize)
            .map(|k| Facet { pivot: centres[k], h: 4.0 + 0.6 * r(), grad: -((2.0 * r() - 1.0) * s).tan() * lights[k].a })
            .collect();
        let got = lit_fractions(&p, &f, &light, 24);
        let want = brute_lit(&p, nx, ny, cell, &f, &light, 24);
        let mut worst: f64 = 0.0;
        for k in 0..f.len() {
            // Self-shadowed facets (facing away) count as unlit in the sweep.
            if lights[k].cos_facet(&f[k]) <= 0.0 {
                continue;
            }
            worst = worst.max((got[k] - want[k]).abs());
        }
        assert!(worst < 0.03, "case {case}: worst lit-fraction error {worst}");
    }
}

#[test]
fn distant_point_light_approaches_directional() {
    let (nx, ny, cell) = (8u32, 4u32, 5.0);
    let p = square_grid(nx, ny, cell, DVec2::ZERO);
    let mut r = rng(3);
    let th: Vec<f64> = (0..nx * ny).map(|_| (2.0 * r() - 1.0) * 0.17).collect();
    let f = grid_facets(nx, ny, cell, -DVec2::X, &th, 4.0);
    let el = 8f64.to_radians();
    let dist = 1e6;
    let c = DVec2::new(20.0, 10.0);
    let lamp = DVec3::new(c.x - dist * el.cos(), c.y, dist * el.sin());
    let a = lit_fractions(&p, &f, &Light::Directional { az: std::f64::consts::PI, el }, 8);
    let b = lit_fractions(&p, &f, &Light::Point { pos_mm: lamp }, 8);
    for k in 0..a.len() {
        assert!((a[k] - b[k]).abs() < 2e-3, "{k}: {} vs {}", a[k], b[k]);
    }
}

#[test]
fn point_light_irradiance_and_targets() {
    let p = square_grid(4, 4, 5.0, DVec2::ZERO);
    let f = grid_facets(4, 4, 5.0, DVec2::X, &[0.0; 16], 3.0);
    let lamp = DVec3::new(-100.0, 10.0, 30.0);
    let centres: Vec<(DVec2, f64)> = f.iter().map(|x| (x.pivot, x.h)).collect();
    let l = facet_lights(&p, &centres, &Light::Point { pos_mm: lamp }).unwrap();
    // Nearer facets get more light and a higher elevation.
    assert!(l[0].irr > l[3].irr && l[0].el > l[3].el);
    let curve = Response::default().prepare(0.0).unwrap();
    let (lo, hi) = target_range(&l, 10f64.to_radians(), &curve).unwrap();
    assert!(hi > lo && lo >= 0.0);
    for fl in &l {
        let th = invert_direct(fl, 10f64.to_radians(), &curve, hi);
        assert!((fl.irr * fl.cos_tilt(th) - hi).abs() < 1e-12);
    }
}

#[test]
fn measured_response_inverts_monotone_part() {
    let resp = Response::Measured {
        tilt_rad: vec![-0.2, -0.1, 0.0, 0.1, 0.2],
        brightness: vec![0.05, 0.05, 0.2, 0.35, 0.4],
    };
    resp.check().unwrap();
    // Reference elevation = the facet's: the table is read at the facet's tilt.
    let fl = FacetLight { a: DVec2::X, el: 0.1, irr: 1.0 };
    let curve = resp.prepare(fl.el).unwrap();
    assert_eq!(curve.shadowed(), 0.05);
    let th = invert_direct(&fl, 0.2, &curve, 0.25);
    assert!((curve.direct(fl.cos_tilt(th), 1.0) - 0.25).abs() < 0.01);
    assert!(Response::Measured { tilt_rad: vec![0.1, 0.1], brightness: vec![0.0, 1.0] }.check().is_err());
}

#[test]
fn match_score_basics() {
    let t = [0.1, 0.2, 0.3, 0.4];
    let w = [1.0; 4];
    let s = match_score(&t, &t, &w, (0.0, 0.5));
    assert!((s.pearson - 1.0).abs() < 1e-12 && s.rmse == 0.0);
    let b = [0.2, 0.3, 0.4, 0.5];
    let s = match_score(&b, &t, &w, (0.0, 0.5));
    assert!((s.pearson - 1.0).abs() < 1e-12 && (s.rmse - 0.1).abs() < 1e-12 && (s.rmse_rel - 0.2).abs() < 1e-12);
    let s = match_score(&[0.4, 0.3, 0.2, 0.1], &t, &w, (0.0, 0.5));
    assert!((s.pearson + 1.0).abs() < 1e-12);
}

#[test]
fn render_flat_and_shadowed() {
    let p = square_grid(4, 2, 5.0, DVec2::ZERO);
    let el = 5f64.to_radians();
    let light = Light::Directional { az: std::f64::consts::PI, el };
    let f = grid_facets(4, 2, 5.0, -DVec2::X, &[0.0; 8], 3.0);
    let img = render(&p, &f, &light, &Response::default(), &RenderOpts { px_per_mm: 2.0, supersample: 2, white: Some(el.sin()) })
        .unwrap();
    assert_eq!((img.w, img.h), (40, 20));
    assert!(img.px.iter().all(|&v| (v - 1.0).abs() < 1e-6));
    // Tilted toward the light: every column but the first has a dark band at its
    // left (light-side) edge, so the mean is sin α cos θ / sin(α+θ) of white.
    let th = 10f64.to_radians();
    let f = grid_facets(4, 2, 5.0, -DVec2::X, &[th; 8], 3.0);
    let img = render(&p, &f, &light, &Response::default(), &RenderOpts { px_per_mm: 8.0, supersample: 2, white: None })
        .unwrap();
    let w = img.w as usize;
    let col_mean = |c0: usize, c1: usize| {
        let mut s = 0.0;
        for r in 0..img.h as usize {
            for c in c0..c1 {
                s += img.px[r * w + c] as f64;
            }
        }
        s / ((c1 - c0) * img.h as usize) as f64
    };
    assert!((col_mean(0, 40) - 1.0).abs() < 1e-6);
    let want = el.tan() / (el.tan() + th.tan());
    assert!((col_mean(40, 80) - want).abs() < 0.01, "{} vs {want}", col_mean(40, 80));
    let g = render::to_gray8_srgb(&img);
    assert_eq!(g.dimensions(), (img.w, img.h));
}

/// A lamp exactly in line with a shared cell edge (and nearly so, where rounding
/// decides): every light line must be split into contiguous, non-overlapping
/// segments, one owner per point, so no area is counted twice or dropped.
#[test]
fn lines_on_shared_edges_partition_exactly() {
    let (nx, ny, cell) = (8u32, 4u32, 5.0);
    let p = square_grid(nx, ny, cell, DVec2::ZERO);
    let f = grid_facets(nx, ny, cell, -DVec2::X, &vec![0.0; 32], 4.0);
    let el = 8f64.to_radians();
    for dist in [1e2, 1e3, 1e4, 1e5, 1e6] {
        for y in [5.0, 10.0, 10.0 + 1e-9, 15.0] {
            let lamp = DVec3::new(20.0 - dist * el.cos(), y, dist * el.sin());
            for lpc in [1, 3, 8] {
                let ls = line_set(&p, &f, &Light::Point { pos_mm: lamp }, lpc).unwrap();
                for (li, l) in ls.lines.iter().enumerate() {
                    let sg = &ls.segs[l.segs.start as usize..l.segs.end as usize];
                    assert!(!sg.is_empty(), "d {dist} y {y} lpc {lpc}: line {li} empty");
                    for w in sg.windows(2) {
                        let gap = w[1].s0 - w[0].s1;
                        assert!(gap.abs() < 1e-6 * dist.max(1.0), "d {dist} y {y} lpc {lpc}: line {li} gap/overlap {gap:e}");
                    }
                    // A distant lamp's (near-parallel) rays cross the whole 40 mm
                    // plate; near lamps' outer rays clip corners.
                    let len = sg.last().unwrap().s1 - sg[0].s0;
                    assert!(dist < 1e4 || (len - 40.0).abs() < 0.5, "d {dist} y {y} lpc {lpc}: line {li} length {len}");
                }
            }
        }
    }
}
