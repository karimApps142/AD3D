//! Export round trips and golden checksums (io/threemf, io/stl, project).
//!
//! - Golden: byte checksums of STL, 3MF model XML, whole 3MF package and a
//!   project file for a fixed hand-built mesh, so any change to the written
//!   bytes is deliberate. (Generator output is checked for determinism, not
//!   pinned, so mesher changes do not break these tests.)
//! - Round trip: a Squama plate (and its tiles) is written as 3MF and STL, read
//!   back and validated; positions survive within f32 rounding.

use std::io::Cursor;

use ad3d_core::CancelToken;
use ad3d_core::io::hash::fnv64_hex;
use ad3d_core::io::stl::{read_stl_from, write_stl_to};
use ad3d_core::io::threemf::{Meta, Part3mf, copy_model_xml, read_3mf_from, write_3mf_to};
use ad3d_core::mesh::validate::{ValidateOpts, validate};
use ad3d_core::mesh::{FaceKind, Mesh};
use ad3d_core::orient::{Orientation, QuarterTurns, Xform, export_xform};
use ad3d_core::profiles::default_printer;
use ad3d_core::project::{ExportSettings, Project};
use ad3d_core::squama::{GenOptions, ImageSource, SquamaParams, run};
use proptest::prelude::*;
use serde_json::json;

fn cube(s: f32, o: [f32; 3]) -> Mesh {
    let mut m = Mesh::default();
    for k in 0..8u32 {
        m.push_vertex([
            o[0] + (k & 1) as f32 * s,
            o[1] + ((k >> 1) & 1) as f32 * s,
            o[2] + ((k >> 2) & 1) as f32 * s,
        ]);
    }
    for t in [
        [0, 2, 1], [1, 2, 3], [4, 5, 6], [5, 7, 6], [0, 1, 4], [1, 5, 4],
        [2, 6, 3], [3, 6, 7], [0, 4, 2], [2, 4, 6], [1, 3, 5], [3, 7, 5],
    ] {
        m.push_triangle(t, FaceKind::Other);
    }
    m
}

const STANDING: Xform = Xform { m: [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]], t: [0.0, 12.5, 0.0] };

fn golden_3mf() -> Vec<u8> {
    let a = cube(10.0, [0.0; 3]);
    let b = cube(12.5, [1.0, 2.0, 0.0]);
    let parts = [
        Part3mf { name: "plate", mesh: &a, transform: Xform::IDENTITY },
        Part3mf { name: "tile r1c2", mesh: &b, transform: STANDING },
    ];
    let meta = Meta {
        title: Some("golden".into()),
        designer: Some("AD3D".into()),
        description: None,
        creation_date: Some("2026-10-20".into()),
        custom: vec![("params".into(), "{\"widthMm\":60}".into())],
    };
    let bed = default_printer().bed();
    let mut out = Cursor::new(Vec::new());
    write_3mf_to(&mut out, &parts, &meta, Some(&bed), Some(b"\x89PNG\r\n\x1a\nfake"), &CancelToken::new()).unwrap();
    out.into_inner()
}

/// Update these only when the written bytes change on purpose (M1 round 1: the default
/// printer margin went from 5 to 8 mm for brims, which moves the packed build items).
const GOLDEN_STL: &str = "aa37f5ab05446866";
const GOLDEN_3MF_MODEL: &str = "43a606961b94735d";
const GOLDEN_3MF_PACKAGE: &str = "a4c7201c5dced24b";

#[test]
fn golden_checksums() {
    let mut stl = Vec::new();
    write_stl_to(&mut stl, &cube(10.0, [0.0; 3]), &STANDING).unwrap();
    let pkg = golden_3mf();
    let mut model = Vec::new();
    copy_model_xml(Cursor::new(&pkg), &mut model).unwrap();
    let got = [fnv64_hex(&stl), fnv64_hex(&model), fnv64_hex(&pkg)];
    println!("golden: stl {} model {} package {}", got[0], got[1], got[2]);
    assert_eq!(got, [GOLDEN_STL, GOLDEN_3MF_MODEL, GOLDEN_3MF_PACKAGE], "written bytes changed");
    // The model XML is plain, millimetre, core-spec 3MF.
    let xml = String::from_utf8(model).unwrap();
    assert!(xml.contains("unit=\"millimeter\""));
    assert!(xml.contains("http://schemas.microsoft.com/3dmanufacturing/core/2015/02"));
    assert_eq!(xml.matches("<object ").count(), 2);
    assert_eq!(xml.matches("<item ").count(), 2);
}

#[test]
fn project_bytes_are_deterministic() {
    let params = json!({ "widthMm": 60.0, "cellMm": 6.0, "lightAzimuthDeg": 180.0, "lightElevationDeg": 5.0,
                         "maxTiltDeg": 10.0, "baseMm": 3.0, "image": "synthetic" });
    let mk = || {
        let mut p = Project::new_squama(params.clone(), None, Some(("photo.png", vec![1, 2, 3, 4])), ExportSettings::default(), default_printer()).unwrap();
        p.thumbnail_png = Some(b"\x89PNGthumb".to_vec());
        p.to_bytes().unwrap()
    };
    let (a, b) = (mk(), mk());
    assert_eq!(a, b);
    let back = Project::from_bytes(&a).unwrap();
    assert_eq!(back.image_bytes.as_deref(), Some(&[1u8, 2, 3, 4][..]));
    assert_eq!(back.to_bytes().unwrap(), a, "load → save is byte-identical");
}

fn plate(width: f64, az_deg: f64) -> ad3d_core::squama::SquamaOutput {
    let p = SquamaParams { width_mm: width, light_azimuth: az_deg.to_radians(), ..Default::default() };
    run(&p, ImageSource::Synthetic, &GenOptions::default(), &CancelToken::new()).unwrap()
}

fn assert_same_positions(written: &Mesh, x: &Xform, back: &Mesh) {
    let w = written.map_positions(|q| x.apply_f32(q)).bounds().unwrap();
    let r = back.bounds().unwrap();
    for k in 0..3 {
        assert!((w.min[k] - r.min[k]).abs() < 1e-3 && (w.max[k] - r.max[k]).abs() < 1e-3, "{w:?} vs {r:?}");
    }
}

#[test]
fn squama_plate_round_trips_through_3mf_and_stl() {
    let o = plate(90.0, 180.0);
    let l = o.layout;
    let (x, _) = export_xform(Orientation::Standing(QuarterTurns::Auto), l.w_mm, l.h_mm, std::f64::consts::PI);
    // 3MF.
    let mut buf = Cursor::new(Vec::new());
    let bed = default_printer().bed();
    let rep = write_3mf_to(&mut buf, &[Part3mf { name: "plate", mesh: &o.mesh, transform: x }], &Meta::default(), Some(&bed), None, &CancelToken::new()).unwrap();
    assert_eq!(rep.objects[0].triangles, o.mesh.triangle_count());
    let bytes = buf.into_inner();
    let m = read_3mf_from(Cursor::new(&bytes)).unwrap();
    assert_eq!(m.unit, "millimeter");
    let ob = &m.objects[0];
    assert_eq!(ob.mesh.triangle_count(), o.mesh.triangle_count());
    let v = validate(&ob.mesh, &ValidateOpts { soup: true, ..Default::default() });
    assert!(v.ok, "3mf re-read: {}", v.summary());
    assert_same_positions(&o.mesh, &x, &ob.mesh);
    // The placed item sits on the bed (z = 0) inside the margin.
    let it = &m.items[0];
    let placed = ob.mesh.map_positions(|q| it.transform.apply_f32(q)).bounds().unwrap();
    assert!(placed.min[2].abs() < 1e-4 && placed.min[0] >= bed.margin_mm as f32 - 1e-3, "{placed:?}");
    // STL.
    let mut stl = Vec::new();
    write_stl_to(&mut stl, &o.mesh, &x).unwrap();
    let back = read_stl_from(Cursor::new(stl)).unwrap();
    let v = validate(&back, &ValidateOpts { soup: true, ..Default::default() });
    assert!(v.ok, "stl re-read: {}", v.summary());
    assert_same_positions(&o.mesh, &x, &back);
    // Same input, same bytes.
    let mut again = Cursor::new(Vec::new());
    write_3mf_to(&mut again, &[Part3mf { name: "plate", mesh: &plate(90.0, 180.0).mesh, transform: x }], &Meta::default(), Some(&bed), None, &CancelToken::new()).unwrap();
    assert_eq!(fnv64_hex(&again.into_inner()), fnv64_hex(&bytes));
}

#[test]
fn cancelled_3mf_write_errors() {
    let c = CancelToken::new();
    c.cancel();
    let a = cube(1.0, [0.0; 3]);
    let r = write_3mf_to(Cursor::new(Vec::new()), &[Part3mf { name: "a", mesh: &a, transform: Xform::IDENTITY }], &Meta::default(), None, None, &c);
    assert!(r.is_err());
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 12, .. ProptestConfig::default() })]

    /// Random plates, random orientation, several parts: every object re-read
    /// from the 3MF is a closed, consistently wound shell and items never overlap.
    #[test]
    fn random_plates_round_trip(width in 30.0f64..90.0, az in 0.0f64..360.0, flat in any::<bool>(), n in 1usize..4) {
        let o = plate(width, az);
        let l = o.layout;
        let orient = if flat { Orientation::Flat } else { Orientation::Standing(QuarterTurns::Auto) };
        let (x, _) = export_xform(orient, l.w_mm, l.h_mm, az.to_radians());
        let names: Vec<String> = (0..n).map(|i| format!("part {i}")).collect();
        let parts: Vec<Part3mf> = names.iter().map(|nm| Part3mf { name: nm, mesh: &o.mesh, transform: x }).collect();
        let bed = default_printer().bed();
        let mut buf = Cursor::new(Vec::new());
        write_3mf_to(&mut buf, &parts, &Meta::default(), Some(&bed), None, &CancelToken::new()).unwrap();
        let m = read_3mf_from(Cursor::new(buf.into_inner())).unwrap();
        prop_assert_eq!(m.objects.len(), n);
        let mut rects: Vec<(u32, [f32; 4])> = Vec::new();
        for (ob, it) in m.objects.iter().zip(&m.items) {
            let v = validate(&ob.mesh, &ValidateOpts { soup: true, ..Default::default() });
            prop_assert!(v.ok, "{}", v.summary());
            prop_assert!(ob.mesh.signed_volume() > 0.0);
            let b = ob.mesh.map_positions(|q| it.transform.apply_f32(q)).bounds().unwrap();
            rects.push((0, [b.min[0], b.min[1], b.max[0], b.max[1]]));
        }
        // Items on the same bed position never overlap (spill beds sit beside the first).
        for i in 0..rects.len() {
            for j in i + 1..rects.len() {
                let (a, b) = (rects[i].1, rects[j].1);
                let overlap = a[0] < b[2] - 1e-3 && b[0] < a[2] - 1e-3 && a[1] < b[3] - 1e-3 && b[1] < a[3] - 1e-3;
                prop_assert!(!overlap, "{:?} overlaps {:?}", a, b);
            }
        }
    }
}
