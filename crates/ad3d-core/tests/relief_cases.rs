//! Hand-built mesher cases (design §6.1, §7) and adversarial integer heights.

mod common;

use common::{assert_ok, build_grid};
use glam::DVec2;
use proptest::prelude::*;

const B: i64 = 3000;
const O: DVec2 = DVec2::ZERO;

#[test]
fn single_flat_cell() {
    let b = build_grid(1, 1, 6.0, O, &[[B; 4]], B);
    assert_ok(&b);
    // 8 corner/back verts + apex; 4 facet tris + 8 wall tris + 2 back tris.
    assert_eq!(b.mesh.vertex_count(), 9);
    assert_eq!(b.mesh.triangle_count(), 14);
    assert!((b.report.written.signed_volume - 36.0 * 3.0).abs() < 1e-3);
}

#[test]
fn single_tilted_cell() {
    let b = build_grid(1, 1, 6.0, O, &[[B, B + 500, B + 1000, B + 500]], B);
    assert_ok(&b);
}

#[test]
fn saddle_ac_high_is_split() {
    // 2×2: D=(0,0) C=(1,0) A=(0,1) B=(1,1). A and C high at the centre.
    let hi = [B + 400; 4];
    let lo = [B; 4];
    let b = build_grid(2, 2, 6.0, O, &[lo, hi, hi, lo], B);
    assert_eq!(b.stats.saddles, 1);
    assert_ok(&b);
}

#[test]
fn saddle_bd_high_is_split() {
    let hi = [B + 400; 4];
    let lo = [B; 4];
    let b = build_grid(2, 2, 6.0, O, &[hi, lo, lo, hi], B);
    assert_eq!(b.stats.saddles, 1);
    assert_ok(&b);
}

#[test]
fn saddle_with_corner_heights_only() {
    // Saddle decided by the centre heights alone; outer corners all equal.
    let c = |k: usize, h: i64| {
        let mut a = [B; 4];
        a[k] = h;
        a
    };
    // D corner 2, C corner 3, A corner 1, B corner 0 at the centre vertex.
    let b = build_grid(2, 2, 6.0, O, &[c(2, B + 100), c(3, B + 300), c(1, B + 300), c(0, B + 100)], B);
    assert_eq!(b.stats.saddles, 1);
    assert_ok(&b);
}

#[test]
fn non_saddle_four_way_vertex_is_not_split() {
    let b = build_grid(2, 2, 6.0, O, &[[B; 4], [B + 100; 4], [B + 200; 4], [B + 300; 4]], B);
    assert_eq!(b.stats.saddles, 0);
    assert_ok(&b);
}

#[test]
fn ties_everywhere_make_no_interior_walls() {
    let b = build_grid(3, 3, 6.0, O, &vec![[B; 4]; 9], B);
    assert_ok(&b);
    assert_eq!(b.stats.walls, 12); // boundary edges only
    assert_eq!(b.stats.crossings, 0);
}

#[test]
fn equal_profiles_share_an_edge() {
    // Shared edge (1,0)-(1,1) has heights B+100, B+200 in both cells.
    let b = build_grid(2, 1, 6.0, O, &[[B, B + 100, B + 200, B], [B + 100, B + 300, B + 300, B + 200]], B);
    assert_ok(&b);
    assert_eq!(b.stats.walls, 6);
}

#[test]
fn near_ties_cluster_up() {
    // 30 µm apart: clustered to the higher value, so no wall.
    let b = build_grid(2, 1, 6.0, O, &[[B; 4], [B + 30; 4]], B);
    assert_ok(&b);
    assert!(b.report.min_distinct_distance.is_none_or(|d| d >= 0.05));
}

#[test]
fn wedge_point_at_a() {
    // Design B2: (B, B, B+51, B) beside a flat cell: d0 = 0 at the shared edge's start.
    let b = build_grid(2, 1, 6.0, O, &[[B, B, B + 51, B], [B; 4]], B);
    assert_ok(&b);
}

#[test]
fn wedge_point_at_b() {
    let b = build_grid(2, 1, 6.0, O, &[[B, B + 51, B, B], [B; 4]], B);
    assert_ok(&b);
}

#[test]
fn wedge_both_orientations_and_sides() {
    for h in [[B, B, B + 400, B], [B, B + 400, B, B]] {
        assert_ok(&build_grid(2, 1, 6.0, O, &[h, [B; 4]], B));
        assert_ok(&build_grid(2, 1, 6.0, O, &[[B + 400; 4], h], B));
    }
}

#[test]
fn crossing_wall() {
    // Left cell's right edge rises, right cell's left edge falls: one crossing.
    let b = build_grid(2, 1, 6.0, O, &[[B, B, B + 500, B + 500], [B + 500, B + 500, B, B]], B);
    assert_eq!(b.stats.crossings, 1);
    assert_ok(&b);
}

#[test]
fn crossing_clamped_away_from_edge_end() {
    // d0 = 51, d1 = −10000 → natural t = 51/10051 (5 µm along a 1 mm edge); clamp to δ.
    let b = build_grid(2, 1, 1.0, O, &[[B, B + 51, B, B], [B, B, B, B + 10000]], B);
    assert_eq!(b.stats.crossings, 1);
    assert_ok(&b);
}

#[test]
fn far_from_origin_f32() {
    let b = build_grid(3, 3, 1.0, DVec2::new(1990.0, 1990.0), &checker(3, 3, B, B + 200), B);
    assert_ok(&b);
}

#[test]
fn saddle_at_every_vertex() {
    let b = build_grid(6, 5, 2.0, O, &checker(6, 5, B, B + 300), B);
    assert_eq!(b.stats.saddles, 5 * 4);
    assert_ok(&b);
}

#[test]
fn one_by_n_and_n_by_one() {
    for (nx, ny) in [(1, 7), (7, 1)] {
        let b = build_grid(nx, ny, 3.0, O, &checker(nx, ny, B, B + 300), B);
        assert_ok(&b);
    }
}

fn checker(nx: u32, ny: u32, lo: i64, hi: i64) -> Vec<[i64; 4]> {
    (0..ny).flat_map(|j| (0..nx).map(move |i| if (i + j) % 2 == 0 { [lo; 4] } else { [hi; 4] })).collect()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 400, .. ProptestConfig::default() })]

    /// Adversarial integer heights (ported from recheck B3): every corner drawn from a
    /// small alphabet 40 µm apart (ties and near-ties within τ), random grid shapes.
    #[test]
    fn adversarial_alphabet_heights(
        shape in prop::sample::select(vec![(2u32, 2u32), (3, 3), (4, 4), (1, 5), (5, 1), (6, 5)]),
        seed in prop::collection::vec(0i64..6, 120),
        cell in prop::sample::select(vec![1.0f64, 2.0, 6.0]),
        far in any::<bool>(),
    ) {
        let (nx, ny) = shape;
        let n = (nx * ny) as usize;
        let corners: Vec<[i64; 4]> = (0..n).map(|c| {
            let s = |k: usize| B + 40 * seed[(4 * c + k) % seed.len()];
            [s(0), s(1), s(2), s(3)]
        }).collect();
        let origin = if far { DVec2::new(1990.0, 1990.0) } else { O };
        let b = build_grid(nx, ny, cell, origin, &corners, B);
        prop_assert!(b.report.ok, "{}", b.report.summary());
    }

    /// Wide random integer heights including large crossings and per-vertex saddles.
    #[test]
    fn random_integer_heights(
        nx in 1u32..7, ny in 1u32..7,
        hs in prop::collection::vec(0i64..3000, 196),
        cell in 0.5f64..8.0,
    ) {
        let n = (nx * ny) as usize;
        let corners: Vec<[i64; 4]> = (0..n).map(|c| [hs[4*c] + 400, hs[4*c+1] + 400, hs[4*c+2] + 400, hs[4*c+3] + 400]).collect();
        let b = build_grid(nx, ny, cell, O, &corners, 400);
        prop_assert!(b.report.ok, "{}", b.report.summary());
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 300, .. ProptestConfig::default() })]

    /// Random per-facet pivot height and gradient (stand-ins for the optimiser and
    /// point lights) within the §4 per-corner rule, through quantise() and clustering,
    /// with plate offsets up to ~2048 mm.
    #[test]
    fn random_facets_through_quantise(
        nx in 1u32..9, ny in 1u32..9,
        cell in prop::sample::select(vec![0.5f64, 1.0, 2.5, 6.0, 10.0]),
        slopes in prop::collection::vec((-0.364f64..0.364, -0.364f64..0.364, 0.0f64..1.0), 64),
        base in prop::sample::select(vec![0.4f64, 3.0]),
        far in any::<bool>(),
    ) {
        use ad3d_core::CancelToken;
        use ad3d_core::mesh::validate::{ValidateOpts, validate};
        use ad3d_core::relief::{Facet, PieceSet, Tol, mesh_relief, quantise, square_grid};
        let tol = Tol::default();
        let origin = if far { DVec2::new(1990.0, 1990.0) } else { O };
        let part = square_grid(nx, ny, cell, origin);
        let facets: Vec<Facet> = (0..nx * ny).map(|k| {
            let (gx, gy, extra) = slopes[k as usize % slopes.len()];
            let (i, j) = (k % nx, k / nx);
            let pivot = origin + DVec2::new((i as f64 + 0.5) * cell, (j as f64 + 0.5) * cell);
            let lift = 0.5 * cell * (gx.abs() + gy.abs());
            Facet { pivot, h: base + lift + tol.q + extra, grad: DVec2::new(gx, gy) }
        }).collect();
        let h = quantise(&part, &facets, base, &tol).unwrap();
        let m = mesh_relief(&part, PieceSet::All, &h, &[], &tol, &CancelToken::new()).unwrap();
        let r = validate(&m, &ValidateOpts {
            min_facet_w: Some(base - tol.q / 2.0),
            back_area: Some(part.outline_area()),
            ..Default::default()
        });
        prop_assert!(r.ok, "{}", r.summary());
    }
}

#[test]
fn subset_of_pieces_meshes_as_its_own_part() {
    use ad3d_core::CancelToken;
    use ad3d_core::mesh::validate::{ValidateOpts, validate};
    use ad3d_core::relief::{Heights, PieceSet, Tol, cluster_heights, mesh_relief, square_grid};
    // A 4×4 checkerboard (saddles everywhere); mesh the lower-left 2×3 block as a tile.
    let tol = Tol::default();
    let part = square_grid(4, 4, 3.0, O);
    let cs = checker(4, 4, B, B + 300);
    let mut corner: Vec<i64> = cs.iter().flat_map(|c| c.iter().copied()).collect();
    cluster_heights(&part, &mut corner, tol.tau_q);
    let h = Heights { corner, apex_w: cs.iter().map(|c| c[0] as f64 * tol.q).collect() };
    let tile: Vec<u32> = vec![0, 1, 4, 5, 8, 9];
    let m = mesh_relief(&part, PieceSet::Subset(&tile), &h, &[], &tol, &CancelToken::new()).unwrap();
    let r = validate(&m, &ValidateOpts { back_area: Some(6.0 * 9.0), ..Default::default() });
    assert!(r.ok, "{}", r.summary());
}

fn tri_area_ok(pts: &[DVec2], tris: &[[u32; 3]], expect: f64) {
    let mut sum = 0.0;
    for t in tris {
        let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
        let cr = (b - a).perp_dot(c - a);
        assert!(cr > 0.0, "triangle {t:?} not strictly CCW");
        sum += cr * 0.5;
    }
    assert!((sum - expect).abs() < 1e-9 * expect.max(1.0), "{sum} vs {expect}");
    assert_eq!(tris.len(), pts.len() - 2);
}

#[test]
fn back_face_strip_and_cdt_cover_the_outline() {
    use ad3d_core::relief::{square_grid, triangulate_convex_strip, triangulate_outline_cdt};
    for (nx, ny) in [(1, 1), (1, 7), (7, 1), (2, 3), (50, 63), (100, 126)] {
        let p = square_grid(nx, ny, 3.0, O);
        let pts: Vec<DVec2> = p.outline.iter().map(|&v| p.verts[v as usize]).collect();
        let area = (nx * ny) as f64 * 9.0;
        tri_area_ok(&pts, &triangulate_convex_strip(&pts).unwrap(), area);
        tri_area_ok(&pts, &triangulate_outline_cdt(&pts).unwrap(), area);
    }
    // Non-convex L shape goes through the CDT fallback.
    let l = [(0., 0.), (2., 0.), (2., 1.), (1., 1.), (1., 2.), (0., 2.)].map(|(x, y)| DVec2::new(x, y));
    assert!(triangulate_convex_strip(&l).is_none());
    tri_area_ok(&l, &ad3d_core::relief::triangulate_outline(&l).unwrap(), 3.0);
}
