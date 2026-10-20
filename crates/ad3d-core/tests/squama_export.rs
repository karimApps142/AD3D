//! Tiling, keys, stand and export (design §9, §14): every exported part passes
//! `validate()` (all three stages and the self-intersection stage) on the data
//! actually written, fits its bed, and parts never overlap.

use std::f64::consts::PI;

use ad3d_core::CancelToken;
use ad3d_core::error::Error;
use ad3d_core::image::{CellBrightness, synthetic_target};
use ad3d_core::io::stl::read_stl;
use ad3d_core::mesh::FaceKind;
use ad3d_core::mesh::validate::{ValidateOpts, validate};
use ad3d_core::orient::{BedMotion, Orientation, QuarterTurns};
use ad3d_core::relief::{BackRegion, PieceSet, Tol, mesh_relief, quantise};
use ad3d_core::squama::export::{
    CollectSink, ExportReport, KeyKind, SquamaExport, SquamaExportDto, StlDirSink, squama_export, squama_export_cb,
};
use ad3d_core::squama::parts::{PartKind, circle};
use ad3d_core::squama::tiles::Bed;
use ad3d_core::squama::{self, GenOptions, SquamaParams};
use glam::DVec2;
use proptest::prelude::*;

fn noise(nx: u32, ny: u32, seed: u64) -> CellBrightness {
    let mut s = seed | 1;
    let g = (0..nx * ny)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 40) as f32 / (1u64 << 24) as f32
        })
        .collect();
    CellBrightness { nx, ny, g }
}

fn cb_for(p: &SquamaParams, aspect: (u32, u32), seed: u64) -> CellBrightness {
    let l = squama::layout(p, aspect).unwrap();
    noise(l.nx, l.ny, seed)
}

/// Bed fit, no overlap, Z ≥ 0, standing faces along the motion axis.
fn check_layout(name: &str, sink: &CollectSink, rep: &ExportReport, bed: &Bed) {
    let u = bed.usable();
    assert_eq!(sink.parts.len(), rep.parts.len());
    for (i, (part, pr)) in sink.parts.iter().enumerate() {
        let x0 = bed.margin_mm;
        let (lo, hi) = (pr.min_mm, pr.max_mm);
        let e = 1e-3;
        assert!(lo[0] >= x0 - e && hi[0] <= x0 + u[0] + e, "{name}: {} X {lo:?}..{hi:?}", part.name);
        assert!(lo[1] >= bed.margin_mm - e && hi[1] <= bed.margin_mm + u[1] + e, "{name}: {} Y {lo:?}..{hi:?}", part.name);
        assert!(lo[2].abs() < e && hi[2] <= u[2] + e, "{name}: {} Z {lo:?}..{hi:?}", part.name);
        assert!((part.transform.det() - 1.0).abs() < 1e-12);
        for (other, pr2) in &sink.parts[i + 1..] {
            let overlap = pr.bed == pr2.bed && (0..2).all(|k| lo[k] < pr2.max_mm[k] - e && pr2.min_mm[k] < hi[k] - e);
            assert!(!overlap, "{name}: {} overlaps {}", part.name, other.name);
        }
        if matches!(part.kind, PartKind::Tile { .. }) && rep.quarter_turns.is_some() {
            // Plate normal (w) direction in print space.
            let n = [part.transform.m[0][2], part.transform.m[1][2], part.transform.m[2][2]];
            match bed.motion {
                BedMotion::Y => assert_eq!(n[0].abs(), 1.0, "{name}: face not parallel to Y"),
                _ => assert_eq!(n[1].abs(), 1.0, "{name}: face not parallel to X"),
            }
            assert_eq!(n[2], 0.0);
        }
    }
}

#[test]
fn luban_default_plate_on_cr6se() {
    let p = SquamaParams::default(); // 300 × 378, light from the left
    let img = synthetic_target(squama::SYNTHETIC_PX.0, squama::SYNTHETIC_PX.1);
    let e = SquamaExport::default();
    let mut sink = CollectSink::default();
    let rep = squama_export(&p, &img, &e, &CancelToken::new(), &mut sink).unwrap();
    assert!(sink.finished);
    assert_eq!((rep.cols, rep.rows), (2, 2));
    assert_eq!(rep.quarter_turns, Some(1), "left light → rotate so it comes from below");
    assert!(rep.keys > 0);
    let tiles = sink.parts.iter().filter(|(p, _)| matches!(p.kind, PartKind::Tile { .. })).count();
    let stands = sink.parts.iter().filter(|(p, _)| matches!(p.kind, PartKind::Stand { .. })).count();
    assert_eq!(tiles, 4);
    assert_eq!(stands, 2);
    assert!(sink.parts.iter().any(|(p, _)| matches!(p.kind, PartKind::KeySplints { .. })));
    for (_, pr) in &sink.parts {
        assert_eq!(pr.self_intersections, Some(0));
        // None: no two distinct vertices within 2δ (the search radius).
        assert!(pr.min_distinct_distance_mm.is_none_or(|d| d >= 0.05), "{:?}", pr.min_distinct_distance_mm);
    }
    check_layout("luban", &sink, &rep, &e.bed);
    // Every tile has pockets (keys) and engraving, and its piece set tiles the plate.
    let total: f64 = sink
        .parts
        .iter()
        .filter(|(p, _)| matches!(p.kind, PartKind::Tile { .. }))
        .map(|(p, _)| {
            assert!(p.mesh.face_kind.contains(&FaceKind::Key));
            p.back_area.unwrap()
        })
        .sum();
    assert!((total - 300.0 * 378.0).abs() < 1e-6);
    // Light from below in print space: ledges stay small (§9 measured ~0.5 mm).
    assert!(rep.max_downward_ledge_mm <= 0.6 + 1e-9, "ledge {}", rep.max_downward_ledge_mm);
    // The tile volumes add up to the whole plate minus the pockets.
    let whole = squama::run(&p, squama::ImageSource::Synthetic, &GenOptions::default(), &CancelToken::new()).unwrap();
    let vt: f64 = sink.parts.iter().filter(|(p, _)| matches!(p.kind, PartKind::Tile { .. })).map(|(p, _)| p.mesh.signed_volume()).sum();
    let vw = whole.mesh.signed_volume();
    assert!(vt < vw && vt > 0.995 * vw, "tiles {vt} vs plate {vw}");
}

#[test]
fn auto_orientation_reduces_ledges() {
    // Top light: unrotated (k = 0) every step faces down; Auto turns the plate over.
    let p = SquamaParams { width_mm: 120.0, light_azimuth: 0.5 * PI, base_mm: 3.0, ..Default::default() };
    let cb = cb_for(&p, (1, 1), 7);
    let run = |o| {
        let e = SquamaExport { orientation: o, stand: false, ..Default::default() };
        squama_export_cb(&p, &cb, &e, &CancelToken::new(), &mut CollectSink::default()).unwrap()
    };
    let auto = run(Orientation::Standing(QuarterTurns::Auto));
    let q0 = run(Orientation::Standing(QuarterTurns::Q0));
    assert_eq!(auto.quarter_turns, Some(2));
    assert!(auto.max_downward_ledge_mm < q0.max_downward_ledge_mm, "{} vs {}", auto.max_downward_ledge_mm, q0.max_downward_ledge_mm);
    assert!(q0.warnings.iter().any(|w| w.code == "downward_ledge"));
}

#[test]
fn stl_sink_writes_valid_files_and_cancel_leaves_none() {
    let dir = std::env::temp_dir().join(format!("ad3d-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let p = SquamaParams { width_mm: 260.0, cell_mm: 6.5, base_mm: 3.0, ..Default::default() };
    let cb = cb_for(&p, (1, 1), 3);
    let e = SquamaExport::default();
    let mut sink = StlDirSink::new(&dir, "t_").unwrap();
    let rep = squama_export_cb(&p, &cb, &e, &CancelToken::new(), &mut sink).unwrap();
    assert_eq!(sink.files.len(), rep.parts.len());
    for (f, pr) in sink.files.iter().zip(&rep.parts) {
        let soup = read_stl(f).unwrap();
        let comps = match pr.kind {
            PartKind::KeySplints { count } => count as usize,
            _ => 1,
        };
        let r = validate(&soup, &ValidateOpts { soup: true, expect_components: comps, ..Default::default() });
        assert!(r.ok, "{}: {}", f.display(), r.summary());
    }
    // A cancelled export leaves no file behind.
    let dir2 = dir.join("cancelled");
    let mut sink2 = StlDirSink::new(&dir2, "").unwrap();
    let c = CancelToken::new();
    c.cancel();
    assert_eq!(squama_export_cb(&p, &cb, &e, &c, &mut sink2), Err(Error::Cancelled));
    assert_eq!(std::fs::read_dir(&dir2).unwrap().count(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn keys_need_a_thick_base() {
    let p = SquamaParams { base_mm: 2.0, ..Default::default() };
    let cb = cb_for(&p, (4, 5), 1);
    let e = SquamaExport::default();
    let r = squama_export_cb(&p, &cb, &e, &CancelToken::new(), &mut CollectSink::default());
    assert!(matches!(r, Err(Error::InvalidParams(_))), "{r:?}");
    let e2 = SquamaExport { keys: KeyKind::None, ..Default::default() };
    let rep = squama_export_cb(&p, &cb, &e2, &CancelToken::new(), &mut CollectSink::default()).unwrap();
    assert_eq!(rep.keys, 0);
    // 2 mm base → ~3 mm plate: the stability cap (40 × thickness) keeps tiles ≤ ~122 mm tall.
    assert!(rep.cols * rep.rows >= 4, "{} × {}", rep.cols, rep.rows);
}

#[test]
fn dto_defaults_and_parsing() {
    let json = r#"{"params":{"jobId":1,"widthMm":100,"cellMm":5,"lightAzimuthDeg":180,"lightElevationDeg":5,
        "maxTiltDeg":10,"baseMm":3,"image":"synthetic"},"outDir":"/tmp/x"}"#;
    let dto: SquamaExportDto = serde_json::from_str(json).unwrap();
    let (p, e) = dto.export().unwrap();
    assert_eq!(e.orientation, Orientation::Standing(QuarterTurns::Auto));
    assert_eq!(e.bed, Bed::CR6_SE);
    assert!((p.light_azimuth - PI).abs() < 1e-12);
    let json2 = r#"{"params":{"jobId":1,"widthMm":100,"cellMm":5,"lightAzimuthDeg":180,"lightElevationDeg":5,
        "maxTiltDeg":10,"baseMm":3,"image":"synthetic"},"outDir":"/tmp/x","orientation":"flat",
        "bedMm":[300,300,400],"marginMm":2,"bedMotion":"none","keys":"none","stand":false}"#;
    let (_, e2) = serde_json::from_str::<SquamaExportDto>(json2).unwrap().export().unwrap();
    assert_eq!(e2.orientation, Orientation::Flat);
    assert_eq!(e2.bed.motion, BedMotion::None);
    assert_eq!(e2.keys, KeyKind::None);
    assert!(!e2.stand);
}

// ---------------------------------------------------------------------------
// Back regions in the relief mesher (§14.3)
// ---------------------------------------------------------------------------

fn grid_heights(nx: u32, ny: u32, cell: f64, seed: u64) -> (ad3d_core::relief::Partition, ad3d_core::relief::Heights) {
    let p = SquamaParams { width_mm: nx as f64 * cell, cell_mm: cell, base_mm: 3.0, ..Default::default() };
    let cb = noise(nx, ny, seed);
    let l = squama::layout(&p, (nx, ny)).unwrap();
    let tol = Tol::default();
    let f = squama::solve_facets(&cb, &l, &p, &tol);
    let part = squama::build_partition(&l, &[]).unwrap();
    let h = quantise(&part, &f, p.base_mm, &tol).unwrap();
    (part, h)
}

#[test]
fn pockets_and_ribs_are_watertight() {
    let (part, h) = grid_heights(12, 10, 6.0, 11);
    let regions = vec![
        BackRegion { poly: circle(DVec2::new(10.0, 10.0), 1.7, 24, 0.0), w_mm: 2.0 },
        BackRegion { poly: circle(DVec2::new(20.0, 10.0), 3.0, 7, 0.3), w_mm: 0.4 },
        // Clockwise rib (orientation is normalised).
        BackRegion {
            poly: vec![DVec2::new(30.0, 40.0), DVec2::new(30.0, 55.0), DVec2::new(65.0, 55.0), DVec2::new(65.0, 40.0)],
            w_mm: -2.0,
        },
        // Non-convex pocket (an L).
        BackRegion {
            poly: vec![
                DVec2::new(40.0, 5.0),
                DVec2::new(60.0, 5.0),
                DVec2::new(60.0, 10.0),
                DVec2::new(45.0, 10.0),
                DVec2::new(45.0, 25.0),
                DVec2::new(40.0, 25.0),
            ],
            w_mm: 1.0,
        },
    ];
    let m = mesh_relief(&part, PieceSet::All, &h, &regions, &Tol::default(), &CancelToken::new()).unwrap();
    let r = validate(
        &m,
        &ValidateOpts {
            min_facet_w: Some(3.0 - 0.0005),
            back_area: Some(72.0 * 60.0),
            check_self_intersection: true,
            ..Default::default()
        },
    );
    assert!(r.ok, "{}", r.summary());
}

#[test]
fn bad_back_regions_are_refused() {
    let (part, h) = grid_heights(8, 8, 6.0, 5);
    let tol = Tol::default();
    let c = CancelToken::new();
    let cases = [
        // Too close to the outline.
        vec![BackRegion { poly: circle(DVec2::new(1.5, 20.0), 1.0, 12, 0.0), w_mm: 1.0 }],
        // Pocket floor too close to the facets (T = 3, floor 2.5 > 2.2).
        vec![BackRegion { poly: circle(DVec2::new(20.0, 20.0), 2.0, 12, 0.0), w_mm: 2.5 }],
        // Two regions closer than 1 mm.
        vec![
            BackRegion { poly: circle(DVec2::new(20.0, 20.0), 2.0, 12, 0.0), w_mm: 1.0 },
            BackRegion { poly: circle(DVec2::new(24.5, 20.0), 2.0, 12, 0.0), w_mm: 1.0 },
        ],
        // Self-intersecting bow tie.
        vec![BackRegion {
            poly: vec![DVec2::new(10.0, 10.0), DVec2::new(20.0, 20.0), DVec2::new(20.0, 10.0), DVec2::new(10.0, 20.0)],
            w_mm: 1.0,
        }],
        // Too shallow.
        vec![BackRegion { poly: circle(DVec2::new(20.0, 20.0), 2.0, 12, 0.0), w_mm: 0.1 }],
    ];
    for (i, regions) in cases.iter().enumerate() {
        let r = mesh_relief(&part, PieceSet::All, &h, regions, &tol, &c);
        assert!(matches!(r, Err(Error::InvalidParams(_))), "case {i}: {:?}", r.map(|m| m.triangle_count()));
    }
}

// ---------------------------------------------------------------------------
// Property test: random plates and beds
// ---------------------------------------------------------------------------

fn arb_bed() -> impl Strategy<Value = Bed> {
    (80.0f64..320.0, 80.0f64..320.0, 80.0f64..320.0, 0.0f64..10.0, 0u8..3).prop_map(|(x, y, z, m, mo)| Bed {
        x_mm: x,
        y_mm: y,
        z_mm: z,
        margin_mm: m,
        motion: [BedMotion::X, BedMotion::Y, BedMotion::None][mo as usize],
    })
}

fn arb_orientation() -> impl Strategy<Value = Orientation> {
    (0u8..6).prop_map(|k| match k {
        0 => Orientation::Flat,
        1 => Orientation::Standing(QuarterTurns::Q0),
        2 => Orientation::Standing(QuarterTurns::Q1),
        3 => Orientation::Standing(QuarterTurns::Q2),
        4 => Orientation::Standing(QuarterTurns::Q3),
        _ => Orientation::Standing(QuarterTurns::Auto),
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 40, ..ProptestConfig::default() })]

    #[test]
    fn random_plates_and_beds_export_valid_parts(
        width in 30.0f64..420.0,
        aspect in 0.3f64..2.0,
        cell in 3.0f64..10.0,
        az in 0.0f64..360.0,
        el in 1.0f64..45.0,
        tilt in 3.0f64..20.0,
        base in 0.6f64..5.0,
        bed in arb_bed(),
        o in arb_orientation(),
        keys in any::<bool>(),
        stand in any::<bool>(),
        seed in any::<u64>(),
    ) {
        let p = SquamaParams {
            width_mm: width,
            height_mm: None,
            cell_mm: cell,
            light_azimuth: az.to_radians(),
            light_elevation: el.to_radians(),
            max_tilt: tilt.to_radians(),
            base_mm: base,
        };
        let cb = cb_for(&p, (1000, (1000.0 * aspect) as u32), seed);
        let e = SquamaExport {
            orientation: o,
            bed,
            keys: if keys { KeyKind::Pegs } else { KeyKind::None },
            stand,
            ..Default::default()
        };
        let mut sink = CollectSink::default();
        match squama_export_cb(&p, &cb, &e, &CancelToken::new(), &mut sink) {
            Ok(rep) => {
                prop_assert!(sink.finished);
                check_layout("prop", &sink, &rep, &bed);
                let tiles = sink.parts.iter().filter(|(p, _)| matches!(p.kind, PartKind::Tile { .. })).count();
                prop_assert_eq!(tiles as u32, rep.cols * rep.rows);
                if let Orientation::Standing(QuarterTurns::Auto) = o {
                    let k = rep.quarter_turns.unwrap();
                    prop_assert!(ad3d_core::orient::light_score(p.light_azimuth, k) <= 1e-9);
                }
            }
            // Beds too small for the plate thickness or for keyed tiles, keys on a
            // thin base, stands that do not fit: refused, never an internal error.
            Err(Error::InvalidParams(_)) => prop_assert!(sink.parts.is_empty()),
            Err(err) => prop_assert!(false, "unexpected error {err:?}"),
        }
    }
}

// ---- M1 integration: jobs (hex, point light, optimiser), calibration tile, 3MF.

fn job_from(json: serde_json::Value) -> ad3d_core::squama::job::SquamaJob {
    let d: ad3d_core::squama::SquamaParamsDto = serde_json::from_value(json).unwrap();
    d.job().unwrap()
}

#[test]
fn hex_plate_tiles_on_cr6se_with_calibration() {
    for orientation in ["pointy", "flat"] {
        let job = job_from(serde_json::json!({
            "jobId": 1, "widthMm": 300, "heightMm": 260, "cellMm": 6, "layout": {"kind": "hex", "orientation": orientation},
            "lightAzimuthDeg": 180, "lightElevationDeg": 8, "maxTiltDeg": 10, "baseMm": 3, "image": "synthetic"
        }));
        let e = SquamaExport { calibration_tile: true, ..SquamaExport::default() };
        let mut sink = CollectSink::default();
        let rep = ad3d_core::squama::export::squama_export_job(&job, None, &e, &CancelToken::new(), &mut sink).unwrap();
        assert!(rep.cols * rep.rows > 1, "{orientation}: should need several tiles");
        assert!(rep.calibration_key.is_some());
        assert!(sink.parts.iter().any(|(p, _)| p.kind == PartKind::Calibration));
        check_layout(orientation, &sink, &rep, &e.bed);
        let tiles: f64 = sink.parts.iter().filter_map(|(p, _)| matches!(p.kind, PartKind::Tile { .. }).then(|| p.back_area.unwrap())).sum();
        assert!((tiles - rep.plate_mm[0] * rep.plate_mm[1]).abs() < 1e-6 * tiles, "{orientation}: tile areas {tiles}");
        for (p, _) in &sink.parts {
            assert!(validate(&p.mesh, &p.validate_opts(true)).ok, "{orientation}: {}", p.name);
        }
    }
}

#[test]
fn optimised_point_light_export_reports_scores() {
    let job = job_from(serde_json::json!({
        "jobId": 1, "widthMm": 120, "cellMm": 6, "lightAzimuthDeg": 180, "lightElevationDeg": 8,
        "light": {"kind": "point", "posMm": [-600, 75, 200]}, "maxTiltDeg": 10, "baseMm": 3, "image": "synthetic",
        "optimiser": {"kind": "shadow", "maxSweeps": 2}
    }));
    let e = SquamaExport { keys: KeyKind::None, stand: false, ..SquamaExport::default() };
    let mut sink = CollectSink::default();
    let rep = ad3d_core::squama::export::squama_export_job(&job, None, &e, &CancelToken::new(), &mut sink).unwrap();
    let o = rep.optimise.expect("optimiser ran");
    assert!(o.after.rmse <= o.before.rmse + 1e-12);
    for (p, _) in &sink.parts {
        assert!(validate(&p.mesh, &p.validate_opts(true)).ok, "{}", p.name);
    }
}

#[test]
fn three_mf_through_the_io_sink() {
    use ad3d_core::io::threemf::{Meta, read_3mf};
    use ad3d_core::squama::export::{IoSink, MultiSink};
    let dir = std::env::temp_dir().join(format!("ad3d-m1-3mf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("plate.3mf");
    let p = SquamaParams { width_mm: 120.0, ..SquamaParams::default() };
    let img = synthetic_target(squama::SYNTHETIC_PX.0, squama::SYNTHETIC_PX.1);
    let e = SquamaExport { stand: true, ..SquamaExport::default() };
    let tmf = ad3d_core::io::sink::ThreeMfSink::new(&path, &Meta::default(), None, None, CancelToken::new()).unwrap();
    let mut sink = MultiSink(vec![Box::new(IoSink::new(tmf)), Box::new(StlDirSink::new(&dir, "p_").unwrap())]);
    let rep = squama_export(&p, &img, &e, &CancelToken::new(), &mut sink).unwrap();
    let model = read_3mf(&path).unwrap();
    assert_eq!(model.objects.len(), rep.parts.len());
    for r in &rep.parts {
        assert!(dir.join(format!("p_{}.stl", r.name)).exists());
    }
    std::fs::remove_dir_all(&dir).ok();
}
