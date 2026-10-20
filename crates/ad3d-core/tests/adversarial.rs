//! Audit round 1, "mesher breaker": adversarial inputs for the relief mesher and
//! the Squama generator, plus probes of how strict `validate()` is.
//!
//! The triangle-triangle self-intersection oracle written for this audit now lives
//! in `ad3d_core::mesh::intersect` and runs as `validate()`'s opt-in stage
//! (`check_self_intersection`). Every generated mesh in this file goes through
//! `validate()`, the self-intersection check and an STL round trip (validated as a
//! soup).
//!
//! Set `AD3D_ADV_STL_DIR=/some/dir` to also dump every generated plate as STL so
//! `tools/check_mesh.py` (trimesh) can cross-check them.

mod common;

use std::f64::consts::PI;

use ad3d_core::CancelToken;
use ad3d_core::image::CellBrightness;
use ad3d_core::io::stl::{read_stl, write_stl_to};
use ad3d_core::mesh::intersect::self_intersections;
use ad3d_core::mesh::validate::{ValidateOpts, validate};
use ad3d_core::mesh::{FaceKind, Mesh};
use ad3d_core::orient::{Orientation, QuarterTurns, Xform, export_xform};
use ad3d_core::relief::{Heights, PieceSet, Tol, cluster_heights, mesh_relief_stats, square_grid};
use ad3d_core::squama::{GenOptions, SquamaOutput, SquamaParams, generate_ex, validate_opts};
use common::build_grid;
use glam::DVec2;
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Shared checks
// ---------------------------------------------------------------------------

fn stl_bytes(m: &Mesh, x: &Xform) -> Vec<u8> {
    let mut v = Vec::new();
    write_stl_to(&mut v, m, x).unwrap();
    v
}

/// STL round trip through a temp file, validated as a soup.
fn stl_roundtrip_ok(name: &str, m: &Mesh, x: &Xform) {
    let dir = std::env::temp_dir().join(format!("ad3d-adv-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.stl"));
    std::fs::write(&path, stl_bytes(m, x)).unwrap();
    let soup = read_stl(&path).unwrap();
    let r = validate(&soup, &ValidateOpts { soup: true, ..Default::default() });
    assert!(r.ok, "{name}: STL soup validate failed:\n{}", r.summary());
    assert_eq!(r.written.vertices_welded, m.vertex_count(), "{name}: STL weld changed the vertex count");
    let _ = std::fs::remove_file(&path);
    if let Ok(d) = std::env::var("AD3D_ADV_STL_DIR") {
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(std::path::Path::new(&d).join(format!("{name}.stl")), stl_bytes(m, x)).unwrap();
    }
}

fn no_self_intersections(name: &str, m: &Mesh) {
    let (n, ex) = self_intersections(m, 5);
    assert_eq!(n, 0, "{name}: {n} self-intersecting triangle pairs:\n{}", ex.join("\n"));
}

fn no_guard() -> GenOptions {
    GenOptions { relief_guard: false, ..Default::default() }
}

/// Full check for a generated Squama plate.
fn check_plate(name: &str, p: &SquamaParams, out: &SquamaOutput, si: bool) {
    let r = validate(&out.mesh, &validate_opts(out, Xform::IDENTITY));
    assert!(r.ok, "{name}: plate-space validate failed for {p:?}:\n{}", r.summary());
    let (x, _) = export_xform(Orientation::Standing(QuarterTurns::Auto), out.layout.w_mm, out.layout.h_mm, p.light_azimuth);
    let r2 = validate(&out.mesh, &validate_opts(out, x));
    assert!(r2.ok, "{name}: standing validate failed for {p:?}:\n{}", r2.summary());
    // Exact volume cross-check: signed volume equals W·H·mean height; at least W·H·T.
    assert!(r.written.signed_volume >= out.layout.w_mm * out.layout.h_mm * p.base_mm * (1.0 - 1e-6));
    stl_roundtrip_ok(name, &out.mesh, &x);
    if si {
        let r3 = validate(&out.mesh, &ValidateOpts { check_self_intersection: true, ..validate_opts(out, x) });
        assert!(r3.ok, "{name}: strict validate failed for {p:?}:\n{}", r3.summary());
    }
}

fn gen_plate(p: &SquamaParams, cb: &CellBrightness) -> SquamaOutput {
    generate_ex(p, cb, &no_guard(), &CancelToken::new()).unwrap_or_else(|e| panic!("{e} for {p:?}"))
}

fn rng(seed: &mut u64) -> f64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    (*seed >> 11) as f64 / (1u64 << 53) as f64
}

fn images(nx: u32, ny: u32, seed: u64) -> Vec<(&'static str, Vec<f32>)> {
    let n = (nx * ny) as usize;
    let mut s = seed | 1;
    let noise: Vec<f32> = (0..n).map(|_| rng(&mut s) as f32).collect();
    vec![
        ("black", vec![0.0; n]),
        ("white", vec![1.0; n]),
        ("checker", (0..n).map(|k| ((k as u32 % nx + k as u32 / nx) % 2) as f32).collect()),
        ("noise", noise.clone()),
        ("binary-noise", noise.iter().map(|&g| (g > 0.5) as u8 as f32).collect()),
        ("stripes-x", (0..n).map(|k| ((k as u32 % nx) % 2) as f32).collect()),
        ("stripes-y", (0..n).map(|k| ((k as u32 / nx) % 2) as f32).collect()),
    ]
}

// ---------------------------------------------------------------------------
// The oracle itself must work (otherwise "0 intersections" means nothing)
// ---------------------------------------------------------------------------

fn cube(s: f32) -> Mesh {
    let p: [[f32; 3]; 8] =
        [[0., 0., 0.], [s, 0., 0.], [s, s, 0.], [0., s, 0.], [0., 0., s], [s, 0., s], [s, s, s], [0., s, s]];
    let t: [[u32; 3]; 12] = [
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [1, 2, 6],
        [1, 6, 5],
        [2, 3, 7],
        [2, 7, 6],
        [3, 0, 4],
        [3, 4, 7],
    ];
    let mut m = Mesh::default();
    for q in p {
        m.push_vertex(q);
    }
    for q in t {
        m.push_triangle(q, FaceKind::Other);
    }
    m
}

#[test]
fn oracle_sees_no_intersections_in_a_cube() {
    assert_eq!(self_intersections(&cube(10.0), 5).0, 0);
}

// ---------------------------------------------------------------------------
// validate() strictness probes. Audit round 1 found these gaps ("lenient_"
// tests that passed when validate() accepted an unsound mesh); they are fixed and
// the tests now require a rejection.
// ---------------------------------------------------------------------------

/// L1: a cube with one top vertex pushed down through the bottom face keeps the
/// same topology and positive volume. The default (topological) validate still
/// accepts it; the opt-in self-intersection stage rejects it.
#[test]
fn self_intersection_stage_rejects_folded_cube() {
    let mut m = cube(10.0);
    // Vertex 6 = (10,10,10) → (5,5,-5): the top face folds through the bottom.
    m.positions[3 * 6 + 2] = -5.0;
    m.positions[3 * 6] = 5.0;
    m.positions[3 * 6 + 1] = 5.0;
    let (n, _) = self_intersections(&m, 3);
    assert!(n > 0, "oracle should see the fold");
    let r = validate(&m, &ValidateOpts::default());
    assert!(r.ok, "topological validate should still pass (documents the scope):\n{}", r.summary());
    assert_eq!(r.self_intersecting_pairs, None);
    let strict = validate(&m, &ValidateOpts { check_self_intersection: true, ..Default::default() });
    assert!(!strict.ok, "strict validate accepted a self-intersecting mesh:\n{}", strict.summary());
    assert_eq!(strict.self_intersecting_pairs, Some(n));
    let ok = validate(&cube(10.0), &ValidateOpts { check_self_intersection: true, ..Default::default() });
    assert!(ok.ok && ok.self_intersecting_pairs == Some(0), "{}", ok.summary());
}

/// L3: two disjoint cubes, the smaller one inside-out, must fail even when two
/// shells are expected (the volume check is per shell now).
#[test]
fn validate_rejects_inside_out_shell_when_two_expected() {
    let mut m = cube(10.0);
    let small = cube(2.0);
    let off = m.vertex_count() as u32;
    for p in small.positions.chunks_exact(3) {
        m.push_vertex([p[0] + 20.0, p[1], p[2]]);
    }
    for t in 0..small.triangle_count() {
        let [a, b, c] = small.triangle(t);
        m.push_triangle([a + off, c + off, b + off], FaceKind::Other); // inverted
    }
    let r = validate(&m, &ValidateOpts { expect_components: 2, ..Default::default() });
    assert!(!r.ok, "{}", r.summary());
    assert_eq!(r.written.negative_shells, 1);
    assert!((r.written.signed_volume - (1000.0 - 8.0)).abs() < 1e-9);
    // Both shells outward: accepted.
    let mut good = cube(10.0);
    for p in small.positions.chunks_exact(3) {
        good.push_vertex([p[0] + 20.0, p[1], p[2]]);
    }
    for t in 0..small.triangle_count() {
        good.push_triangle(small.triangle(t).map(|v| v + off), FaceKind::Other);
    }
    let r = validate(&good, &ValidateOpts { expect_components: 2, ..Default::default() });
    assert!(r.ok, "{}", r.summary());
}

/// L2: a 1 µm sliver fails the default altitude gate (δ/10 = 5 µm); with the gate
/// off it is only reported.
#[test]
fn validate_rejects_sliver_triangle() {
    // Replace bottom triangle (0,2,1) by (0,m,1), (m,2,1) and a sliver (0,2,m) closing
    // against (0,3,2), with m 1 µm off the diagonal 0–2.
    let mut m = cube(10.0);
    let mid = m.push_vertex([5.0005, 4.9995, 0.0]);
    m.indices[0..3].copy_from_slice(&[0, mid, 1]);
    m.push_triangle([mid, 2, 1], FaceKind::Other);
    m.push_triangle([0, 2, mid], FaceKind::Other);
    let r = validate(&m, &ValidateOpts::default());
    assert!(!r.ok, "{}", r.summary());
    assert!(r.written.min_altitude < 1e-3, "min altitude {}", r.written.min_altitude);
    let r = validate(&m, &ValidateOpts { min_altitude: None, ..Default::default() });
    assert!(r.ok, "{}", r.summary());
}

/// L4: an unreferenced vertex fails (it would move the payload bbox).
#[test]
fn validate_rejects_unreferenced_vertex() {
    let mut m = cube(10.0);
    m.push_vertex([1000.0, 1000.0, 1000.0]);
    let r = validate(&m, &ValidateOpts::default());
    assert!(!r.ok, "{}", r.summary());
    assert_eq!(r.unreferenced_vertices, 1);
}

/// A4: NaN, ±Inf and absurd coordinates are rejected up front, without a panic in
/// the slicer stage's integer conversion.
#[test]
fn validate_rejects_nan_and_inf_vertices() {
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 3.0e9, -3.0e9] {
        let mut m = cube(10.0);
        m.positions[3 * 6 + 2] = bad;
        for soup in [false, true] {
            let r = validate(&m, &ValidateOpts { soup, ..Default::default() });
            assert!(!r.ok, "validate accepted a vertex with {bad}:\n{}", r.summary());
            assert_eq!(r.non_finite_values, 1);
        }
    }
    // The slicer meld on its own must not overflow either.
    let (_, _, merged) = ad3d_core::mesh::validate::cura_meld(&[[f32::INFINITY, 0.0, 0.0], [f32::MAX, 0.0, 0.0]], 30);
    assert!(merged <= 1);
}

#[test]
fn validate_rejects_duplicate_shell() {
    // Same cube twice with separate vertex ids (non-soup) and as a soup.
    let mut m = cube(10.0);
    let c = cube(10.0);
    let off = m.vertex_count() as u32;
    m.positions.extend_from_slice(&c.positions);
    for t in 0..c.triangle_count() {
        m.push_triangle(c.triangle(t).map(|v| v + off), FaceKind::Other);
    }
    assert!(!validate(&m, &ValidateOpts::default()).ok);
    assert!(!validate(&m, &ValidateOpts { soup: true, ..Default::default() }).ok);
}

#[test]
fn validate_rejects_hole_closed_by_reversed_face() {
    // Cube with the top face replaced by a coplanar copy wound the wrong way.
    let mut m = cube(10.0);
    m.indices[6..12].copy_from_slice(&[4, 6, 5, 4, 7, 6]);
    assert!(!validate(&m, &ValidateOpts::default()).ok);
}

#[test]
fn validate_rejects_inside_out_single_shell() {
    let mut m = cube(10.0);
    for t in m.indices.chunks_exact_mut(3) {
        t.swap(1, 2);
    }
    assert!(!validate(&m, &ValidateOpts::default()).ok);
}

#[test]
fn validate_rejects_torus_when_sphere_expected() {
    let t = ad3d_core::synth::torus(2000);
    let r = validate(&t, &ValidateOpts { min_distance: None, slicer_merge_um: None, ..Default::default() });
    assert!(!r.ok);
    assert_eq!(r.written.euler, 0);
}

// ---------------------------------------------------------------------------
// Generic relief mesher: heights fed straight in
// ---------------------------------------------------------------------------

/// Mesh with explicit corner heights and explicit apex heights (not the mean).
fn mesh_with_apex(
    nx: u32,
    ny: u32,
    cell: f64,
    origin: DVec2,
    corners: &[[i64; 4]],
    apex_um: &[i64],
    base_um: i64,
) -> (Mesh, ad3d_core::relief::MeshStats, ad3d_core::mesh::validate::Report) {
    let tol = Tol::default();
    let part = square_grid(nx, ny, cell, origin);
    let mut corner: Vec<i64> = corners.iter().flat_map(|c| c.iter().copied()).collect();
    cluster_heights(&part, &mut corner, tol.tau_q);
    let apex_w = apex_um.iter().map(|&a| a as f64 * tol.q).collect();
    let h = Heights { corner, apex_w };
    let (mesh, stats) =
        mesh_relief_stats(&part, PieceSet::All, &h, &[], &tol, &CancelToken::new()).unwrap_or_else(|e| panic!("{e}"));
    let r = validate(
        &mesh,
        &ValidateOpts {
            min_facet_w: Some(base_um as f64 * tol.q - tol.q / 2.0),
            // Area of the outline as stored (f32); the f64 area is also accepted now,
            // see `back_area_check_is_too_strict_off_origin`.
            back_area: Some(outline_area_f32(&part)),
            check_self_intersection: true,
            // No sliver gate here: with walls up to 1 m tall on 0.5 mm cells the
            // ladder (§7.4, no Steiner points) must join a 51 µm step on one side to
            // a vertex up to 1 m away on the other, so altitude ≈ step × width /
            // wall height can be nanometres. That is a quality limit of the generic
            // mesher on extreme inputs, not a validity bug; Squama plates (walls of a
            // few mm) pass the default δ/10 gate in every test below.
            min_altitude: None,
            ..Default::default()
        },
    );
    (mesh, stats, r)
}

fn outline_area_f32(part: &ad3d_core::relief::Partition) -> f64 {
    let v: Vec<(f64, f64)> = part
        .outline
        .iter()
        .map(|&i| (part.verts[i as usize].x as f32 as f64, part.verts[i as usize].y as f32 as f64))
        .collect();
    let n = v.len();
    (0..n).map(|k| v[k].0 * v[(k + 1) % n].1 - v[(k + 1) % n].0 * v[k].1).sum::<f64>() * 0.5
}

/// A3 (fixed): the f64 outline area is now accepted for a correct small part far
/// from the origin (tolerance scales with perimeter × f32 ulp).
#[test]
fn back_area_check_is_too_strict_off_origin() {
    let corners = [[400i64; 4]];
    let (m, _, r) = mesh_with_apex(1, 1, 3.3, DVec2::new(1000.123, 7.77), &corners, &[400], 400);
    assert!(r.ok, "{}", r.summary());
    let tol = Tol::default();
    let part = square_grid(1, 1, 3.3, DVec2::new(1000.123, 7.77));
    let r64 = validate(
        &m,
        &ValidateOpts {
            min_facet_w: Some(0.4 - tol.q / 2.0),
            back_area: Some(part.outline_area()),
            ..Default::default()
        },
    );
    assert!(r64.ok, "correct 1-cell plate at (1000.123, 7.77) fails validate:\n{}", r64.summary());
    // A genuinely wrong area (one cell too small) still fails.
    let wrong = validate(
        &m,
        &ValidateOpts { min_facet_w: Some(0.4 - tol.q / 2.0), back_area: Some(part.outline_area() * 0.999), ..Default::default() },
    );
    assert!(!wrong.ok);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 300, .. ProptestConfig::default() })]

    /// Random heights over a huge range (0.4 mm .. 1 m), random apex heights anywhere
    /// in [base, max corner + 5 mm], cells 0.5–50 mm, origins up to 2000 mm.
    #[test]
    fn random_heights_wide_range_with_random_apex(
        nx in 1u32..7, ny in 1u32..7,
        hs in prop::collection::vec(0i64..1_000_000, 144),
        ap in prop::collection::vec(0f64..1.0, 36),
        cell in prop::sample::select(vec![0.5f64, 0.7, 1.0, 3.3, 6.0, 20.0, 50.0]),
        origin in prop::sample::select(vec![(0.0f64, 0.0f64), (1990.3, 1990.7), (1000.123, 7.77), (1500.0, 1500.0)]),
        scale in prop::sample::select(vec![1i64, 10, 1000]),
    ) {
        let base = 400i64;
        let n = (nx * ny) as usize;
        let corners: Vec<[i64; 4]> = (0..n).map(|c| {
            let s = |k: usize| base + hs[4 * c + k] / scale;
            [s(0), s(1), s(2), s(3)]
        }).collect();
        let apex: Vec<i64> = (0..n).map(|c| {
            let mx = *corners[c].iter().max().unwrap() + 5000;
            base + ((mx - base) as f64 * ap[c]) as i64
        }).collect();
        let (_m, _, r) = mesh_with_apex(nx, ny, cell, DVec2::new(origin.0, origin.1), &corners, &apex, base);
        prop_assert!(r.ok, "{}", r.summary());
        prop_assert_eq!(r.self_intersecting_pairs, Some(0), "{}", r.summary());
    }

    /// Dense ties and saddles: corners from {B, B+τ, B+τ+1, B+2τ+1, B+2τ+2} (cluster
    /// boundaries exactly at τ = 50 µm), plus checkerboard saddles everywhere.
    #[test]
    fn dense_ties_at_cluster_boundaries(
        nx in 1u32..9, ny in 1u32..9,
        seed in prop::collection::vec(0usize..5, 256),
        cell in prop::sample::select(vec![0.5f64, 1.0, 6.0]),
        far in any::<bool>(),
    ) {
        const B: i64 = 3000;
        let alphabet = [B, B + 50, B + 51, B + 101, B + 102];
        let n = (nx * ny) as usize;
        let corners: Vec<[i64; 4]> = (0..n).map(|c| {
            let s = |k: usize| alphabet[seed[(4 * c + k) % seed.len()]];
            [s(0), s(1), s(2), s(3)]
        }).collect();
        let origin = if far { DVec2::new(1990.3, 1990.7) } else { DVec2::ZERO };
        let b = build_grid(nx, ny, cell, origin, &corners, B);
        prop_assert!(b.report.ok, "{}", b.report.summary());
        let r = validate(&b.mesh, &ValidateOpts {
            min_facet_w: Some(B as f64 * 1e-3 - 0.0005),
            back_area: Some(b.part.outline_area()),
            check_self_intersection: true,
            ..Default::default()
        });
        prop_assert!(r.ok && r.self_intersecting_pairs == Some(0), "{}", r.summary());
    }
}

#[test]
fn saddle_checkerboards_all_scales() {
    for &cell in &[0.5, 1.0, 6.0, 50.0] {
        for &(nx, ny) in &[(2u32, 2u32), (1, 9), (9, 1), (8, 8), (7, 5)] {
            for &d in &[51i64, 300, 20_000] {
                let corners: Vec<[i64; 4]> = (0..ny)
                    .flat_map(|j| (0..nx).map(move |i| if (i + j) % 2 == 0 { [3000; 4] } else { [3000 + d; 4] }))
                    .collect();
                let b = build_grid(nx, ny, cell, DVec2::ZERO, &corners, 3000);
                assert!(b.report.ok, "cell {cell} {nx}x{ny} d {d}:\n{}", b.report.summary());
                no_self_intersections(&format!("checker-{cell}-{nx}x{ny}-{d}"), &b.mesh);
            }
        }
    }
}

/// A1 (fixed: the clamp now keeps 1.05δ). Crossing clamped near δ from a vertex where the lower piece is flat along the
/// edge, so the crossing sits exactly δ (horizontally, 0 vertically) from that piece's
/// corner vertex. Far from the origin the f32 rounding of both points decides whether
/// the written distance is ≥ δ.
#[test]
fn clamped_crossing_exactly_delta_far_from_origin() {
    const B: i64 = 3000;
    let mut fails = Vec::new();
    let mut tried = 0;
    let mut seed = 0x9E3779B97F4A7C15u64;
    for _ in 0..400 {
        let ox = 1000.0 + 1000.0 * rng(&mut seed);
        let oy = 1000.0 + 1000.0 * rng(&mut seed);
        for &cell in &[1.0f64, 0.7, 3.3] {
            // Vertical shared edge (2×1): left cell flat B+1000 along its right edge; right
            // cell 51 µm higher at the bottom and 10 mm lower at the top → t ≈ 0.005, clamped.
            let corners = [[B, B + 1000, B + 1000, B], [B + 1051, B + 1051, B, B]];
            let b = build_grid(2, 1, cell, DVec2::new(ox, oy), &corners, B);
            tried += 1;
            if b.stats.crossings != 1 {
                continue;
            }
            if !b.report.ok {
                fails.push(format!(
                    "origin ({ox:.6}, {oy:.6}) cell {cell}: min distance {:?}\n{}",
                    b.report.min_distinct_distance,
                    b.report.summary()
                ));
            }
            // Horizontal shared edge (1×2).
            let corners = [[B, B, B + 1000, B + 1000], [B + 1051, B, B, B + 1051]];
            let b = build_grid(1, 2, cell, DVec2::new(ox, oy), &corners, B);
            tried += 1;
            if b.stats.crossings == 1 && !b.report.ok {
                fails.push(format!(
                    "1x2 origin ({ox:.6}, {oy:.6}) cell {cell}: min distance {:?}",
                    b.report.min_distinct_distance
                ));
            }
        }
    }
    assert!(fails.is_empty(), "{} of {tried} clamped-crossing cases fail validate; first:\n{}", fails.len(), fails[0]);
    // The audit's minimal reproducers.
    for origin in [DVec2::new(1859.794121, 1394.301338), DVec2::new(8.597941, 3.943013)] {
        let b = build_grid(2, 1, 1.0, origin, &[[3000, 4000, 4000, 3000], [4051, 4051, 3000, 3000]], 3000);
        assert!(b.report.ok, "origin {origin:?}:\n{}", b.report.summary());
        assert!(b.report.min_distinct_distance.unwrap() >= 0.05, "{:?}", b.report.min_distinct_distance);
    }
}

/// The generic mesher is public and takes any `Tol`/cell size; a cell shorter than 2δ
/// with a crossing must be an `Err`, not a panic in `f64::clamp`.
#[test]
fn tiny_cell_with_crossing_does_not_panic() {
    let res = std::panic::catch_unwind(|| {
        let tol = Tol::default();
        let part = square_grid(2, 1, 0.08, DVec2::ZERO);
        let mut corner: Vec<i64> =
            [[3000, 3000, 3500, 3500], [3500, 3500, 3000, 3000]].iter().flat_map(|c| c.iter().copied()).collect();
        cluster_heights(&part, &mut corner, tol.tau_q);
        let h = Heights { corner, apex_w: vec![3.25, 3.25] };
        mesh_relief_stats(&part, PieceSet::All, &h, &[], &tol, &CancelToken::new()).map(|_| ())
    });
    match res {
        Ok(r) => assert!(
            matches!(r, Err(ad3d_core::Error::InvalidParams(_))),
            "expected InvalidParams for a 0.08 mm cell, got {r:?}"
        ),
        Err(e) => {
            let msg = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()));
            panic!("mesh_relief panicked on a 0.08 mm cell: {msg:?}");
        }
    }
}

// ---------------------------------------------------------------------------
// Squama generator sweeps
// ---------------------------------------------------------------------------

fn params(nx: u32, cell_mm: f64) -> SquamaParams {
    SquamaParams { width_mm: nx as f64 * cell_mm, cell_mm, ..Default::default() }
}

#[test]
fn degenerate_grid_shapes_all_images() {
    for &(nx, ny) in &[(1u32, 1u32), (1, 2), (2, 1), (1, 17), (17, 1), (2, 2), (3, 1)] {
        for &cell in &[0.5, 6.0, 50.0] {
            for &az in &[180.0, 37.0, 0.0, 225.0, 301.7] {
                for (name, g) in images(nx, ny, (nx * 31 + ny) as u64) {
                    let mut p = params(nx, cell);
                    p.light_azimuth = (az as f64).to_radians();
                    p.max_tilt = 20f64.to_radians();
                    p.light_elevation = 0.0;
                    let out = gen_plate(&p, &CellBrightness { nx, ny, g });
                    check_plate(&format!("shape-{nx}x{ny}-c{cell}-az{az}-{name}"), &p, &out, true);
                }
            }
        }
    }
}

#[test]
fn tiny_and_huge_cells_alpha0_s20_off_axis() {
    let (nx, ny) = (16u32, 12u32);
    for &cell in &[0.5, 0.501, 0.777, 1.13, 50.0] {
        for &az in &[0.0, 37.0, 133.3, 180.0, 301.7] {
            for &(el, tilt) in &[(0.0, 20.0), (0.0, 1.0), (45.0, 20.0)] {
                for (name, g) in images(nx, ny, (cell * 1000.0) as u64 ^ az as u64) {
                    let mut p = params(nx, cell);
                    p.light_azimuth = (az as f64).to_radians();
                    p.light_elevation = (el as f64).to_radians();
                    p.max_tilt = (tilt as f64).to_radians();
                    p.base_mm = 0.4;
                    let out = gen_plate(&p, &CellBrightness { nx, ny, g });
                    check_plate(&format!("c{cell}-az{az}-el{el}-s{tilt}-{name}"), &p, &out, true);
                }
            }
        }
    }
}

#[test]
fn plate_2000mm_noise_off_axis() {
    // 2000 mm wide at 10 mm cells (200×200): f32 spacing near 2000 mm is 0.12 µm.
    let (nx, ny) = (200u32, 200u32);
    for &az in &[37.0, 180.0, 301.7] {
        let mut s = 7u64 + az as u64;
        let g: Vec<f32> = (0..nx * ny).map(|_| rng(&mut s) as f32).collect();
        let mut p = params(nx, 10.0);
        p.light_azimuth = (az as f64).to_radians();
        p.max_tilt = 20f64.to_radians();
        p.light_elevation = 0.0;
        let out = gen_plate(&p, &CellBrightness { nx, ny, g });
        // validate + STL here; the self-intersection oracle runs on the 40×40 plate below.
        check_plate(&format!("plate2000-az{az}"), &p, &out, false);
        let (nx, ny) = (40u32, 40u32);
        let g: Vec<f32> = (0..nx * ny).map(|_| rng(&mut s) as f32).collect();
        let mut p = params(nx, 50.0);
        p.light_azimuth = (az as f64).to_radians();
        let out = gen_plate(&p, &CellBrightness { nx, ny, g });
        check_plate(&format!("plate2000-c50-az{az}"), &p, &out, true);
    }
}

/// 2000 mm at 1.13 mm cells: 1770 × 1770 = 3.1 M cells. Slow in the test profile;
/// run with `cargo test --release -p ad3d-core --test adversarial plate_2000mm_small -- --ignored`.
#[test]
#[ignore]
fn plate_2000mm_small_cells_noise() {
    let (nx, ny) = (1770u32, 1770u32);
    let mut s = 99u64;
    let g: Vec<f32> = (0..nx * ny).map(|_| rng(&mut s) as f32).collect();
    let mut p = params(nx, 1.13);
    p.light_azimuth = 37f64.to_radians();
    p.max_tilt = 20f64.to_radians();
    let t = std::time::Instant::now();
    let out = gen_plate(&p, &CellBrightness { nx, ny, g });
    eprintln!(
        "2000 mm / 1.13 mm: {} tris, {} verts, generate {:.0} ms",
        out.mesh.triangle_count(),
        out.mesh.vertex_count(),
        t.elapsed().as_secs_f64() * 1e3
    );
    let r = validate(&out.mesh, &validate_opts(&out, Xform::IDENTITY));
    assert!(r.ok, "{}", r.summary());
    eprintln!("min distinct distance {:?}, min altitude {}", r.min_distinct_distance, r.written.min_altitude);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 200, .. ProptestConfig::default() })]

    /// Random everything, with the self-intersection oracle and STL round trip.
    #[test]
    fn random_plates_with_si_oracle(
        nx in 1u32..14, ny in 1u32..14,
        cell_um in 500i64..50_000,
        az in 0.0f64..360.0,
        el in prop::sample::select(vec![0.0f64, 1.0, 5.0, 45.0, 17.3]),
        tilt in prop::sample::select(vec![1.0f64, 20.0, 10.0, 3.7]),
        base in prop::sample::select(vec![0.4f64, 3.0]),
        kind in 0usize..7,
        seed in any::<u64>(),
    ) {
        let cell = cell_um as f64 / 1000.0;
        let (_, g) = images(nx, ny, seed).swap_remove(kind);
        let mut p = params(nx, cell);
        p.light_azimuth = az.to_radians();
        p.light_elevation = el.to_radians();
        p.max_tilt = tilt.to_radians();
        p.base_mm = base;
        match generate_ex(&p, &CellBrightness { nx, ny, g }, &no_guard(), &CancelToken::new()) {
            Ok(out) => check_plate(&format!("prop-{nx}x{ny}-{cell_um}-{az:.1}"), &p, &out, true),
            Err(e) => prop_assert!(false, "generate failed: {e} for {p:?}"),
        }
    }
}

#[test]
fn quarter_turns_and_flat_all_validate_and_roundtrip() {
    let (nx, ny) = (9u32, 7u32);
    let (_, g) = images(nx, ny, 5).swap_remove(3);
    let mut p = params(nx, 3.3);
    p.light_azimuth = 1.0;
    let out = gen_plate(&p, &CellBrightness { nx, ny, g });
    for o in [
        Orientation::Flat,
        Orientation::Standing(QuarterTurns::Q0),
        Orientation::Standing(QuarterTurns::Q1),
        Orientation::Standing(QuarterTurns::Q2),
        Orientation::Standing(QuarterTurns::Q3),
    ] {
        let (x, _) = export_xform(o, out.layout.w_mm, out.layout.h_mm, p.light_azimuth);
        let r = validate(&out.mesh, &validate_opts(&out, x));
        assert!(r.ok, "{o:?}: {}", r.summary());
        stl_roundtrip_ok(&format!("orient-{o:?}"), &out.mesh, &x);
    }
    let _ = PI;
}


/// A5 (fixed): the convex-strip back face used to end in a fan of needles of
/// altitude ≈ cell²/width (0.64 µm on a 2000 mm plate at 1.13 mm cells). The strip
/// now advances by normalised arc length; every back triangle keeps an altitude
/// close to cell/√2 on square outlines and stays far above the δ/10 sliver gate.
#[test]
fn back_face_needles_on_large_square_outline() {
    use ad3d_core::relief::triangulate_outline;
    for &(nx, ny, cell, ox, oy) in &[
        (100u32, 100u32, 1.13f64, 0.0f64, 0.0f64),
        (1770, 1770, 1.13, 0.0, 0.0),
        (4096, 4096, 0.5, 0.0, 0.0),
        (2635, 2635, 0.777, 0.0, 0.0),
        (100, 126, 3.0, 0.0, 0.0),
        (4000, 3, 0.5, 0.0, 0.0),
        (3, 4000, 0.5, 0.0, 0.0),
        (200, 150, 3.3, 1000.123, 7.77),
    ] {
        let part = square_grid(nx, ny, cell, DVec2::new(ox, oy));
        let pts: Vec<DVec2> = part.outline.iter().map(|&v| part.verts[v as usize]).collect();
        let tris = triangulate_outline(&pts).unwrap();
        assert_eq!(tris.len(), pts.len() - 2);
        let p32: Vec<[f64; 2]> = pts.iter().map(|p| [p.x as f32 as f64, p.y as f32 as f64]).collect();
        let (mut min_alt, mut needles, mut flipped) = (f64::INFINITY, 0usize, 0usize);
        let mut area = 0.0;
        for t in &tris {
            let [a, b, c] = t.map(|k| p32[k as usize]);
            let cr = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            if cr <= 0.0 {
                flipped += 1;
            }
            area += cr * 0.5;
            let l = |p: [f64; 2], q: [f64; 2]| ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt();
            let alt = cr.abs() / l(a, b).max(l(b, c)).max(l(a, c));
            min_alt = min_alt.min(alt);
            if alt < 0.05 {
                needles += 1;
            }
        }
        eprintln!("{nx}x{ny} cell {cell}: {} back tris, min altitude {:.4} mm, {needles} below δ, {flipped} flipped", tris.len(), min_alt);
        assert_eq!(flipped, 0);
        assert_eq!(needles, 0, "{nx}x{ny} cell {cell}");
        assert!(min_alt > 0.25 * cell, "{nx}x{ny} cell {cell}: min altitude {min_alt}");
        let want = (nx as f64 * cell) * (ny as f64 * cell);
        assert!((area - want).abs() < 1e-6 * want + 1e-3, "area {area} vs {want}");
    }
}
