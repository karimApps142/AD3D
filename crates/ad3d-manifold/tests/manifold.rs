//! Integration tests for the manifold FFI: the vendored C++ links, imports
//! meshes, rejects broken ones, and computes booleans with the right volume.

use std::collections::HashMap;
use std::f64::consts::PI;
use std::sync::Arc;

use ad3d_manifold::{BoolOp, Error, ExecutionContext, MANIFOLD_VERSION, Manifold, MeshGL, Status};

/// Axis-aligned box [0, s]^3 as an indexed mesh: 8 vertices, 12 triangles,
/// counter-clockwise seen from outside.
fn cube_mesh(s: f32) -> (Vec<f32>, Vec<u32>) {
    #[rustfmt::skip]
    let positions = vec![
        0.0, 0.0, 0.0,   s, 0.0, 0.0,   s, s, 0.0,   0.0, s, 0.0, // z = 0: 0..3
        0.0, 0.0, s,     s, 0.0, s,     s, s, s,     0.0, s, s,   // z = s: 4..7
    ];
    #[rustfmt::skip]
    let triangles = vec![
        0, 2, 1,  0, 3, 2, // bottom (-z)
        4, 5, 6,  4, 6, 7, // top (+z)
        0, 1, 5,  0, 5, 4, // front (-y)
        2, 3, 7,  2, 7, 6, // back (+y)
        1, 2, 6,  1, 6, 5, // right (+x)
        3, 0, 4,  3, 4, 7, // left (-x)
    ];
    (positions, triangles)
}

/// Volume of a regular n-gon prism with circumradius r and height h, which is
/// exactly what manifold's cylinder is.
fn ngon_prism_volume(n: i32, r: f64, h: f64) -> f64 {
    0.5 * f64::from(n) * r * r * (2.0 * PI / f64::from(n)).sin() * h
}

/// Independent watertightness check on manifold's output: every undirected
/// edge is used exactly twice, once in each direction.
fn assert_closed_and_consistent(tris: &[u32]) {
    let mut directed: HashMap<(u32, u32), u32> = HashMap::new();
    for t in tris.chunks_exact(3) {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            assert_ne!(a, b, "degenerate triangle {t:?}");
            *directed.entry((a, b)).or_default() += 1;
        }
    }
    for (&(a, b), &count) in &directed {
        assert_eq!(count, 1, "directed edge {a}->{b} used {count} times");
        assert_eq!(directed.get(&(b, a)), Some(&1), "edge {a}->{b} has no opposite half-edge");
    }
}

fn assert_close(actual: f64, expected: f64, rel: f64) {
    let err = (actual - expected).abs() / expected.abs().max(1e-12);
    assert!(err <= rel, "expected {expected}, got {actual} (relative error {err:e})");
}

#[test]
fn vendored_version() {
    assert_eq!(MANIFOLD_VERSION, "3.5.4");
}

#[test]
fn cube_from_meshgl_is_valid() {
    let (p, t) = cube_mesh(10.0);
    let mesh = MeshGL::new(&p, &t).unwrap();
    assert_eq!((mesh.num_vert(), mesh.num_tri(), mesh.num_prop()), (8, 12, 3));

    let cube = Manifold::from_meshgl(&mesh);
    assert_eq!(cube.status(), Status::NoError);
    assert!(!cube.is_empty());
    assert_eq!(cube.num_tri(), 12);
    assert_eq!(cube.num_vert(), 8);
    assert_eq!(cube.genus(), 0);
    assert_close(cube.volume(), 1000.0, 1e-12);
    assert_close(cube.surface_area(), 600.0, 1e-12);
    assert_eq!(cube.bounding_box(), Some(([0.0; 3], [10.0; 3])));
}

#[test]
fn cube_minus_cylinder_has_expected_volume() {
    let (p, t) = cube_mesh(20.0);
    let cube = Manifold::from_mesh(&p, &t).unwrap().translate([-10.0, -10.0, -10.0]);
    // A 64-sided, r = 5 mm cylinder through the whole cube (40 mm long, centred).
    let segments = 64;
    let cylinder = Manifold::cylinder(40.0, 5.0, 5.0, segments, true);

    let result = cube.difference(&cylinder);
    assert_eq!(result.status(), Status::NoError);
    let expected = 8000.0 - ngon_prism_volume(segments, 5.0, 20.0);
    assert_close(result.volume(), expected, 1e-9);
    assert_eq!(result.genus(), 1, "a through-hole makes a genus-1 solid");

    // Same answer through the generic entry point.
    let via_op = cube.boolean(&cylinder, BoolOp::Difference);
    assert_close(via_op.volume(), expected, 1e-9);

    // And the output mesh is closed with consistent winding.
    let mesh = result.to_meshgl();
    assert_eq!(mesh.num_prop(), 3);
    assert_eq!(mesh.num_tri(), result.num_tri());
    assert_closed_and_consistent(&mesh.tri_verts());
}

#[test]
fn union_and_intersection_of_overlapping_cubes() {
    let (p, t) = cube_mesh(10.0);
    let a = Manifold::from_mesh(&p, &t).unwrap();
    let b = a.translate([5.0, 5.0, 5.0]);

    let union = a.union(&b);
    assert_eq!(union.status(), Status::NoError);
    assert_close(union.volume(), 2000.0 - 125.0, 1e-12);

    let inter = a.intersection(&b);
    assert_close(inter.volume(), 125.0, 1e-12);
    assert_eq!(inter.bounding_box(), Some(([5.0; 3], [10.0; 3])));

    let disjoint = a.intersection(&a.translate([20.0, 0.0, 0.0]));
    assert_eq!(disjoint.status(), Status::NoError);
    assert!(disjoint.is_empty());
    assert_eq!(disjoint.bounding_box(), None);
}

#[test]
fn mesh_with_a_missing_triangle_is_rejected() {
    let (p, mut t) = cube_mesh(10.0);
    t.truncate(t.len() - 3); // drop one triangle: the surface now has a hole
    let mesh = MeshGL::new(&p, &t).unwrap();

    let m = Manifold::from_meshgl(&mesh);
    assert_eq!(m.status(), Status::NotManifold);
    assert!(m.is_empty());
    assert_eq!(m.num_tri(), 0);

    assert_eq!(
        Manifold::try_from_meshgl(&mesh).unwrap_err(),
        Error::Manifold(Status::NotManifold)
    );
}

#[test]
fn flipped_triangle_is_rejected() {
    let (p, mut t) = cube_mesh(10.0);
    t.swap(0, 1); // reverse the winding of one triangle
    assert_eq!(
        Manifold::from_mesh(&p, &t).unwrap_err(),
        Error::Manifold(Status::NotManifold)
    );
}

#[test]
fn bad_indices_and_coordinates_are_rejected() {
    let (p, mut t) = cube_mesh(10.0);
    t[4] = 99;
    assert_eq!(
        Manifold::from_mesh(&p, &t).unwrap_err(),
        Error::Manifold(Status::VertexIndexOutOfBounds)
    );

    let (mut p, t) = cube_mesh(10.0);
    p[7] = f32::NAN;
    assert_eq!(
        Manifold::from_mesh(&p, &t).unwrap_err(),
        Error::Manifold(Status::NonFiniteVertex)
    );
}

#[test]
fn malformed_buffers_are_caught_before_ffi() {
    assert!(matches!(MeshGL::new(&[0.0; 7], &[0, 1, 2]), Err(Error::InvalidInput(_))));
    assert!(matches!(MeshGL::new(&[0.0; 9], &[0, 1]), Err(Error::InvalidInput(_))));
    assert!(matches!(MeshGL::with_properties(&[0.0; 4], 2, &[]), Err(Error::InvalidInput(_))));
}

#[test]
fn triangle_soup_needs_merge() {
    // STL-style soup: every triangle has its own three vertices.
    let (p, t) = cube_mesh(10.0);
    let soup: Vec<f32> = t
        .iter()
        .flat_map(|&i| p[3 * i as usize..3 * i as usize + 3].to_vec())
        .collect();
    let soup_tris: Vec<u32> = (0..t.len() as u32).collect();
    let mesh = MeshGL::new(&soup, &soup_tris).unwrap();
    assert_eq!(mesh.num_vert(), 36);

    assert_eq!(Manifold::from_meshgl(&mesh).status(), Status::NotManifold);
    let merged = Manifold::try_from_meshgl(&mesh.merge()).unwrap();
    assert_close(merged.volume(), 1000.0, 1e-12);
}

#[test]
fn meshgl_round_trip_preserves_the_solid() {
    let (p, t) = cube_mesh(20.0);
    let part = Manifold::from_mesh(&p, &t)
        .unwrap()
        .difference(&Manifold::cylinder(30.0, 4.0, 4.0, 48, false).translate([10.0, 10.0, -5.0]));
    let mesh = part.to_meshgl();
    let positions = mesh.positions();
    let tris = mesh.tri_verts();
    assert_eq!(positions.len(), 3 * mesh.num_vert());
    assert_eq!(tris.len(), 3 * mesh.num_tri());

    let again = Manifold::from_mesh(&positions, &tris).unwrap();
    assert_eq!(again.num_tri(), part.num_tri());
    assert_close(again.volume(), part.volume(), 1e-6);
}

#[test]
fn cancelled_context_stops_evaluation() {
    let (p, t) = cube_mesh(20.0);
    let mesh = MeshGL::new(&p, &t).unwrap();
    let ctx = ExecutionContext::new();
    assert!(!ctx.is_cancelled());
    ctx.cancel();
    assert!(ctx.is_cancelled());

    // A cancel before the import wins.
    assert_eq!(Manifold::from_meshgl_with_context(&mesh, &ctx).status(), Status::Cancelled);

    // A pending boolean evaluated under a cancelled context.
    let cube = Manifold::from_meshgl(&mesh).translate([-10.0, -10.0, -10.0]);
    let cylinder = Manifold::cylinder(40.0, 5.0, 5.0, 64, true);
    let pending = cube.difference(&cylinder);
    assert_eq!(pending.with_context(&ctx).status(), Status::Cancelled);

    // Upstream: "Cancellation is permanent for a Manifold". The cancelled
    // node is shared, so the same tree stays Cancelled even with a fresh
    // context...
    let fresh = ExecutionContext::new();
    assert_eq!(pending.with_context(&fresh).status(), Status::Cancelled);
    assert_eq!(pending.status(), Status::Cancelled);

    // ...but the inputs are untouched: rebuilding the tree from them works.
    let fresh = ExecutionContext::new();
    let rebuilt = cube.difference(&cylinder).with_context(&fresh);
    assert_eq!(rebuilt.status(), Status::NoError);
    assert!((fresh.progress() - 1.0).abs() < 1e-12, "progress {}", fresh.progress());
    assert_close(rebuilt.volume(), 8000.0 - ngon_prism_volume(64, 5.0, 20.0), 1e-9);
}

#[test]
fn work_moves_across_threads() {
    let ctx = Arc::new(ExecutionContext::new());
    let worker_ctx = Arc::clone(&ctx);
    let handle = std::thread::spawn(move || {
        let (p, t) = cube_mesh(10.0);
        let cube = Manifold::from_mesh(&p, &t).unwrap();
        let hole = Manifold::cylinder(20.0, 2.0, 2.0, 32, false).translate([5.0, 5.0, -5.0]);
        let part = cube.difference(&hole).with_context(&worker_ctx);
        assert_eq!(part.status(), Status::NoError);
        part // Manifold is Send
    });
    let part = handle.join().unwrap();
    assert_close(part.volume(), 1000.0 - ngon_prism_volume(32, 2.0, 10.0), 1e-9);
    assert!(!ctx.is_cancelled());
}
