//! End-to-end CLI runs: squama → 3MF/STL → validate, calibration, projects.

use std::path::PathBuf;
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ad3d"))
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ad3d-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(c: &mut Command) -> (i32, String) {
    let o = c.output().unwrap();
    let s = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    (o.status.code().unwrap_or(-1), s)
}

#[test]
fn squama_tiles_to_bed_and_validates_3mf_and_stl() {
    let d = tmp("squama");
    let (code, out) = run(bin().args(["squama", "--synthetic", "--width", "240", "--cell", "8", "--format", "3mf,stl", "--out"]).arg(&d));
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("tiles: "), "{out}");
    let three = d.join("squama.3mf");
    assert!(three.exists());
    let (code, out) = run(bin().arg("validate").arg(&three));
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("unit millimeter"), "{out}");
    let stls: Vec<_> = std::fs::read_dir(&d).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "stl")).collect();
    assert!(stls.len() > 1, "240 mm plate must be tiled on the CR-6 SE");
    for s in stls {
        let (code, out) = run(bin().arg("validate").arg(&s));
        assert_eq!(code, 0, "{}: {out}", s.display());
    }
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn single_file_out_and_refusals() {
    let d = tmp("single");
    let f = d.join("one.stl");
    let (code, out) = run(bin().args(["squama", "--synthetic", "--width", "60", "--tiles", "none", "--orientation", "flat", "--out"]).arg(&f));
    assert_eq!(code, 0, "{out}");
    assert!(f.exists());
    // M1 options owned by unmerged modules are refused, not ignored.
    for extra in [&["--layout", "hex"][..], &["--light-point", "0,0,400"], &["--optimise", "shadow"], &["--keys", "pegs"], &["--stand"]] {
        let (code, out) = run(bin().args(["squama", "--synthetic", "--width", "60", "--out"]).arg(&f).args(extra));
        assert_eq!(code, 2, "{extra:?}: {out}");
    }
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn calibration_writes_tile_and_key() {
    let d = tmp("cal");
    let (code, out) = run(bin().args(["calibration", "--format", "stl,3mf", "--out"]).arg(&d));
    assert_eq!(code, 0, "{out}");
    for f in ["calibration.stl", "calibration.3mf", "calibration.key.json"] {
        assert!(d.join(f).exists(), "{f}");
    }
    let (code, out) = run(bin().arg("validate").arg(d.join("calibration.3mf")));
    assert_eq!(code, 0, "{out}");
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn project_save_open_export() {
    let d = tmp("proj");
    let p = d.join("job.ad3d");
    let (code, out) = run(bin().args(["project", "save", "--synthetic", "--width", "80", "--gamma", "1.2", "--out"]).arg(&p));
    assert_eq!(code, 0, "{out}");
    let (code, out) = run(bin().args(["project", "open", "--json"]).arg(&p));
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("\"schemaVersion\": 1") && out.contains("\"gamma\": 1.2") && out.contains("thumbnail: yes"), "{out}");
    let (code, out) = run(bin().args(["project", "open"]).arg(&p).arg("--out").arg(d.join("job.3mf")));
    assert_eq!(code, 0, "{out}");
    assert!(d.join("job.3mf").exists());
    // Not a project.
    let (code, _) = run(bin().args(["project", "open"]).arg(d.join("job.3mf")));
    assert_eq!(code, 2);
    std::fs::remove_dir_all(&d).ok();
}
