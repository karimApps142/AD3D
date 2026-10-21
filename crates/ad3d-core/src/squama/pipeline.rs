//! The M1 plate pipeline (§12, §13, §16) as separately cacheable stages:
//!
//! 1. [`job_layout`]: snapped layout from the job and the source image size;
//! 2. [`build_partition`](super::build_partition): the 2D cells (independent of the image);
//! 3. [`job_target`] / [`prepare_target`]: per-facet linear targets (§16);
//! 4. [`solve_job`]: facet planes — the Lambert / measured inverse, plus the
//!    shadow-aware optimiser at `Quality::Final` when the job asks for it (§15.1);
//! 5. [`plate_from_target`]: quantise and mesh (with face kinds), plus the match
//!    score and simulated appearance at `Quality::Final`.
//!
//! [`squama_preview`] runs every stage uncached (§12 entry point). The app keeps
//! the image stages in an `image::TargetPipeline` and the partition per layout,
//! so a slider drag only re-runs stages 3–5 (§13).

use std::time::Instant;

use serde::Serialize;

use super::job::SquamaJob;
use super::layout::{Layout, build_partition, layout_for};
use super::optimise::{OptimiseReport, OptimiserOpts, optimise_from};
use super::solve::{Limits, PivotRule, Quality, SolveOpts, SolveReport, Solved, solve_facets_ex};
use super::{GenOptions, SYNTHETIC_PX, check_params_for};
use crate::cancel::CancelToken;
use crate::error::{Error, Result, Warning};
use crate::image::{LinearLuma, Target, TargetSampler, synthetic_target};
use crate::image::{ToneLut, build_working, working::DEFAULT_MAX_SIDE};
use crate::mesh::Mesh;
use crate::relief::{Facet, Heights, MeshStats, Partition, PieceSet, mesh_relief_stats, quantise};
use crate::sim::MatchScore;

/// The source image of a job: the built-in synthetic target or a decoded photo.
pub fn synthetic_image() -> LinearLuma {
    synthetic_target(SYNTHETIC_PX.0, SYNTHETIC_PX.1)
}

/// Keeps every warning code once (first message wins).
pub fn dedup_warnings(ws: &mut Vec<Warning>) {
    let mut seen = std::collections::HashSet::new();
    ws.retain(|w| seen.insert(w.code));
}

/// §2.1 / §6.2: the snapped layout for `job` on a source image of `src_px`
/// pixels (before the job's rotation and crop), plus the parameter warnings.
pub fn job_layout(job: &SquamaJob, src_px: (u32, u32), gopts: &GenOptions) -> Result<(Layout, Vec<Warning>)> {
    // Under a point light every facet has its own direction; the solver guards the relief.
    let guard_az = match job.light {
        super::light::Light::Directional { .. } => Some(job.params.light_azimuth),
        super::light::Light::Point { .. } => None,
    };
    let (pc, warnings) = check_params_for(&job.params, gopts, guard_az)?;
    let (cw, ch) = job.image_opts.cropped_size_px(src_px.0.max(1), src_px.1.max(1));
    if !(cw > 0.0 && ch > 0.0) {
        return Err(Error::InvalidParams("empty crop".into()));
    }
    // layout_for only uses the aspect: keep sub-pixel crop precision.
    let s = 1.0e6 / cw.max(ch);
    let px = ((cw * s).round().max(1.0) as u32, (ch * s).round().max(1.0) as u32);
    let l = layout_for(&pc, job.layout, px)?;
    if !(l.w_mm <= 2048.0 + 1e-9 && l.h_mm <= 2048.0 + 1e-9) {
        return Err(Error::InvalidParams(format!("plate {:.1} × {:.1} mm above 2048 mm", l.w_mm, l.h_mm)));
    }
    Ok((l, warnings))
}

/// §16 step 7 on a prepared sampler (the job's crop window): per-facet targets
/// over the layout's centre crop.
pub fn job_target(sampler: &TargetSampler, l: &Layout, p: &Partition) -> Result<Target> {
    sampler.cropped(l.crop)?.facet_targets(p, (l.w_mm, l.h_mm))
}

/// §12 `prepare_target`, uncached: working copy, tone, SAT, per-facet means.
pub fn prepare_target(img: &LinearLuma, opts: &crate::image::ImageOpts, l: &Layout, p: &Partition) -> Result<Target> {
    opts.check()?;
    let wk = build_working(img, opts, DEFAULT_MAX_SIDE)?;
    let lut = ToneLut::new(opts, Some(&wk.hist));
    job_target(&TargetSampler::new(&wk, &lut), l, p)
}

/// The solve stage's output.
#[derive(Debug, Clone, PartialEq)]
pub struct SolvedJob {
    pub facets: Vec<Facet>,
    pub solved: Solved,
    /// Set when the shadow-aware optimiser ran (Final quality, optimiser on).
    pub optimise: Option<OptimiseReport>,
}

/// §3, §15: facet planes for the job's light, limits and response. At
/// `Quality::Final` with `optimiser: shadow`, the inverse is refined by the
/// shadow-aware optimiser on partition `p` (the same cells, same pivots).
pub fn solve_job(
    job: &SquamaJob,
    l: &Layout,
    p: &Partition,
    g: &[f32],
    gopts: &GenOptions,
    cancel: &CancelToken,
) -> Result<SolvedJob> {
    let opts = SolveOpts { pivot: PivotRule::Common, relief_guard: gopts.relief_guard };
    let solved = solve_facets_ex(g, l, &job.light, &job.limits, &job.response, &opts, job.quality, &gopts.tol, cancel)?;
    let mut solved = solved;
    let mut facets = solved.facets.clone();
    let mut optimise = None;
    if job.quality == Quality::Final
        && let Some(o) = job.optimiser.filter(|o| o.max_sweeps > 0)
    {
        cancel.check()?;
        // With no common brightness range the solver falls back to per-facet ranges;
        // the optimiser needs one shared range, so keep the inverse and say so.
        let skip = |why: String| Warning { code: "optimiser_skipped", message: format!("shadow optimiser skipped: {why}") };
        if solved.report.per_facet_range {
            solved.warnings.push(skip("no brightness range is reachable by every facet (move the lamp farther away)".into()));
        } else {
            let lim = optimiser_limits(job, &solved.report);
            match optimise_from(g, p, &facets, &job.light, &lim, &job.response, &o, cancel) {
                Ok(r) => {
                    facets = r.facets.clone();
                    optimise = Some(r);
                }
                Err(Error::InvalidParams(m)) => solved.warnings.push(skip(m)),
                Err(e) => return Err(e),
            }
        }
    }
    Ok(SolvedJob { facets, solved, optimise })
}

/// The optimiser keeps the solver's effective tilt (after a relief cap) and the
/// relief budget, so an optimised plate is never thicker than the plain one.
fn optimiser_limits(job: &SquamaJob, rep: &SolveReport) -> Limits {
    Limits { max_tilt: rep.max_tilt_used.min(job.limits.max_tilt), ..job.limits }
}

/// Predicted appearance of a plate (§15.1 forward model).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Appearance {
    /// Match against the target (Pearson r, RMSE, contrast).
    pub score: MatchScore,
    /// Score of the plain inverse, when the optimiser ran.
    pub score_before: Option<MatchScore>,
    /// Simulated brightness per facet, unnormalised: linear radiance in the
    /// response's units (Lambert: `ambient + I·(n·L)·lit_fraction`, the lamp alone
    /// with the default ambient 0). The viewer adds its preview ambient and
    /// normalises to the brightest facet, exactly like the 3D preview's shader.
    #[serde(skip)]
    pub sim: Vec<f32>,
}

/// §15.1: scores `facets` against target `g` with cast shadows, and returns the
/// simulated per-facet appearance. `optimised` reuses an optimiser report.
pub fn appearance(
    job: &SquamaJob,
    p: &Partition,
    facets: &[Facet],
    g: &[f32],
    solved: &SolveReport,
    optimised: Option<&OptimiseReport>,
) -> Result<Appearance> {
    let lim = optimiser_limits(job, solved);
    let rep = match optimised {
        Some(r) => r.clone(),
        None => {
            let o = OptimiserOpts { max_sweeps: 0, ..OptimiserOpts::default() };
            optimise_from(g, p, facets, &job.light, &lim, &job.response, &o, &CancelToken::new())?
        }
    };
    let curve = crate::sim::prepare_response(p, &job.light, &job.response, job.limits.base_mm)?;
    let b = crate::sim::shade_curve(p, facets, &job.light, &curve, 0)?;
    let sim = b.iter().map(|&v| v.max(0.0) as f32).collect();
    Ok(Appearance { score: rep.after, score_before: optimised.map(|r| r.before), sim })
}

/// Timings of one plate (ms).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineTimings {
    pub target_ms: f64,
    pub solve_ms: f64,
    pub optimise_ms: f64,
    pub quantise_ms: f64,
    pub mesh_ms: f64,
    pub appearance_ms: f64,
    pub total_ms: f64,
}

/// One whole plate (one part), ready for the viewport.
#[derive(Debug, Clone, PartialEq)]
pub struct PlateResult {
    pub mesh: Mesh,
    pub layout: Layout,
    pub target: Target,
    pub facets: Vec<Facet>,
    pub heights: Heights,
    pub warnings: Vec<Warning>,
    pub report: SolveReport,
    pub optimise: Option<OptimiseReport>,
    /// The plain inverse (before the optimiser), so an export or tile plan can
    /// reuse this solve ([`PlateResult::solved_job`]).
    pub solved: Solved,
    /// Final quality only.
    pub appearance: Option<Appearance>,
    pub mesh_stats: MeshStats,
    pub timings: PipelineTimings,
    /// For `validate()`: lowest allowed facet height and the outline area.
    pub min_facet_w: f64,
    pub back_area: f64,
}

impl PlateResult {
    /// The solve behind this plate, for [`crate::squama::export::export_plate_solved`]
    /// and [`crate::squama::export::preview_tile_plan_solved`] (final quality only:
    /// a preview solve differs from the export's).
    pub fn solved_job(&self) -> SolvedJob {
        SolvedJob { facets: self.facets.clone(), solved: self.solved.clone(), optimise: self.optimise.clone() }
    }

    pub fn validate_opts(&self, transform: crate::orient::Xform) -> crate::mesh::validate::ValidateOpts {
        crate::mesh::validate::ValidateOpts {
            transform,
            min_facet_w: Some(self.min_facet_w),
            back_area: Some(self.back_area),
            ..Default::default()
        }
    }
}

/// Stages 4–5 for a job whose layout, partition and target are known.
/// `warnings` are the layout stage's; solver warnings are appended.
pub fn plate_from_target(
    job: &SquamaJob,
    l: &Layout,
    p: &Partition,
    target: Target,
    mut warnings: Vec<Warning>,
    gopts: &GenOptions,
    cancel: &CancelToken,
) -> Result<PlateResult> {
    let t0 = Instant::now();
    cancel.check()?;
    let sj = solve_job(job, l, p, &target.g, gopts, cancel)?;
    let t1 = Instant::now();
    let heights = quantise(p, &sj.facets, job.limits.base_mm, &gopts.tol)?;
    let t2 = Instant::now();
    cancel.check()?;
    let (mesh, mesh_stats) = mesh_relief_stats(p, PieceSet::All, &heights, &[], &gopts.tol, cancel)?;
    let t3 = Instant::now();
    warnings.extend(sj.solved.warnings.iter().cloned());
    let appearance = if job.quality == Quality::Final {
        match appearance(job, p, &sj.facets, &target.g, &sj.solved.report, sj.optimise.as_ref()) {
            Ok(a) => Some(a),
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => {
                warnings.push(Warning { code: "no_appearance", message: format!("appearance not simulated: {e}") });
                None
            }
        }
    } else {
        None
    };
    if let Some(o) = &sj.optimise
        && o.timed_out
    {
        warnings.push(Warning {
            code: "optimiser_timed_out",
            message: format!("optimiser stopped after {} sweep(s) (time budget)", o.sweeps),
        });
    }
    let t4 = Instant::now();
    dedup_warnings(&mut warnings);
    let ms = |a: Instant, b: Instant| (b - a).as_secs_f64() * 1e3;
    let optimise_ms = sj.optimise.as_ref().map_or(0.0, |o| o.elapsed_ms);
    Ok(PlateResult {
        mesh,
        layout: *l,
        target,
        facets: sj.facets,
        heights,
        warnings,
        report: sj.solved.report.clone(),
        solved: sj.solved,
        optimise: sj.optimise,
        appearance,
        mesh_stats,
        timings: PipelineTimings {
            target_ms: 0.0,
            solve_ms: ms(t0, t1) - optimise_ms,
            optimise_ms,
            quantise_ms: ms(t1, t2),
            mesh_ms: ms(t2, t3),
            appearance_ms: ms(t3, t4),
            total_ms: ms(t0, t4),
        },
        min_facet_w: job.limits.base_mm - gopts.tol.q / 2.0,
        back_area: l.w_mm * l.h_mm,
    })
}

/// §12 `squama_preview`: the whole plate as one part, every stage uncached.
/// `img` is the decoded source (`None` = the synthetic target).
pub fn squama_preview(job: &SquamaJob, img: Option<&LinearLuma>, cancel: &CancelToken) -> Result<PlateResult> {
    let gopts = GenOptions::default();
    let synth;
    let img = match img {
        Some(i) => i,
        None => {
            synth = synthetic_image();
            &synth
        }
    };
    let t0 = Instant::now();
    let (l, warnings) = job_layout(job, (img.w, img.h), &gopts)?;
    let p = build_partition(&l, &[])?;
    let target = prepare_target(img, &job.image_opts, &l, &p)?;
    let target_ms = t0.elapsed().as_secs_f64() * 1e3;
    let mut out = plate_from_target(job, &l, &p, target, warnings, &gopts, cancel)?;
    out.timings.target_ms = target_ms;
    out.timings.total_ms += target_ms;
    Ok(out)
}

/// Per-facet values as a square raster for the 2D views (row 0 at the plate
/// bottom): a square layout is its own grid; a hex layout is sampled at
/// half-cell resolution (each sample takes the value of the hex it falls in).
pub fn facet_raster(l: &Layout, values: &[f32]) -> (u32, u32, Vec<f32>) {
    if l.hex_lattice().is_none() {
        return (l.nx, l.ny, values.to_vec());
    }
    let step = l.cell_mm / 2.0;
    let nx = ((l.w_mm / step).round() as u32).max(1);
    let ny = ((l.h_mm / step).round() as u32).max(1);
    let (sx, sy) = (l.w_mm / nx as f64, l.h_mm / ny as f64);
    let mut out = Vec::with_capacity((nx * ny) as usize);
    for j in 0..ny {
        for i in 0..nx {
            let p = glam::DVec2::new((i as f64 + 0.5) * sx, (j as f64 + 0.5) * sy);
            out.push(l.facet_at(p).and_then(|f| values.get(f as usize)).copied().unwrap_or(0.0));
        }
    }
    (nx, ny, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::validate::validate;
    use crate::orient::Xform;
    use crate::squama::job::{LayoutDto, LightDto, OptimiserDto, HexOrientationDto};
    use crate::squama::{SquamaParams, SquamaParamsDto};

    fn dto(width: f64, cell: f64) -> SquamaParamsDto {
        serde_json::from_value(serde_json::json!({
            "jobId": 1, "widthMm": width, "cellMm": cell, "lightAzimuthDeg": 180.0,
            "lightElevationDeg": 5.0, "maxTiltDeg": 10.0, "baseMm": 3.0, "image": "synthetic"
        }))
        .unwrap()
    }

    #[test]
    fn square_preview_matches_m0() {
        let d = dto(120.0, 6.0);
        let job = d.job().unwrap();
        let out = squama_preview(&job, None, &CancelToken::new()).unwrap();
        let m0 = crate::squama::run(&d.params(), crate::squama::ImageSource::Synthetic, &GenOptions::default(), &CancelToken::new())
            .unwrap();
        assert_eq!((out.layout.nx, out.layout.ny), (m0.layout.nx, m0.layout.ny));
        assert_eq!(out.mesh.triangle_count(), m0.mesh.triangle_count());
        assert!(!out.mesh.face_kind.is_empty());
        assert!(out.appearance.is_none());
        assert!(validate(&out.mesh, &out.validate_opts(Xform::IDENTITY)).ok);
    }

    #[test]
    fn final_hex_point_light_optimised_is_watertight_and_scored() {
        let mut d = dto(90.0, 6.0);
        d.quality = Some(crate::squama::job::QualityDto::Final);
        d.layout = Some(LayoutDto::Hex { orientation: HexOrientationDto::Pointy });
        d.light = Some(LightDto::Point { pos_mm: [-400.0, 60.0, 120.0] });
        d.optimiser = Some(OptimiserDto::Shadow { max_sweeps: Some(2) });
        d.height_mm = Some(70.0);
        let job = d.job().unwrap();
        let out = squama_preview(&job, None, &CancelToken::new()).unwrap();
        assert!(out.optimise.is_some());
        let a = out.appearance.as_ref().expect("appearance at final");
        assert_eq!(a.sim.len(), out.target.g.len());
        assert!(a.score_before.is_some());
        let r = validate(&out.mesh, &out.validate_opts(Xform::IDENTITY));
        assert!(r.ok, "{}", r.summary());
    }

    #[test]
    fn crop_and_height_snap_follow_the_image_opts() {
        let mut d = dto(100.0, 5.0);
        d.image_opts = Some(crate::image::ImageOpts { crop: Some([0.0, 0.0, 1.0, 0.5]), ..Default::default() });
        let job = d.job().unwrap();
        let (l, _) = job_layout(&job, (400, 400), &GenOptions::default()).unwrap();
        assert_eq!((l.nx, l.ny), (20, 10));
        // Rotated 90°: a 400×200 source becomes 200×400.
        d.image_opts = Some(crate::image::ImageOpts { rotate_deg: 90, ..Default::default() });
        let (l, _) = job_layout(&d.job().unwrap(), (400, 200), &GenOptions::default()).unwrap();
        assert_eq!((l.nx, l.ny), (20, 40));
    }

    #[test]
    fn facet_at_finds_the_nearest_hex_centre() {
        use crate::squama::layout::{HexOrientation, LayoutKind, layout_for};
        for o in [HexOrientation::Pointy, HexOrientation::Flat] {
            let p = SquamaParams { width_mm: 60.0, height_mm: Some(50.0), cell_mm: 5.0, ..SquamaParams::default() };
            let l = layout_for(&p, LayoutKind::Hex { orientation: o }, (100, 100)).unwrap();
            for f in 0..l.facet_count() {
                let c = l.facet_centre(f);
                if c.x > 0.1 && c.y > 0.1 && c.x < l.w_mm - 0.1 && c.y < l.h_mm - 0.1 {
                    assert_eq!(l.facet_at(c + glam::DVec2::new(0.3, -0.2)), Some(f), "{o:?} {f}");
                }
            }
            let (nx, ny, r) = facet_raster(&l, &(0..l.facet_count()).map(|f| f as f32).collect::<Vec<_>>());
            assert_eq!(r.len(), (nx * ny) as usize);
        }
    }

    #[test]
    fn cancel_is_honoured() {
        let job = SquamaJob::from_params(&SquamaParams::default());
        let c = CancelToken::new();
        c.cancel();
        assert!(matches!(squama_preview(&job, None, &c), Err(Error::Cancelled)));
    }
}
