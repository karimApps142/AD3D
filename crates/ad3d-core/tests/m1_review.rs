//! Verify round 1 (correctness review of M1): regression tests for confirmed
//! bugs, plus a randomised check of the M1 export path (hex layouts, point
//! lights, measured responses, optimiser, keys, stand, calibration tile, forced
//! grids) that the v1-only property test in `squama_export.rs` does not reach.

use ad3d_core::CancelToken;
use ad3d_core::error::Error;
use ad3d_core::mesh::validate::validate;
use ad3d_core::orient::{BedMotion, Orientation, QuarterTurns};
use ad3d_core::squama::export::{CollectSink, KeyKind, SquamaExport, TileRequest, squama_export_job};
use ad3d_core::squama::job::SquamaJob;
use ad3d_core::squama::parts::PartKind;
use ad3d_core::squama::tiles::Bed;
use proptest::prelude::*;

fn job_from(json: serde_json::Value) -> SquamaJob {
    let d: ad3d_core::squama::SquamaParamsDto = serde_json::from_value(json).unwrap();
    d.job().unwrap()
}

fn arb_bed() -> impl Strategy<Value = Bed> {
    (100.0f64..320.0, 100.0f64..320.0, 100.0f64..320.0, 0.0f64..8.0, 0u8..3).prop_map(|(x, y, z, m, mo)| Bed {
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
    #![proptest_config(ProptestConfig { cases: 32, ..ProptestConfig::default() })]

    /// Every M1 job either exports parts that all pass `validate()` and lie on
    /// their beds, or is refused with `InvalidParams` — never an internal error.
    #[test]
    fn random_m1_jobs_export_valid_parts(
        width in 40.0f64..360.0,
        height in 40.0f64..360.0,
        cell in 3.0f64..9.0,
        layout in 0u8..3,
        az in 0.0f64..360.0,
        el in 2.0f64..30.0,
        point in any::<bool>(),
        lamp in (300.0f64..1500.0, 60.0f64..600.0),
        tilt in 4.0f64..18.0,
        base in 1.0f64..5.0,
        measured in any::<bool>(),
        optimiser in any::<bool>(),
        bed in arb_bed(),
        o in arb_orientation(),
        keys in any::<bool>(),
        stand in any::<bool>(),
        calib in any::<bool>(),
        grid in prop_oneof![Just(None), (1u32..4, 1u32..4).prop_map(Some)],
    ) {
        let mut j = serde_json::json!({
            "jobId": 1, "widthMm": width, "heightMm": height, "cellMm": cell,
            "lightAzimuthDeg": az, "lightElevationDeg": el, "maxTiltDeg": tilt, "baseMm": base,
            "image": "synthetic", "quality": "final",
        });
        match layout {
            1 => j["layout"] = serde_json::json!({"kind": "hex", "orientation": "pointy"}),
            2 => j["layout"] = serde_json::json!({"kind": "hex", "orientation": "flat"}),
            _ => {}
        }
        if point {
            let a = az.to_radians();
            j["light"] = serde_json::json!({"kind": "point", "posMm": [
                width / 2.0 + lamp.0 * a.cos(), height / 2.0 + lamp.0 * a.sin(), lamp.1]});
        }
        if measured {
            j["response"] = serde_json::json!({"kind": "measured", "tiltDeg": [-20, -5, 0, 5, 20], "brightness": [0.0, 0.1, 0.3, 0.6, 1.0]});
        }
        if optimiser {
            j["optimiser"] = serde_json::json!({"kind": "shadow", "maxSweeps": 1});
        }
        let job = job_from(j);
        let e = SquamaExport {
            orientation: o,
            bed,
            keys: if keys { KeyKind::Pegs } else { KeyKind::None },
            stand,
            calibration_tile: calib,
            tiles: grid.map_or(TileRequest::Auto, |(c, r)| TileRequest::Grid(c, r)),
            // The stage is slow on big plates; topology stages still run.
            check_self_intersection: false,
            ..Default::default()
        };
        let mut sink = CollectSink::default();
        match squama_export_job(&job, None, &e, &CancelToken::new(), &mut sink) {
            Ok(rep) => {
                prop_assert!(sink.finished);
                let tiles = sink.parts.iter().filter(|(p, _)| matches!(p.kind, PartKind::Tile { .. })).count();
                prop_assert_eq!(tiles as u32, rep.cols * rep.rows);
                for (p, _) in &sink.parts {
                    let r = validate(&p.mesh, &p.validate_opts(false));
                    prop_assert!(r.ok, "{}: {}", p.name, r.summary());
                    prop_assert!((p.transform.det() - 1.0).abs() < 1e-12);
                }
                let area: f64 = sink.parts.iter().filter_map(|(p, _)| p.back_area.filter(|_| matches!(p.kind, PartKind::Tile { .. }))).sum();
                prop_assert!((area - rep.plate_mm[0] * rep.plate_mm[1]).abs() < 1e-6 * area, "tile areas {area}");
            }
            Err(Error::InvalidParams(_)) => prop_assert!(sink.parts.is_empty()),
            Err(err) => prop_assert!(false, "unexpected error {err:?}"),
        }
    }
}

/// The solver (`ResponseCurve::forward`) and the forward model / optimiser (the sim's
/// `direct` term) must read a measured table the same way. The table is measured at
/// the reference elevation (plate centre); a facet whose light sits `Δ` higher reads
/// it at `θ + Δ` in both, so under a point light the optimiser and the match score aim
/// at the response the plate was solved for.
#[test]
fn measured_response_agrees_between_solver_and_forward_model() {
    use ad3d_core::sim::{FacetLight, Light, direct_range, prepare_response};
    use ad3d_core::squama::light::CellLight;
    use ad3d_core::squama::response::Response;
    use glam::{DVec2, DVec3};
    let d = |x: f64| x.to_radians();
    // Non-linear, non-monotone table: the forward model must use the same PAV + PCHIP fit.
    let resp = Response::Measured {
        tilt_rad: vec![d(-20.0), d(-8.0), d(0.0), d(6.0), d(20.0)],
        brightness: vec![0.0, 0.12, 0.1, 0.6, 1.0],
    };
    let ref_el = d(10.0);
    let curve = resp.prepare(ref_el).unwrap();
    for el_deg in [10.0, 15.0, 6.0] {
        let el = d(el_deg);
        for irr in [1.0, 0.7] {
            for th_deg in [-8.0, -3.0, 0.0, 7.0] {
                let th = d(th_deg);
                let solver = curve.forward(th, &CellLight { a: DVec2::X, el, irradiance: irr });
                let fl = FacetLight { a: DVec2::X, el, irr };
                let forward = curve.shadowed() + curve.direct(fl.cos_tilt(th), irr);
                assert!(
                    (solver - forward).abs() < 1e-9,
                    "facet light at {el_deg}° (reference 10°), irr {irr}, tilt {th_deg}°: solver {solver:.4} vs forward model {forward:.4}"
                );
            }
        }
    }
    // The sim prepares the curve with the solver's reference elevation (plate centre, base).
    let p = ad3d_core::relief::square_grid(10, 8, 6.0, DVec2::ZERO);
    let light = Light::Point { pos_mm: DVec3::new(-300.0, 24.0, 80.0) };
    let sim_curve = prepare_response(&p, &light, &resp, 3.0).unwrap();
    let solver_ref = light.reference(60.0, 48.0, 3.0).unwrap().el;
    assert!((sim_curve.ref_el() - solver_ref).abs() < 1e-12);
    let (lo, hi) = direct_range(&FacetLight { a: DVec2::X, el: solver_ref, irr: 1.0 }, d(10.0), &sim_curve);
    assert!(hi > lo);
}

/// `light: point` overrides the v1 directional fields (job.rs), but the tile plan's
/// `Standing(Auto)` choice (and `check_params`' relief guard) still read
/// `lightAzimuthDeg`. A lamp on the plate's left must print with the light from
/// below (k = 1), whatever stale azimuth the request carries.
#[test]
fn auto_orientation_follows_the_point_light() {
    let job = job_from(serde_json::json!({
        "jobId": 1, "widthMm": 100, "heightMm": 80, "cellMm": 6,
        "lightAzimuthDeg": 90, "lightElevationDeg": 8,
        "light": {"kind": "point", "posMm": [-600, 40, 150]},
        "maxTiltDeg": 10, "baseMm": 3, "image": "synthetic"
    }));
    let e = SquamaExport { keys: KeyKind::None, stand: false, check_self_intersection: false, ..Default::default() };
    let mut sink = CollectSink::default();
    let rep = squama_export_job(&job, None, &e, &CancelToken::new(), &mut sink).unwrap();
    assert_eq!(rep.quarter_turns, Some(1), "lamp on the left: light must come from below in print");
}

/// A point lamp may sit up to 85° above a facet (light.rs), and the app's lamp sliders
/// reach ~84° at the centre (distance 100, height 1000). Ticking "calibration tile"
/// must not make such an export fail outright because the tile's own spec only
/// accepts directional elevations ≤ 45°.
#[test]
fn calibration_tile_with_a_high_point_lamp_does_not_fail_the_export() {
    let job = job_from(serde_json::json!({
        "jobId": 1, "widthMm": 90, "heightMm": 60, "cellMm": 6,
        "lightAzimuthDeg": 180, "lightElevationDeg": 8,
        "light": {"kind": "point", "posMm": [-255, 30, 600]},
        "maxTiltDeg": 10, "baseMm": 3, "image": "synthetic"
    }));
    let e = SquamaExport { keys: KeyKind::None, stand: false, calibration_tile: true, check_self_intersection: false, ..Default::default() };
    let mut sink = CollectSink::default();
    let without = SquamaExport { calibration_tile: false, ..e.clone() };
    squama_export_job(&job, None, &without, &CancelToken::new(), &mut CollectSink::default()).expect("plate alone exports");
    let r = squama_export_job(&job, None, &e, &CancelToken::new(), &mut sink);
    assert!(r.is_ok(), "adding the calibration tile failed the export: {:?}", r.err());
}

/// With a close lamp the solver has no common brightness range and falls back to
/// per-facet ranges with a `no_common_range` warning (solve.rs). Turning the
/// optimiser on must not turn that warning into a hard failure of the final plate
/// (and of every export): the drag preview works, then release / export errors.
#[test]
fn optimiser_does_not_fail_when_the_solver_falls_back_to_per_facet_ranges() {
    use ad3d_core::squama::pipeline::squama_preview;
    let mk = |quality: &str, opt: bool| {
        let mut j = serde_json::json!({
            "jobId": 1, "widthMm": 120, "heightMm": 80, "cellMm": 6, "quality": quality,
            "lightAzimuthDeg": 180, "lightElevationDeg": 8,
            "light": {"kind": "point", "posMm": [-20, 40, 25]},
            "maxTiltDeg": 10, "baseMm": 3, "image": "synthetic"
        });
        if opt {
            j["optimiser"] = serde_json::json!({"kind": "shadow", "maxSweeps": 1});
        }
        job_from(j)
    };
    let preview = squama_preview(&mk("preview", true), None, &CancelToken::new()).expect("drag preview works");
    assert!(preview.warnings.iter().any(|w| w.code == "no_common_range"), "{:?}", preview.warnings);
    squama_preview(&mk("final", false), None, &CancelToken::new()).expect("final without optimiser works");
    let r = squama_preview(&mk("final", true), None, &CancelToken::new());
    assert!(r.is_ok(), "final with optimiser failed: {:?}", r.err());
}

/// The tile overlay (`preview_tile_plan` with the plate's target) must show the seams
/// the export cuts: the export refines seams into flat areas (±25 %), so a plan with
/// balanced seams predicted the wrong lines and tile sizes (300 × 378 on the CR-6 SE).
#[test]
fn preview_tile_plan_equals_the_export_cut_lines() {
    use ad3d_core::squama::GenOptions;
    use ad3d_core::squama::build_partition;
    use ad3d_core::squama::export::preview_tile_plan;
    use ad3d_core::squama::pipeline::{job_layout, prepare_target, synthetic_image};
    let job = job_from(serde_json::json!({
        "jobId": 1, "widthMm": 300, "heightMm": 378, "cellMm": 6, "quality": "final",
        "lightAzimuthDeg": 180, "lightElevationDeg": 5, "maxTiltDeg": 10, "baseMm": 3, "image": "synthetic"
    }));
    let img = synthetic_image();
    let (l, _) = job_layout(&job, (img.w, img.h), &GenOptions::default()).unwrap();
    let p = build_partition(&l, &[]).unwrap();
    let t = prepare_target(&img, &job.image_opts, &l, &p).unwrap();
    let e = SquamaExport { check_self_intersection: false, ..Default::default() };
    let (plan, _) = preview_tile_plan(&job, &l, Some(&t.g), &e, &CancelToken::new()).unwrap();
    let mut sink = CollectSink::default();
    let rep = squama_export_job(&job, None, &e, &CancelToken::new(), &mut sink).unwrap();
    assert!(rep.cols * rep.rows > 1);
    assert_eq!(plan.u_edges(), rep.u_edges_mm);
    assert_eq!(plan.v_edges(), rep.v_edges_mm);
    assert_eq!(plan.quarter_turns, rep.quarter_turns);
}

/// A 3MF/STL consumer opens each bed on its own: every part's transform is in its own
/// bed's coordinates (inside the usable area), never offset beside the first bed.
#[test]
fn every_part_lies_on_its_own_bed() {
    let job = job_from(serde_json::json!({
        "jobId": 1, "widthMm": 300, "heightMm": 378, "cellMm": 6,
        "lightAzimuthDeg": 180, "lightElevationDeg": 5, "maxTiltDeg": 10, "baseMm": 3, "image": "synthetic"
    }));
    let e = SquamaExport { stand: true, calibration_tile: true, check_self_intersection: false, ..Default::default() };
    let mut sink = CollectSink::default();
    let rep = squama_export_job(&job, None, &e, &CancelToken::new(), &mut sink).unwrap();
    assert!(rep.beds_used > 1, "this export needs more than one bed");
    let (u, m) = (e.bed.usable(), e.bed.margin_mm);
    for (p, r) in &sink.parts {
        assert!(r.min_mm[0] >= m - 1e-6 && r.max_mm[0] <= m + u[0] + 1e-6, "{} X {:?}..{:?}", p.name, r.min_mm, r.max_mm);
        assert!(r.min_mm[1] >= m - 1e-6 && r.max_mm[1] <= m + u[1] + 1e-6, "{} Y", p.name);
        assert!(r.fits_bed);
    }
}

/// A forced grid with keys that makes tiles too narrow for the key holes is refused
/// with a message that says what to change (not a back-region collision).
#[test]
fn forced_grid_too_narrow_for_keys_says_so() {
    let job = job_from(serde_json::json!({
        "jobId": 1, "widthMm": 80, "heightMm": 60, "cellMm": 4,
        "lightAzimuthDeg": 180, "lightElevationDeg": 8, "maxTiltDeg": 10, "baseMm": 3.2, "image": "synthetic"
    }));
    let e = SquamaExport { keys: KeyKind::Pegs, stand: false, tiles: TileRequest::Grid(5, 1), check_self_intersection: false, ..Default::default() };
    let err = squama_export_job(&job, None, &e, &CancelToken::new(), &mut CollectSink::default()).unwrap_err();
    assert!(err.to_string().contains("too narrow for alignment keys"), "{err}");
}
