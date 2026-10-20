//! Mutation tests: validate() must catch deliberately broken meshes.

use ad3d_core::CancelToken;
use ad3d_core::image::CellBrightness;
use ad3d_core::io::stl::{read_stl, write_stl};
use ad3d_core::mesh::validate::{ValidateOpts, validate};
use ad3d_core::mesh::{FaceKind, Mesh};
use ad3d_core::orient::Xform;
use ad3d_core::squama::{GenOptions, SquamaOutput, SquamaParams, generate_ex, validate_opts};

fn plate() -> SquamaOutput {
    let p = SquamaParams { width_mm: 24.0, cell_mm: 4.0, ..Default::default() };
    let g = (0..36).map(|k| ((k * 7) % 5) as f32 / 4.0).collect();
    let out = generate_ex(&p, &CellBrightness { nx: 6, ny: 6, g }, &GenOptions::default(), &CancelToken::new()).unwrap();
    let r = validate(&out.mesh, &validate_opts(&out, Xform::IDENTITY));
    assert!(r.ok, "{}", r.summary());
    assert!(out.stats.mesh.saddles > 0 && out.stats.mesh.crossings > 0, "test plate should exercise saddles and crossings");
    out
}

fn opts(out: &SquamaOutput) -> ValidateOpts {
    validate_opts(out, Xform::IDENTITY)
}

/// Index of a wall triangle, so facet/back checks don't mask topology checks.
fn a_wall(m: &Mesh) -> usize {
    m.face_kind.iter().position(|&k| k == FaceKind::Wall).unwrap()
}

#[test]
fn removed_triangle_gives_boundary_edges() {
    let out = plate();
    let mut m = out.mesh.clone();
    let t = a_wall(&m);
    m.indices.drain(3 * t..3 * t + 3);
    m.face_kind.remove(t);
    let r = validate(&m, &opts(&out));
    assert!(!r.ok);
    let raw = r.raw.as_ref().unwrap();
    assert_eq!(raw.boundary_edges, 3, "{}", r.summary());
    assert_eq!(r.written.boundary_edges, 3);
}

#[test]
fn flipped_triangle_gives_inconsistent_winding() {
    let out = plate();
    let mut m = out.mesh.clone();
    let t = a_wall(&m);
    m.indices.swap(3 * t + 1, 3 * t + 2);
    let r = validate(&m, &opts(&out));
    assert!(!r.ok);
    assert_eq!(r.raw.as_ref().unwrap().inconsistent_winding_edges, 3, "{}", r.summary());
}

#[test]
fn two_fans_at_one_vertex_is_pinched() {
    // Two closed tetrahedra sharing exactly one vertex.
    let mut m = Mesh::default();
    let tet = |m: &mut Mesh, o: [f32; 3], shared: Option<u32>| {
        let apex = match shared {
            Some(s) => s,
            None => m.push_vertex(o),
        };
        let a = m.push_vertex([o[0] + 1.0, o[1], o[2] - 1.0]);
        let b = m.push_vertex([o[0], o[1] + 1.0, o[2] - 1.0]);
        let c = m.push_vertex([o[0] - 1.0, o[1] - 1.0, o[2] - 1.0]);
        for t in [[a, c, b], [apex, a, b], [apex, b, c], [apex, c, a]] {
            m.push_triangle(t, FaceKind::Other);
        }
        apex
    };
    let s = tet(&mut m, [0.0, 0.0, 0.0], None);
    // Second tet: the first reflected through the shared apex (det −1, so reverse winding).
    let first: Vec<[u32; 3]> = (0..m.triangle_count()).map(|t| m.triangle(t)).collect();
    let mut map = std::collections::HashMap::new();
    map.insert(s, s);
    for t in &first {
        for &v in t {
            if !map.contains_key(&v) {
                let p = m.vertex(v);
                let id = m.push_vertex([-p[0], -p[1], -p[2]]);
                map.insert(v, id);
            }
        }
    }
    for t in &first {
        m.push_triangle([map[&t[0]], map[&t[2]], map[&t[1]]], FaceKind::Other);
    }
    let r = validate(&m, &ValidateOpts { min_distance: None, ..Default::default() });
    let raw = r.raw.as_ref().unwrap();
    assert_eq!(raw.boundary_edges + raw.nonmanifold_edges + raw.inconsistent_winding_edges, 0, "{}", r.summary());
    assert_eq!(raw.pinched_vertices, 1, "{}", r.summary());
    assert!(!r.ok);
}

#[test]
fn pinch_inside_a_generated_plate() {
    // Weld a back-face corner of the plate onto a duplicate of itself from a second
    // copy of the plate translated so that the two corners coincide: one vertex, two fans.
    let out = plate();
    let m = &out.mesh;
    let b = m.bounds().unwrap();
    let corner = (0..m.vertex_count() as u32).find(|&v| m.vertex(v) == [b.max[0], b.max[1], 0.0]).unwrap();
    let mut two = m.clone();
    let off = m.vertex_count() as u32;
    for v in 0..m.vertex_count() as u32 {
        let p = m.vertex(v);
        two.push_vertex([p[0] + b.max[0], p[1] + b.max[1], p[2]]);
    }
    let origin = (0..m.vertex_count() as u32).find(|&v| m.vertex(v) == [0.0, 0.0, 0.0]).unwrap();
    for t in 0..m.triangle_count() {
        let tri = m.triangle(t).map(|i| if i == origin { corner } else { i + off });
        two.push_triangle(tri, m.face_kind[t]);
    }
    let r = validate(&two, &ValidateOpts { expect_components: 1, ..Default::default() });
    assert!(!r.ok);
    assert!(r.raw.as_ref().unwrap().pinched_vertices >= 1, "{}", r.summary());
}

#[test]
fn duplicate_vertex_at_same_position() {
    let out = plate();
    let mut m = out.mesh.clone();
    let t = a_wall(&m);
    let v = m.indices[3 * t];
    let dup = m.push_vertex(m.vertex(v));
    m.indices[3 * t] = dup;
    let r = validate(&m, &opts(&out));
    assert!(!r.ok);
    // Raw stage sees the split; the written stage's exact weld must report a merge.
    let raw = r.raw.as_ref().unwrap();
    assert!(raw.boundary_edges > 0 || raw.pinched_vertices > 0, "{}", r.summary());
    assert_eq!(r.written.merged, 1);
    assert!(r.failures.iter().any(|f| f.contains("merged 1")), "{:?}", r.failures);
}

#[test]
fn zero_area_triangle() {
    let out = plate();
    let mut m = out.mesh.clone();
    let t = a_wall(&m);
    let [a, b, _] = m.triangle(t);
    let (pa, pb) = (m.vertex(a), m.vertex(b));
    let mid = m.push_vertex([(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0, (pa[2] + pb[2]) / 2.0]);
    m.push_triangle([a, mid, b], FaceKind::Wall);
    let r = validate(&m, &opts(&out));
    assert!(!r.ok);
    assert!(r.raw.as_ref().unwrap().zero_area_tris >= 1, "{}", r.summary());
}

#[test]
fn repeated_index_triangle() {
    let out = plate();
    let mut m = out.mesh.clone();
    let t = a_wall(&m);
    m.indices[3 * t + 2] = m.indices[3 * t];
    let r = validate(&m, &opts(&out));
    assert!(!r.ok);
    assert_eq!(r.raw.as_ref().unwrap().repeated_index_tris, 1);
}

#[test]
fn near_vertices_fail_gate_and_slicer_stage() {
    let out = plate();
    let mut m = out.mesh.clone();
    // Move a top facet apex 20 µm from a corner of its own facet.
    let t = m.face_kind.iter().position(|&k| k == FaceKind::Facet).unwrap();
    let [apex, c, _] = m.triangle(t);
    let pc = m.vertex(c);
    let i = apex as usize * 3;
    // Pick a 10–29 µm x offset that keeps both points in one CuraEngine 30 µm bucket
    // (Cura only melds within a bucket).
    let bucket = |x: f32| ((x as f64 * 1000.0 + 0.5) as i64 + 15) / 30;
    let d = (10..30).map(|um| um as f32 / 1000.0).find(|&d| bucket(pc[0] + d) == bucket(pc[0])).unwrap();
    m.positions[i] = pc[0] + d;
    m.positions[i + 1] = pc[1];
    m.positions[i + 2] = pc[2];
    let r = validate(&m, &opts(&out));
    assert!(!r.ok);
    assert!(r.min_distinct_distance.unwrap() < 0.03);
    assert!(r.slicer.as_ref().unwrap().merged >= 1, "{}", r.summary());
}

#[test]
fn facet_below_base_is_caught() {
    let out = plate();
    let mut m = out.mesh.clone();
    let t = m.face_kind.iter().position(|&k| k == FaceKind::Facet).unwrap();
    let apex = m.triangle(t)[0] as usize;
    m.positions[apex * 3 + 2] = 1.0; // below T = 3
    let r = validate(&m, &opts(&out));
    assert!(!r.ok);
    assert_eq!(r.facet_vertices_below_min, 1);
}

#[test]
fn stl_roundtrip_validates_as_soup() {
    let out = plate();
    let dir = std::env::temp_dir().join(format!("ad3d-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("plate.stl");
    write_stl(&path, &out.mesh, &Xform::IDENTITY).unwrap();
    let soup = read_stl(&path).unwrap();
    assert_eq!(soup.triangle_count(), out.mesh.triangle_count());
    let r = validate(&soup, &ValidateOpts { soup: true, ..Default::default() });
    assert!(r.ok, "{}", r.summary());
    assert_eq!(r.written.vertices_welded, out.mesh.vertex_count());
    std::fs::remove_dir_all(&dir).ok();
}
