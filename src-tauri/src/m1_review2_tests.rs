//! Verify round 2 (UX and performance review): times the app's command layer the way the
//! UI drives it, with the real photo, and checks the error messages for bad inputs.
//!
//! Timing table (release, prints only):
//!   cargo test -p ad3d-app --release m1_review2 -- --ignored --nocapture --test-threads=1
//! The non-ignored tests are regressions for the problems found in this round.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use ad3d_core::CancelToken;
use serde_json::{Value, json};

use crate::squama_cmds::{SquamaExportDto, SquamaParamsM1, SquamaState};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn photo() -> String {
    repo().join("tests/fixtures/mona-lisa.jpg").display().to_string()
}

fn params(extra: Value) -> SquamaParamsM1 {
    let mut p = json!({
        "jobId": 1, "widthMm": 300.0, "cellMm": 6.0, "lightAzimuthDeg": 180.0,
        "lightElevationDeg": 5.0, "maxTiltDeg": 10.0, "baseMm": 3.0, "image": photo(),
        "optimiser": { "kind": "shadow", "maxSweeps": 8 }
    });
    p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    serde_json::from_value(p).unwrap()
}

fn export_dto(p: &SquamaParamsM1) -> SquamaExportDto {
    serde_json::from_value(json!({
        "params": p, "outDir": "/plan", "baseName": "plate", "formats": ["3mf"],
        "orientation": "standing-auto", "bedMm": [235.0, 235.0, 250.0], "marginMm": 8.0,
        "bedMotion": "y", "keys": "auto"
    }))
    .unwrap()
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

/// One drag: `n` ticks of `key` from `from` by `step`, at preview quality, as the slider sends them.
fn drag(st: &SquamaState, base: &SquamaParamsM1, key: &str, from: f64, step: f64, n: usize) -> Vec<f64> {
    let mut v = serde_json::to_value(base).unwrap();
    v["quality"] = json!("preview");
    (0..n)
        .map(|i| {
            v[key] = json!(from + step * i as f64);
            v["jobId"] = json!(i as u32 + 10);
            let p: SquamaParamsM1 = serde_json::from_value(v.clone()).unwrap();
            let t = Instant::now();
            st.generate(&p, &CancelToken::new()).expect("preview");
            ms(t)
        })
        .collect()
}

fn summary(label: &str, mut xs: Vec<f64>) {
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p = |q: f64| xs[((xs.len() - 1) as f64 * q).round() as usize];
    println!("{label:<58} median {:6.1}  p90 {:6.1}  max {:6.1} ms  (n={})", p(0.5), p(0.9), p(1.0), xs.len());
}

#[test]
#[ignore = "timing table; run in release with --nocapture"]
fn m1_review2_latency_table() {
    let st = Arc::new(SquamaState::default());
    let t = Instant::now();
    st.image_info(&photo()).unwrap();
    println!("image open (decode + 2048 working): {:.1} ms", ms(t));

    for (label, extra) in [
        ("300 mm / 6 mm (LuBan default, 50x75)", json!({})),
        ("300 mm / 3 mm (100x150)", json!({ "cellMm": 3.0 })),
        ("160 mm / 6 mm (CR-6 one piece)", json!({ "widthMm": 160.0 })),
        ("300 mm / 6 mm hex", json!({ "layout": { "kind": "hex", "orientation": "pointy" } })),
        ("300 mm / 6 mm point lamp", json!({ "light": { "kind": "point", "posMm": [-600.0, 189.0, 60.0] } })),
    ] {
        let base = params(extra);
        drag(&st, &base, "maxTiltDeg", 9.0, 0.1, 3); // warm caches
        summary(&format!("drag maxTilt  {label}"), drag(&st, &base, "maxTiltDeg", 10.0, 0.1, 30));
        summary(&format!("drag width    {label}"), drag(&st, &base, "widthMm", base.width_mm, 1.0, 30));
        let mut f = base.clone();
        f.quality = Some(serde_json::from_value(json!("final")).unwrap());
        let t = Instant::now();
        let (_, info) = st.generate(&f, &CancelToken::new()).unwrap();
        println!(
            "final (release)  {label:<44} {:7.1} ms  sweeps {:?}  score {:?}",
            ms(t),
            info.optimiser_sweeps,
            info.score.map(|s| s.pearson)
        );
        let t = Instant::now();
        st.target(&f).unwrap();
        println!("squama_target    {label:<44} {:7.1} ms", ms(t));
        let t = Instant::now();
        let plan = st.tile_plan(&export_dto(&f), &CancelToken::new()).unwrap();
        println!("squama_tile_plan {label:<44} {:7.1} ms  cols {:?} rows {:?}", ms(t), plan.cols, plan.rows);
    }

    // The export panel re-plans 150 ms after any document change (a drag pause included), on the
    // blocking pool, uncancellable, while the next drag ticks run.
    let base = params(json!({ "cellMm": 3.0 }));
    let quiet = drag(&st, &base, "maxTiltDeg", 10.0, 0.1, 30);
    let st2 = st.clone();
    let planner = std::thread::spawn(move || {
        let mut f = params(json!({ "cellMm": 3.0 }));
        let mut n = 0;
        let t = Instant::now();
        for i in 0..4 {
            f.max_tilt_deg = 11.0 + i as f64 * 0.1;
            st2.tile_plan(&export_dto(&f), &CancelToken::new()).unwrap();
            n += 1;
        }
        (n, ms(t))
    });
    let busy = drag(&st, &base, "maxTiltDeg", 10.0, 0.1, 30);
    let (n, total) = planner.join().unwrap();
    summary("drag maxTilt 300/3, quiet", quiet);
    summary(&format!("drag maxTilt 300/3, during {n} tile plans ({total:.0} ms)"), busy);
}

fn open_err(path: &str) -> String {
    SquamaState::default().image_info(path).expect_err("must fail")
}

/// The messages a person sees in the toast (`Could not open <name>: <message>`).
#[test]
fn bad_photo_messages_are_readable() {
    let dir = std::env::temp_dir().join(format!("ad3d-m1r2-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let text = dir.join("notes.png");
    std::fs::write(&text, b"this is not an image").unwrap();
    let trunc = dir.join("cut.jpg");
    let full = std::fs::read(photo()).unwrap();
    std::fs::write(&trunc, &full[..full.len() / 3]).unwrap();
    let gif = dir.join("anim.gif");
    std::fs::write(&gif, b"GIF89a\x01\x00\x01\x00\x00\x00\x00;").unwrap();
    let cases = [
        ("missing", dir.join("nope.jpg").display().to_string()),
        ("text as png", text.display().to_string()),
        ("gif", gif.display().to_string()),
    ];
    let mut bad = Vec::new();
    for (label, p) in cases {
        let m = open_err(&p);
        println!("{label:<16} -> {m}");
        // A message must not be a bare decoder/IO string: it should say what to do or what the file is.
        let raw = m.starts_with("No such file") || m.contains("os error") && !m.contains(".jpg") || m.len() < 12;
        if raw {
            bad.push(format!("{label}: {m:?}"));
        }
    }
    assert!(bad.is_empty(), "unhelpful messages: {bad:#?}");
}

/// A JPEG cut off at a third (an interrupted download or an iCloud placeholder) must not open
/// silently: today it decodes with the missing two thirds filled grey and builds a plate from it.
#[test]
fn truncated_photo_is_reported() {
    let dir = std::env::temp_dir().join(format!("ad3d-m1r2-trunc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let trunc = dir.join("cut.jpg");
    let full = std::fs::read(photo()).unwrap();
    std::fs::write(&trunc, &full[..full.len() / 3]).unwrap();
    let r = SquamaState::default().image_info(&trunc.display().to_string());
    assert!(r.is_err(), "truncated JPEG opened without an error or warning: {r:?}");
}

/// Huge photos: time to open and the message past the decoder limit. Needs the files made by
///   sips -z 9000 7000 tests/fixtures/mona-lisa.jpg --out $AD3D_BIG
///   sips -z 12000 10000 tests/fixtures/mona-lisa.jpg --out $AD3D_HUGE
#[test]
#[ignore = "needs AD3D_BIG / AD3D_HUGE photos"]
fn m1_review2_huge_photos() {
    for var in ["AD3D_BIG", "AD3D_HUGE"] {
        let Ok(p) = std::env::var(var) else { continue };
        let st = SquamaState::default();
        let t = Instant::now();
        let r = st.image_info(&p);
        println!("{var}: {:.0} ms -> {:?}", ms(t), r.as_ref().map(|i| (i.width_px, i.height_px)));
        if let Err(e) = &r {
            println!("   message: {e}");
            continue;
        }
        let mut f = params(json!({ "image": p, "quality": "preview" }));
        let t = Instant::now();
        st.generate(&f, &CancelToken::new()).unwrap();
        println!("   first preview after open: {:.0} ms", ms(t));
        f.quality = Some(serde_json::from_value(json!("final")).unwrap());
        let t = Instant::now();
        st.generate(&f, &CancelToken::new()).unwrap();
        println!("   first final after open: {:.0} ms", ms(t));
        let mut c = params(json!({ "image": p, "quality": "preview" }));
        let mut ticks = Vec::new();
        for i in 0..10 {
            c.image_opts = serde_json::from_value(json!({ "crop": [0.02 + i as f64 * 0.005, 0.02, 0.98, 0.98] })).unwrap();
            let t = Instant::now();
            let (_, ji) = st.generate(&c, &CancelToken::new()).unwrap();
            ticks.push(ms(t));
            if i == 5 {
                println!("   tick timings {:?}", ji.timings);
            }
        }
        summary("   crop ticks (CropBox commits on release only)", ticks);
        let mut ticks = Vec::new();
        for i in 0..10 {
            c.image_opts = serde_json::from_value(json!({ "gamma": 1.0 + i as f64 * 0.02, "contrast": 1.1 })).unwrap();
            let t = Instant::now();
            st.generate(&c, &CancelToken::new()).unwrap();
            ticks.push(ms(t));
        }
        summary("   gamma drag ticks", ticks);
    }
}

#[test]
#[ignore = "timing; release"]
fn m1_review2_heavy_finals() {
    let st = SquamaState::default();
    for (label, extra) in [
        ("300/3 point lamp, 8 sweeps", json!({ "cellMm": 3.0, "light": { "kind": "point", "posMm": [-600.0, 189.0, 60.0] } })),
        ("300/3 point lamp, 32 sweeps", json!({ "cellMm": 3.0, "light": { "kind": "point", "posMm": [-600.0, 189.0, 60.0] }, "optimiser": { "kind": "shadow", "maxSweeps": 32 } })),
        ("300/2 directional, 8 sweeps", json!({ "cellMm": 2.0 })),
    ] {
        let mut f = params(extra);
        f.quality = Some(serde_json::from_value(json!("final")).unwrap());
        let t = Instant::now();
        let (_, ji) = st.generate(&f, &CancelToken::new()).unwrap();
        println!("final {label:<34} {:7.0} ms  {}x{} sweeps {:?} timings {:?}", ms(t), ji.layout.nx, ji.layout.ny, ji.optimiser_sweeps, ji.timings);
        let t = Instant::now();
        st.tile_plan(&export_dto(&f), &CancelToken::new()).unwrap();
        println!("tile plan {label:<30} {:7.0} ms", ms(t));
    }
}
