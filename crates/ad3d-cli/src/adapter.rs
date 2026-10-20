//! Squama job (design §12.1 params JSON + project export settings) → validated
//! export parts, through the core's M1 pipeline:
//! - [`squama::pipeline::squama_preview`] for the whole-plate preview (stats and
//!   the project/3MF thumbnail);
//! - [`squama::export::squama_export_job`] for the parts: §16 image options,
//!   square or hex cells, directional or point light, Lambert/ambient/measured
//!   response, the shadow optimiser, `maxReliefMm`, tiling to the bed, keys,
//!   stand and the calibration tile. Every part is validated by the core.

use ad3d_core::CancelToken;
use ad3d_core::error::{Error, Result, Warning};
use ad3d_core::image::LinearLuma;
use ad3d_core::io::sink::ExportPart;
use ad3d_core::mesh::validate::ValidateOpts;
use ad3d_core::profiles::PrinterProfile;
use ad3d_core::project::ExportSettings;
use ad3d_core::squama::SquamaParamsDto;
use ad3d_core::squama::export::{CollectSink, ExportReport, io_part_kind, squama_export_job};
use ad3d_core::squama::job::SquamaJob;
use ad3d_core::squama::parts::PartKind;
use ad3d_core::squama::pipeline::{PlateResult, dedup_warnings, squama_preview};
use ad3d_core::squama::solve::Quality;
use serde_json::Value;

/// One generated part with the `validate()` options that match it.
pub struct GenPart {
    pub part: ExportPart,
    pub validate: ValidateOpts,
    /// Resolved quarter turns (None = flat).
    pub quarter_turns: Option<u32>,
    /// Bed (print job) the core placed it on; its transform is in that bed's coordinates.
    pub bed: u32,
    /// Largest downward-facing ledge (standing tiles, calibration tile).
    pub max_ledge_mm: Option<f64>,
}

pub struct GenResult {
    /// The whole plate at preview quality (one part), for stats and thumbnails.
    pub plate: PlateResult,
    pub job: SquamaJob,
    pub parts: Vec<GenPart>,
    pub warnings: Vec<Warning>,
    /// Tile grid used (columns along u, rows along v).
    pub tiles: [u32; 2],
    pub report: ExportReport,
}

/// The §12.1 params JSON as a job (`jobId` optional).
pub fn job_of(params: &Value) -> Result<SquamaJob> {
    let mut v = params.clone();
    if let Some(o) = v.as_object_mut() {
        o.entry("jobId").or_insert(Value::from(0));
    }
    let dto: SquamaParamsDto =
        serde_json::from_value(v).map_err(|e| Error::InvalidParams(format!("squama params: {e}")))?;
    dto.job()
}

/// Generates every part for a job. `luma` is `None` for the synthetic target.
pub fn generate(
    params: &Value,
    luma: Option<&LinearLuma>,
    export: &ExportSettings,
    printer: &PrinterProfile,
    cancel: &CancelToken,
) -> Result<GenResult> {
    let job = job_of(params)?;
    let plate = squama_preview(&job.with_quality(Quality::Preview), luma, cancel)?;
    let mut e = export.squama_export(printer);
    // The CLI re-validates every part (with `--strict` adding the
    // self-intersection stage), so the core skips that slow stage here.
    e.check_self_intersection = false;
    let mut sink = CollectSink::default();
    let report = squama_export_job(&job.with_quality(Quality::Final), luma, &e, cancel, &mut sink)?;
    let parts = sink
        .parts
        .into_iter()
        .map(|(p, pr)| {
            let oriented = matches!(p.kind, PartKind::Tile { .. } | PartKind::Calibration);
            GenPart {
                validate: p.validate_opts(false),
                quarter_turns: if oriented { report.quarter_turns } else { None },
                bed: p.bed,
                max_ledge_mm: pr.max_downward_ledge_mm,
                part: ExportPart { name: p.name, kind: io_part_kind(&p.kind), mesh: p.mesh, transform: p.transform },
            }
        })
        .collect();
    let mut warnings = plate.warnings.clone();
    warnings.extend(report.warnings.iter().cloned());
    dedup_warnings(&mut warnings);
    Ok(GenResult { tiles: [report.cols, report.rows], plate, job, parts, warnings, report })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ad3d_core::mesh::validate::validate;
    use ad3d_core::profiles::default_printer;
    use ad3d_core::project::{KeysDto, TileMode, TilesDto};
    use serde_json::json;

    fn params(width: f64) -> Value {
        json!({ "widthMm": width, "cellMm": 6.0, "lightAzimuthDeg": 180.0, "lightElevationDeg": 5.0,
                "maxTiltDeg": 10.0, "baseMm": 3.0, "image": "synthetic" })
    }

    #[test]
    fn auto_tiles_fit_the_cr6se_and_validate() {
        let ex = ExportSettings { tiles: TilesDto::Mode(TileMode::Auto), ..Default::default() };
        let r = generate(&params(300.0), None, &ex, &default_printer(), &CancelToken::new()).unwrap();
        assert!(r.parts.len() > 1, "300 × 378 standing must be tiled on 235 × 235 × 250");
        assert!(r.warnings.iter().all(|w| w.code != "part_exceeds_bed"), "{:?}", r.warnings);
        for gp in &r.parts {
            let rep = validate(&gp.part.mesh, &gp.validate);
            assert!(rep.ok, "{}: {}", gp.part.name, rep.summary());
        }
        let tris: usize = r.parts.iter().map(|p| p.part.mesh.triangle_count()).sum();
        assert!(tris > r.plate.mesh.triangle_count());
    }

    #[test]
    fn m1_fields_run_end_to_end() {
        let mut p = params(120.0);
        p["layout"] = json!({ "kind": "hex", "orientation": "pointy" });
        p["light"] = json!({ "kind": "point", "posMm": [-500.0, 60.0, 200.0] });
        p["optimiser"] = json!({ "kind": "shadow", "maxSweeps": 1 });
        p["response"] = json!({ "kind": "lambert", "ambient": 0.1 });
        p["maxReliefMm"] = json!(3.0);
        let ex = ExportSettings { keys: KeysDto::Pegs, stand: true, calibration_tile: true, ..Default::default() };
        let r = generate(&p, None, &ex, &default_printer(), &CancelToken::new()).unwrap();
        assert!(r.report.calibration_key.is_some());
        assert!(r.report.optimise.is_some());
        for gp in &r.parts {
            assert!(validate(&gp.part.mesh, &gp.validate).ok, "{}", gp.part.name);
        }
    }

    #[test]
    fn forced_grid_and_none() {
        let ex = ExportSettings { tiles: TilesDto::Grid([3, 1]), ..Default::default() };
        let r = generate(&params(150.0), None, &ex, &default_printer(), &CancelToken::new()).unwrap();
        assert_eq!(r.tiles, [3, 1]);
        let ex = ExportSettings { tiles: TilesDto::Mode(TileMode::None), ..Default::default() };
        let r = generate(&params(300.0), None, &ex, &default_printer(), &CancelToken::new()).unwrap();
        assert_eq!(r.tiles, [1, 1]);
        assert!(r.warnings.iter().any(|w| w.code == "part_exceeds_bed"));
    }

    #[test]
    fn bad_params_are_refused() {
        let mut p = params(60.0);
        p["imageOpts"] = json!({ "gamma": 9.0 });
        assert!(generate(&p, None, &ExportSettings::default(), &default_printer(), &CancelToken::new()).is_err());
    }
}
