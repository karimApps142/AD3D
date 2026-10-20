//! UX review (verify round 1): times the app's preview/final path the way
//! `src-tauri/src/squama_cmds.rs::SquamaState::generate` runs it (cached partition and
//! sampler, then `plate_from_target`). Run in release:
//!   cargo test -p ad3d-core --release --test ux_latency -- --ignored --nocapture
use std::sync::Arc;
use std::time::Instant;

use ad3d_core::CancelToken;
use ad3d_core::image::working::{DEFAULT_MAX_SIDE, DRAG_MAX_SIDE};
use ad3d_core::image::{ImageOpts, LinearLuma, TargetPipeline};
use ad3d_core::payload;
use ad3d_core::squama::job::{LayoutDto, HexOrientationDto, OptimiserDto, QualityDto};
use ad3d_core::squama::pipeline::{job_layout, job_target, plate_from_target};
use ad3d_core::squama::{GenOptions, SquamaParamsDto, build_partition};

fn photo(w: u32, h: u32) -> LinearLuma {
    let px = (0..w * h)
        .map(|k| {
            let (x, y) = ((k % w) as f32 / w as f32, (k / w) as f32 / h as f32);
            (0.5 + 0.4 * (9.0 * x).sin() * (7.0 * y).cos()).clamp(0.0, 1.0)
        })
        .collect();
    LinearLuma { w, h, px }
}

fn dto(width: f64, cell: f64) -> SquamaParamsDto {
    serde_json::from_value(serde_json::json!({
        "jobId": 1, "widthMm": width, "cellMm": cell, "lightAzimuthDeg": 180.0,
        "lightElevationDeg": 5.0, "maxTiltDeg": 10.0, "baseMm": 3.0, "image": "photo"
    }))
    .unwrap()
}

fn run(label: &str, pipe: &mut TargetPipeline, d: &SquamaParamsDto, reps: usize) {
    let src = pipe.source().clone();
    let gopts = GenOptions::default();
    let mut best = f64::INFINITY;
    let mut info = String::new();
    for _ in 0..reps {
        let t0 = Instant::now();
        let job = d.job().unwrap();
        let (l, w) = job_layout(&job, (src.w, src.h), &gopts).unwrap();
        let part = build_partition(&l, &[]).unwrap(); // the app caches this; timed separately below
        let tp = Instant::now();
        let side = if job.quality == ad3d_core::squama::solve::Quality::Final { DEFAULT_MAX_SIDE } else { DRAG_MAX_SIDE };
        let s = pipe.sampler(&job.image_opts, side).unwrap();
        let t = job_target(&s, &l, &part).unwrap();
        let out = plate_from_target(&job, &l, &part, t, w, &gopts, &CancelToken::new()).unwrap();
        let buf = payload::encode_ex(&out.mesh, 1, 0.0, payload::FLAG_FACE_KINDS, None);
        let ms = (Instant::now() - tp).as_secs_f64() * 1e3;
        let part_ms = (tp - t0).as_secs_f64() * 1e3;
        if ms < best {
            best = ms;
            info = format!(
                "{}x{} cells, {} tris, {:.2} MB, partition {:.1} ms (cached in app), timings {:?}",
                l.nx, l.ny, out.mesh.triangle_count(), buf.len() as f64 / 1e6, part_ms, out.timings
            );
        }
    }
    println!("{label:<44} best {best:8.1} ms  | {info}");
}

#[test]
#[ignore]
fn ux_latency_table() {
    let t0 = Instant::now();
    let mut pipe = TargetPipeline::new(Arc::new(photo(6000, 4000)));
    println!("hash 24 MP source: {:.0} ms", t0.elapsed().as_secs_f64() * 1e3);
    let t0 = Instant::now();
    pipe.sampler(&ImageOpts::default(), DRAG_MAX_SIDE).unwrap();
    println!("first 1024 sampler from 24 MP: {:.0} ms", t0.elapsed().as_secs_f64() * 1e3);
    let t0 = Instant::now();
    pipe.sampler(&ImageOpts::default(), DEFAULT_MAX_SIDE).unwrap();
    println!("first 2048 sampler from 24 MP: {:.0} ms", t0.elapsed().as_secs_f64() * 1e3);
    for (i, g) in [1.1, 1.2, 1.3].iter().enumerate() {
        let t0 = Instant::now();
        pipe.sampler(&ImageOpts { gamma: *g, ..ImageOpts::default() }, DRAG_MAX_SIDE).unwrap();
        println!("gamma drag tick {i} (1024 tone rebuild): {:.1} ms", t0.elapsed().as_secs_f64() * 1e3);
    }
    for (i, c) in [0.02, 0.03, 0.04].iter().enumerate() {
        let t0 = Instant::now();
        pipe.sampler(&ImageOpts { crop: Some([*c, *c, 1.0 - c, 1.0 - c]), ..ImageOpts::default() }, DRAG_MAX_SIDE).unwrap();
        println!("crop drag tick {i} (1024 working rebuild from 24 MP): {:.1} ms", t0.elapsed().as_secs_f64() * 1e3);
    }

    let mut d = dto(300.0, 6.0);
    d.quality = Some(QualityDto::Preview);
    run("preview 300 mm / 6 mm square (default)", &mut pipe, &d, 5);
    let mut d = dto(300.0, 3.0);
    d.quality = Some(QualityDto::Preview);
    run("preview 300 mm / 3 mm square (100x126)", &mut pipe, &d, 5);
    let mut dh = dto(300.0, 6.0);
    dh.quality = Some(QualityDto::Preview);
    dh.layout = Some(LayoutDto::Hex { orientation: HexOrientationDto::Pointy });
    run("preview 300 mm / 6 mm hex", &mut pipe, &dh, 5);
    let mut d = dto(300.0, 6.0);
    d.quality = Some(QualityDto::Final);
    run("final 300/6 square, no optimiser", &mut pipe, &d, 3);
    d.optimiser = Some(OptimiserDto::Shadow { max_sweeps: None });
    run("final 300/6 square, optimiser default", &mut pipe, &d, 1);
    let mut d = dto(300.0, 3.0);
    d.quality = Some(QualityDto::Final);
    d.optimiser = Some(OptimiserDto::Shadow { max_sweeps: None });
    run("final 300/3 square, optimiser default", &mut pipe, &d, 1);
    let mut dh = dto(300.0, 6.0);
    dh.quality = Some(QualityDto::Final);
    dh.layout = Some(LayoutDto::Hex { orientation: HexOrientationDto::Pointy });
    dh.optimiser = Some(OptimiserDto::Shadow { max_sweeps: None });
    run("final 300/6 hex, optimiser default", &mut pipe, &dh, 1);
}
