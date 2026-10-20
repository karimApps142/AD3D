//! M1 end-to-end proof through the app's command layer (verify round 1).
//!
//! Drives `SquamaState` exactly as the Tauri commands `squama_image_open`,
//! `generate_squama`, `squama_target`, `squama_tile_plan` and `export_squama` do
//! (they wrap these methods in `spawn_blocking`), with the real photo
//! `tests/fixtures/mona-lisa.jpg`, and writes every part to `docs/m1/out/app/<case>/`.
//!
//! Ignored by default (it writes into the repo); run with
//! `cargo test -p ad3d-app e2e_m1 -- --ignored --nocapture`.

use std::path::PathBuf;

use ad3d_core::CancelToken;
use ad3d_core::io::stl::read_stl;
use ad3d_core::mesh::validate::{ValidateOpts, validate};
use serde_json::{Value, json};

use crate::squama_cmds::{ExportEvent, SquamaExportDto, SquamaParamsM1, SquamaState};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn photo() -> String {
    repo().join("tests/fixtures/mona-lisa.jpg").display().to_string()
}

/// LuBan default (300 / 6 / 5° / 10° / 3) on the photo, as the UI sends it.
fn base_params(extra: Value) -> Value {
    let mut p = json!({
        "jobId": 1, "widthMm": 300.0, "cellMm": 6.0, "lightAzimuthDeg": 180.0,
        "lightElevationDeg": 5.0, "maxTiltDeg": 10.0, "baseMm": 3.0, "image": photo()
    });
    p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    p
}

fn case(name: &str, params: Value, export: Value) -> (String, SquamaExportDto) {
    let dir = repo().join("docs/m1/out/app").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut e = json!({
        "params": params, "outDir": dir.display().to_string(), "baseName": name,
        "formats": ["stl", "3mf"], "orientation": "standing-auto"
    });
    e.as_object_mut().unwrap().extend(export.as_object().unwrap().clone());
    (name.to_owned(), serde_json::from_value(e).expect("export DTO parses"))
}

#[test]
#[ignore = "writes docs/m1/out/app; run explicitly"]
fn e2e_m1_app_exports() {
    let st = SquamaState::default();
    let info = st.image_info(&photo()).expect("squama_image_open");
    println!("image: {}x{} (working {}x{})", info.width_px, info.height_px, info.working_width_px, info.working_height_px);

    let cr6 = json!({ "widthMm": 160.0 });
    let cases = vec![
        case("luban-default", base_params(json!({})), json!({ "orientation": "flat" })),
        case("cr6-onepiece", base_params(cr6.clone()), json!({})),
        case(
            "tiled-300x378",
            base_params(json!({ "heightMm": 378.0 })),
            json!({ "bedMm": [235.0, 235.0, 250.0], "marginMm": 8.0, "keys": "pegs", "stand": true }),
        ),
        case("hex", base_params(json!({ "widthMm": 160.0, "layout": { "kind": "hex", "orientation": "pointy" } })), json!({})),
        case("point", base_params(json!({ "widthMm": 160.0, "light": { "kind": "point", "posMm": [-400.0, 120.0, 80.0] } })), json!({})),
        case("optimised", base_params(json!({ "widthMm": 160.0, "optimiser": { "kind": "shadow" } })), json!({})),
        case("calibration", base_params(cr6), json!({ "calibrationTile": true })),
    ];

    let mut failures = Vec::new();
    for (name, e) in &cases {
        // Live preview, then final, as the viewport does (generate_squama).
        let mut p: SquamaParamsM1 = e.params.clone();
        for q in ["preview", "final"] {
            p.quality = Some(serde_json::from_value(json!(q)).unwrap());
            let (buf, ji) = st.generate(&p, &CancelToken::new()).expect("generate_squama");
            println!(
                "[{name}] generate {q}: {} bytes, {} tris, {:.1} ms (target {:.1} solve {:.1} mesh {:.1}), validated {:?}, score {:?} before {:?}, sweeps {:?}, warnings {:?}",
                buf.len(),
                ji.triangles,
                ji.gen_ms,
                ji.timings.target_ms,
                ji.timings.solve_ms,
                ji.timings.mesh_ms,
                ji.validated,
                ji.score.map(|s| s.pearson),
                ji.score_before.map(|s| s.pearson),
                ji.optimiser_sweeps,
                ji.warnings.iter().map(|w| w.code.as_str()).collect::<Vec<_>>()
            );
            if ji.validated == Some(false) {
                failures.push(format!("{name}: final generate failed validate"));
            }
        }
        st.target(&p).expect("squama_target");
        match st.tile_plan(e, &CancelToken::new()) {
            Ok(tp) => println!("[{name}] tile plan cols {:?} rows {:?} turns {:?}", tp.cols, tp.rows, tp.quarter_turns),
            Err(err) => println!("[{name}] tile plan error: {err}"),
        }
        let mut events = 0usize;
        let r = match st.export(e, &CancelToken::new(), &mut |ev: ExportEvent| {
            events += 1;
            if let ExportEvent::Warning { code, message } = ev {
                println!("[{name}] export warning {code}: {message}");
            }
        }) {
            Ok(r) => r,
            Err(err) => {
                failures.push(format!("{name}: export failed: {err}"));
                continue;
            }
        };
        println!(
            "[{name}] export {:.0} ms, {} parts, {} events, tiles {:?}, beds {}, optimise {:?}, brim {:?}",
            r.elapsed_ms,
            r.parts.len(),
            events,
            r.tiles.as_ref().map(|t| (t.cols.clone(), t.rows.clone())),
            r.beds,
            r.optimise.as_ref().map(|o| (o.before.pearson, o.after.pearson)),
            r.brim
        );
        for part in &r.parts {
            println!(
                "[{name}]   part {} {} tris size {:?} bed {} fits {:?} ledge {:?} h/footprint {:.1} files {:?}",
                part.name, part.triangles, part.size_mm, part.bed + 1, part.fits_bed, part.max_downward_ledge_mm, part.height_ratio, part.files
            );
            if let Some(stl) = part.files.iter().find(|f| f.ends_with(".stl")) {
                let m = read_stl(std::path::Path::new(stl)).unwrap();
                let opts = |c| ValidateOpts { soup: true, expect_components: c, check_self_intersection: true, ..Default::default() };
                let mut rep = validate(&m, &opts(1));
                // Splints are several closed shells in one part.
                if part.name.contains("splint") && rep.written.components > 1 {
                    println!("[{name}]   {} has {} shells", part.name, rep.written.components);
                    rep = validate(&m, &opts(rep.written.components));
                }
                if !rep.ok {
                    failures.push(format!("{name}/{}: {}", part.name, rep.failures.join("; ")));
                }
            }
        }
        for f in &r.files {
            println!("[{name}]   file {f}");
        }
    }
    assert!(failures.is_empty(), "failures:\n{}", failures.join("\n"));
}

/// Dumps the core's per-facet target and simulated appearance (§15.1, what the match score
/// measures) for the CR-6 photo plate, with and without the optimiser, as PGM images, to compare
/// against the viewport's analytic shadow preview (docs/m1/preview-*.png).
#[test]
#[ignore = "writes docs/m1/sim-*.pgm; run explicitly"]
fn e2e_m1_dump_core_sim() {
    let st = SquamaState::default();
    for (tag, extra) in [("plain", json!({})), ("opt", json!({ "optimiser": { "kind": "shadow", "maxSweeps": 8 } }))] {
        let mut v = base_params(json!({ "widthMm": 160.0, "quality": "final" }));
        v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let p: SquamaParamsM1 = serde_json::from_value(v).unwrap();
        st.generate(&p, &CancelToken::new()).unwrap();
        let t = st.target(&p).unwrap();
        let rd = |o: usize| u32::from_le_bytes(t[o..o + 4].try_into().unwrap());
        let (nx, ny, flags) = (rd(4) as usize, rd(8) as usize, rd(12));
        assert_eq!(flags & 1, 1, "no simulated grid");
        let grid = |k: usize| -> Vec<f32> {
            (0..nx * ny).map(|i| f32::from_le_bytes(t[16 + 4 * (k * nx * ny + i)..20 + 4 * (k * nx * ny + i)].try_into().unwrap())).collect()
        };
        for (name, g) in [("target", grid(0)), ("sim", grid(1))] {
            let mx = g.iter().cloned().fold(1e-9f32, f32::max);
            let s = 12; // px per facet
            let mut px = Vec::with_capacity(nx * ny * s * s);
            for y in (0..ny * s).rev() {
                for x in 0..nx * s {
                    px.push(((g[(y / s) * nx + x / s] / mx).clamp(0.0, 1.0) * 255.0) as u8);
                }
            }
            let path = repo().join(format!("docs/m1/sim-{tag}-{name}.pgm"));
            let mut out = format!("P5 {} {} 255\n", nx * s, ny * s).into_bytes();
            out.extend(px);
            std::fs::write(&path, out).unwrap();
            println!("{tag} {name}: {nx}x{ny}, max {mx:.4}, wrote {}", path.display());
        }
    }
}

/// Writes the shadow-agreement fixture `tests/fixtures/mona-lisa-160-shadow.json`: the CR-6
/// Mona Lisa plate (160 mm, LuBan defaults, final quality, optimiser off and on) as per-cell
/// planes plus the core forward model's lit fraction per facet (§15.1, what the match score and
/// the optimiser see). `src/viewport/shadowAgreement.test.ts` traces the same planes with the
/// viewport's `traceShadow` and checks that both shadow models agree cell by cell.
#[test]
#[ignore = "writes tests/fixtures/mona-lisa-160-shadow.json; run explicitly"]
fn e2e_m1_dump_shadow_fixture() {
    use ad3d_core::image::LinearLuma;
    use ad3d_core::sim::{facet_lights, line_set, prepare_response, shade_lines};
    use ad3d_core::squama::SquamaParamsDto;
    use ad3d_core::squama::build_partition;
    use ad3d_core::squama::pipeline::squama_preview;
    let img = LinearLuma::load(std::path::Path::new(&photo())).unwrap();
    let mut cases = Vec::new();
    for (tag, extra) in [("plain", json!({})), ("opt", json!({ "optimiser": { "kind": "shadow", "maxSweeps": 8 } }))] {
        let mut v = base_params(json!({ "widthMm": 160.0, "quality": "final" }));
        v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let d: SquamaParamsDto = serde_json::from_value(v).unwrap();
        let job = d.job().unwrap();
        let out = squama_preview(&job, Some(&img), &CancelToken::new()).unwrap();
        let l = out.layout;
        let p = build_partition(&l, &[]).unwrap();
        let ls = line_set(&p, &out.facets, &job.light, 0).unwrap();
        let centres: Vec<_> = out.facets.iter().map(|f| (f.pivot, f.h)).collect();
        let lights = facet_lights(&p, &centres, &job.light).unwrap();
        let curve = prepare_response(&p, &job.light, &job.response, job.limits.base_mm).unwrap();
        let sh = shade_lines(&ls, &out.facets, &lights, &curve);
        let planes: Vec<[f64; 3]> = out
            .facets
            .iter()
            .map(|f| [(f.h * 1e6).round() / 1e6, (f.grad.x * 1e9).round() / 1e9, (f.grad.y * 1e9).round() / 1e9])
            .collect();
        let lit: Vec<f64> = sh.lit_fraction.iter().map(|x| (x * 1e6).round() / 1e6).collect();
        // What the app's Appearance view shows (pipeline::appearance → squama_target).
        let sim: Vec<f64> = out.appearance.as_ref().unwrap().sim.iter().map(|&x| (x as f64 * 1e6).round() / 1e6).collect();
        cases.push(json!({
            "tag": tag, "nx": l.nx, "ny": l.ny, "cellMm": l.cell_mm, "wMm": l.w_mm, "hMm": l.h_mm,
            "lightAzimuthDeg": 180.0, "lightElevationDeg": 5.0, "planes": planes, "litFraction": lit, "sim": sim,
        }));
        println!("{tag}: {} x {} facets", l.nx, l.ny);
    }
    let path = repo().join("tests/fixtures/mona-lisa-160-shadow.json");
    std::fs::write(&path, serde_json::to_string(&json!({ "cases": cases })).unwrap()).unwrap();
    println!("wrote {}", path.display());
}
