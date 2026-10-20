//! Verify round 1 (M1 correctness review): regression tests for confirmed app-side bugs.

use std::path::PathBuf;

use ad3d_core::CancelToken;

use crate::squama_cmds::{ExportFormat, KeysDto, SquamaExportDto, SquamaParamsM1, SquamaState};

fn tmp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ad3d-m1-review-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// "Tile to bed" off (no `bedMm`): a 300 × 378 mm plate is exported in one piece on a
/// 235 mm bed, the core warns `part_exceeds_bed` — but the part report still says
/// `fitsBed: true`, because `AppSink::accept` hard-codes it.
#[test]
fn oversized_single_part_is_not_reported_as_fitting_the_bed() {
    let dir = tmp_dir("fits");
    let st = SquamaState::default();
    let params: SquamaParamsM1 = serde_json::from_value(serde_json::json!({
        "jobId": 0, "widthMm": 300.0, "cellMm": 6.0, "lightAzimuthDeg": 180.0,
        "lightElevationDeg": 5.0, "maxTiltDeg": 10.0, "baseMm": 3.0, "image": "synthetic"
    }))
    .unwrap();
    let e = SquamaExportDto {
        params,
        out_dir: dir.display().to_string(),
        base_name: Some("big".into()),
        formats: vec![ExportFormat::Stl],
        orientation: "standing-auto".into(),
        bed_mm: None,
        margin_mm: Some(5.0),
        bed_motion: None,
        keys: Some(KeysDto::None),
        stand: Some(false),
        calibration_tile: Some(false),
    };
    let r = st.export(&e, &CancelToken::new(), &mut |_| {}).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(r.warnings.iter().any(|w| w.code == "part_exceeds_bed"), "{:?}", r.warnings);
    let plate = r.parts.iter().find(|p| p.name == "plate").expect("one plate part");
    assert_eq!(plate.fits_bed, Some(false), "plate {:?} mm reported as fitting a 235 mm bed", plate.size_mm);
}
