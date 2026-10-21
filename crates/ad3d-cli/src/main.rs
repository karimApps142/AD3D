//! `ad3d`: headless AD3D generators, exports, projects and mesh checks.
//!
//! Units are millimetres; angles on the command line are degrees.

mod adapter;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use ad3d_core::CancelToken;
use ad3d_core::image::LinearLuma;
use ad3d_core::io::sink::{ExportFormat, ExportPart, safe_file_name};
use ad3d_core::io::stl::{read_stl, write_stl_cancellable};
use ad3d_core::io::threemf::{Meta, Part3mf, read_3mf, write_3mf};
use ad3d_core::mesh::Mesh;
use ad3d_core::mesh::validate::{ValidateOpts, validate};
use ad3d_core::profiles::{Bed, PrinterProfile, custom_printer, default_printer, parse_dims, printer_by_id, printers};
use ad3d_core::project::thumbnail::render_relief_png;
use ad3d_core::project::{ExportSettings, ImageOptsDto, KeysDto, OrientationDto, Project, TilesDto};
use ad3d_core::squama::calibration::{CalibrationSpec, calibration_tile};
use ad3d_core::squama::{GenOptions, SquamaParams};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde_json::{Value, json};

#[derive(Parser)]
#[command(name = "ad3d", version, about = "AD3D generators, exports and mesh checks (mm, degrees)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum LayoutArg {
    Square,
    Hex,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum HexOrient {
    Pointy,
    Flat,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OptimiseArg {
    None,
    Shadow,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum KeysArg {
    None,
    Pegs,
    /// Pegs when the plate is tiled and the base leaves room (default).
    Auto,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum FormatArg {
    Stl,
    #[value(name = "3mf")]
    ThreeMf,
}

impl From<FormatArg> for ExportFormat {
    fn from(f: FormatArg) -> Self {
        match f {
            FormatArg::Stl => ExportFormat::Stl,
            FormatArg::ThreeMf => ExportFormat::ThreeMf,
        }
    }
}

/// Printer / bed selection shared by every exporting command.
#[derive(Args, Clone)]
struct BedArgs {
    /// Printer preset id (see `ad3d profiles`); default Creality CR-6 SE.
    #[arg(long)]
    printer: Option<String>,
    /// Build volume override, `XxYxZ` mm (e.g. 235x235x250).
    #[arg(long)]
    bed: Option<String>,
    /// Clear margin kept around parts on the bed, mm.
    #[arg(long)]
    margin: Option<f64>,
}

/// Output selection shared by every exporting command.
#[derive(Args, Clone)]
struct OutArgs {
    /// Output: a file (`x.stl`, `x.3mf`) or a directory (files named after the job).
    #[arg(long)]
    out: PathBuf,
    /// Formats to write (repeat or comma-separate). Default: from `--out`'s extension, else 3MF.
    #[arg(long = "format", value_enum, value_delimiter = ',')]
    format: Vec<FormatArg>,
    /// Also run the triangle-triangle self-intersection stage of validate.
    #[arg(long)]
    strict: bool,
}

/// Every Squama option (design §12.1 params + `SquamaExport`).
#[derive(Args, Clone)]
struct SquamaArgs {
    /// Start from a params JSON file (§12.1 DTO); flags below override its fields.
    #[arg(long)]
    params: Option<PathBuf>,
    /// Grayscale or colour image (EXIF orientation applied, linearised).
    #[arg(long, conflicts_with = "synthetic")]
    image: Option<PathBuf>,
    /// Use the built-in test target (default when no image is given).
    #[arg(long)]
    synthetic: bool,
    #[arg(long)]
    width: Option<f64>,
    /// Optional plate height; the image is centre-cropped to fill.
    #[arg(long)]
    height: Option<f64>,
    #[arg(long)]
    cell: Option<f64>,
    #[arg(long, value_enum)]
    layout: Option<LayoutArg>,
    #[arg(long = "hex-orientation", value_enum, default_value_t = HexOrient::Pointy)]
    hex_orientation: HexOrient,
    /// Direction the light comes from: 180 = Left, 0 = Right, 90 = Top, 270 = Bottom.
    #[arg(long)]
    azimuth: Option<f64>,
    #[arg(long)]
    elevation: Option<f64>,
    /// Point light position `x,y,z` in plate mm (overrides azimuth/elevation).
    #[arg(long = "light-point")]
    light_point: Option<String>,
    #[arg(long = "max-tilt")]
    max_tilt: Option<f64>,
    #[arg(long)]
    base: Option<f64>,
    #[arg(long = "max-relief")]
    max_relief: Option<f64>,
    #[arg(long, value_enum)]
    optimise: Option<OptimiseArg>,
    #[arg(long = "max-sweeps")]
    max_sweeps: Option<u32>,
    /// Lambert ambient term 0..1.
    #[arg(long)]
    ambient: Option<f64>,
    /// Measured response JSON (`{tiltDeg:[..], brightness:[..]}`, from calibration).
    #[arg(long)]
    response: Option<PathBuf>,
    /// Image crop `x0,y0,x1,y1` (normalised, y down).
    #[arg(long)]
    crop: Option<String>,
    #[arg(long)]
    gamma: Option<f64>,
    #[arg(long)]
    contrast: Option<f64>,
    #[arg(long)]
    brightness: Option<f64>,
    #[arg(long)]
    invert: bool,
    /// Auto levels percentiles `lo,hi` (e.g. 0.01,0.99).
    #[arg(long = "auto-levels")]
    auto_levels: Option<String>,
    /// standing (auto quarter turn), standing-q0..q3, or flat.
    #[arg(long)]
    orientation: Option<String>,
    /// Tiling: auto (fit the bed), none, or NxM.
    #[arg(long)]
    tiles: Option<String>,
    #[arg(long, value_enum)]
    keys: Option<KeysArg>,
    /// Add a stand part.
    #[arg(long)]
    stand: bool,
    /// Add the calibration tile as an extra part.
    #[arg(long = "calibration-tile")]
    calibration_tile: bool,
    #[command(flatten)]
    bed: BedArgs,
}

#[derive(Subcommand)]
enum Cmd {
    /// Generate a Squama shadow plate (tiled to the bed when needed) and export it.
    Squama {
        #[command(flatten)]
        args: SquamaArgs,
        #[command(flatten)]
        out: OutArgs,
        /// Also save the job as an .ad3d project.
        #[arg(long = "save-project")]
        save_project: Option<PathBuf>,
    },
    /// Generate the calibration tile (grey ramp + light-side marker) and its key.
    Calibration {
        #[arg(long, default_value_t = 6.0)]
        cell: f64,
        #[arg(long, default_value_t = 180.0)]
        azimuth: f64,
        #[arg(long, default_value_t = 5.0)]
        elevation: f64,
        #[arg(long = "max-tilt", default_value_t = 10.0)]
        max_tilt: f64,
        #[arg(long, default_value_t = 3.0)]
        base: f64,
        #[arg(long, default_value_t = 12)]
        steps: u32,
        /// Largest tile `WxH` mm.
        #[arg(long = "max-size", default_value = "120x90")]
        max_size: String,
        /// standing (the tile's own best turn), standing-q0..q3, or flat.
        #[arg(long, default_value = "standing")]
        orientation: String,
        /// Quarter turns the plate will print with (its export report's
        /// `standing-qN`): the tile then stacks its layers the same way relative to
        /// the light whenever its ledges allow, so the measured response transfers.
        #[arg(long = "plate-turns", value_parser = clap::value_parser!(u32).range(0..4))]
        plate_turns: Option<u32>,
        #[command(flatten)]
        bed: BedArgs,
        #[command(flatten)]
        out: OutArgs,
    },
    /// Validate a binary/ASCII STL or a 3MF (every object). Exit code 1 on failure.
    Validate {
        file: PathBuf,
        /// Minimum distinct-vertex distance gate in mm (0 disables).
        #[arg(long, default_value_t = 0.05)]
        min_distance: f64,
        /// Expected number of closed shells for every object. Default: 1, except
        /// for multi-shell parts (3MF objects or STL files named `*key_splints*`),
        /// which may have any number N of shells with Euler number 2·N.
        #[arg(long)]
        components: Option<usize>,
        /// Allow any number N ≥ 1 of closed genus-0 shells (Euler = 2·N) for every object.
        #[arg(long)]
        multi_shell: bool,
        /// Smallest allowed triangle altitude in mm (0 disables the sliver gate).
        #[arg(long, default_value_t = ad3d_core::mesh::validate::DEFAULT_MIN_ALTITUDE_MM)]
        min_altitude: f64,
        /// Also run the triangle-triangle self-intersection stage.
        #[arg(long)]
        strict: bool,
    },
    /// Open or save .ad3d project files.
    Project {
        #[command(subcommand)]
        cmd: ProjectCmd,
    },
    /// List machine profiles.
    Profiles,
}

#[derive(Subcommand)]
enum ProjectCmd {
    /// Print a project's summary; with --out, regenerate and export it.
    Open {
        file: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long = "format", value_enum, value_delimiter = ',')]
        format: Vec<FormatArg>,
        #[arg(long)]
        strict: bool,
        /// Print project.json.
        #[arg(long)]
        json: bool,
    },
    /// Save a Squama job (same options as `squama`) as an .ad3d project.
    Save {
        #[command(flatten)]
        args: SquamaArgs,
        #[arg(long)]
        out: PathBuf,
    },
}

fn main() -> ExitCode {
    match real_main() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}

type R<T> = Result<T, Box<dyn std::error::Error>>;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

fn parse_list<const N: usize>(s: &str, what: &str) -> R<[f64; N]> {
    let v: Vec<f64> = s.split(',').map(|t| t.trim().parse::<f64>()).collect::<Result<_, _>>().map_err(|_| format!("{what}: expected {N} comma-separated numbers, got {s:?}"))?;
    v.try_into().map_err(|_| format!("{what}: expected {N} comma-separated numbers, got {s:?}").into())
}

fn printer_of(b: &BedArgs) -> R<PrinterProfile> {
    let mut p = match &b.printer {
        Some(id) => printer_by_id(id).ok_or_else(|| format!("unknown printer {id:?}; see `ad3d profiles`"))?,
        None => default_printer(),
    };
    if let Some(s) = &b.bed {
        let c = custom_printer(s)?;
        p.build_mm = c.build_mm;
        p.name = format!("{} (bed {})", p.name, s);
    }
    if let Some(m) = b.margin {
        if !(0.0..50.0).contains(&m) {
            return Err(format!("margin {m} mm out of range 0..50").into());
        }
        p.margin_mm = m;
    }
    Ok(p)
}

/// Builds the §12.1 params JSON, export settings and image path from flags.
fn squama_job(a: &SquamaArgs) -> R<(Value, ExportSettings, Option<PathBuf>, PrinterProfile)> {
    let mut p: Value = match &a.params {
        Some(f) => serde_json::from_slice(&std::fs::read(f).map_err(|e| format!("{}: {e}", f.display()))?)
            .map_err(|e| format!("{}: {e}", f.display()))?,
        None => json!({ "widthMm": 300.0, "cellMm": 6.0, "lightAzimuthDeg": 180.0, "lightElevationDeg": 5.0,
                        "maxTiltDeg": 10.0, "baseMm": 3.0, "image": "synthetic" }),
    };
    let o = p.as_object_mut().ok_or("params JSON must be an object")?;
    // A params file may carry `export` (SquamaExport fields) alongside.
    let mut export: ExportSettings = match o.remove("export") {
        Some(e) => serde_json::from_value(e).map_err(|e| format!("params export: {e}"))?,
        None => ExportSettings::default(),
    };
    let mut set = |k: &str, v: Option<f64>| {
        if let Some(v) = v {
            o.insert(k.into(), json!(v));
        }
    };
    set("widthMm", a.width);
    set("heightMm", a.height);
    set("cellMm", a.cell);
    set("lightAzimuthDeg", a.azimuth);
    set("lightElevationDeg", a.elevation);
    set("maxTiltDeg", a.max_tilt);
    set("baseMm", a.base);
    set("maxReliefMm", a.max_relief);
    match a.layout {
        Some(LayoutArg::Square) => {
            o.insert("layout".into(), json!({ "kind": "square" }));
        }
        Some(LayoutArg::Hex) => {
            let or = if a.hex_orientation == HexOrient::Flat { "flat" } else { "pointy" };
            o.insert("layout".into(), json!({ "kind": "hex", "orientation": or }));
        }
        None => {}
    }
    if let Some(s) = &a.light_point {
        let pos = parse_list::<3>(s, "--light-point")?;
        o.insert("light".into(), json!({ "kind": "point", "posMm": pos }));
    }
    match a.optimise {
        Some(OptimiseArg::None) => {
            o.insert("optimiser".into(), json!({ "kind": "none" }));
        }
        Some(OptimiseArg::Shadow) => {
            let mut v = json!({ "kind": "shadow" });
            if let Some(n) = a.max_sweeps {
                v["maxSweeps"] = json!(n);
            }
            o.insert("optimiser".into(), v);
        }
        None => {}
    }
    if let Some(f) = &a.response {
        let mut v: Value = serde_json::from_slice(&std::fs::read(f).map_err(|e| format!("{}: {e}", f.display()))?)
            .map_err(|e| format!("{}: {e}", f.display()))?;
        if v.get("kind").is_none() {
            v["kind"] = json!("measured");
        }
        o.insert("response".into(), v);
    } else if let Some(am) = a.ambient {
        o.insert("response".into(), json!({ "kind": "lambert", "ambient": am }));
    }
    // Image options merge into any from the params file.
    let mut io: ImageOptsDto = match o.remove("imageOpts") {
        Some(Value::Null) | None => ImageOptsDto::default(),
        Some(v) => serde_json::from_value(v).map_err(|e| format!("imageOpts: {e}"))?,
    };
    if let Some(s) = &a.crop {
        io.crop = Some(parse_list::<4>(s, "--crop")?);
    }
    if let Some(g) = a.gamma {
        io.gamma = g;
    }
    if let Some(c) = a.contrast {
        io.contrast = c;
    }
    if let Some(b) = a.brightness {
        io.brightness = b;
    }
    if a.invert {
        io.invert = true;
    }
    if let Some(s) = &a.auto_levels {
        io.auto_levels = Some(parse_list::<2>(s, "--auto-levels")?);
    }
    if !io.is_default() {
        o.insert("imageOpts".into(), serde_json::to_value(&io)?);
    }
    let image = match (&a.image, a.synthetic) {
        (Some(path), _) => {
            o.insert("image".into(), json!(path.display().to_string()));
            Some(path.clone())
        }
        (None, true) => {
            o.insert("image".into(), json!("synthetic"));
            None
        }
        (None, false) => match o.get("image").and_then(Value::as_str) {
            Some("synthetic") | None => {
                o.insert("image".into(), json!("synthetic"));
                None
            }
            Some(path) => Some(PathBuf::from(path)),
        },
    };
    if let Some(s) = &a.orientation {
        export.orientation = OrientationDto::parse(s)?;
    }
    if let Some(s) = &a.tiles {
        export.tiles = TilesDto::parse(s)?;
    }
    match a.keys {
        Some(KeysArg::Pegs) => export.keys = KeysDto::Pegs,
        Some(KeysArg::None) => export.keys = KeysDto::None,
        Some(KeysArg::Auto) => export.keys = KeysDto::Auto,
        None => {}
    }
    export.stand |= a.stand;
    export.calibration_tile |= a.calibration_tile;
    let printer = printer_of(&a.bed)?;
    if let Some(s) = &a.bed.bed {
        export.bed_mm = Some(parse_dims::<3>(s)?);
    }
    if let Some(m) = a.bed.margin {
        export.margin_mm = Some(m);
    }
    Ok((p, export, image, printer))
}

/// Where each format goes for a job named `stem`.
struct Outputs {
    dir: PathBuf,
    stem: String,
    formats: Vec<ExportFormat>,
}

fn outputs(out: &Path, formats: &[FormatArg], default_stem: &str) -> R<Outputs> {
    let ext = out.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
    let file_fmt = match ext.as_deref() {
        Some("stl") => Some(ExportFormat::Stl),
        Some("3mf") => Some(ExportFormat::ThreeMf),
        _ => None,
    };
    let mut fm: Vec<ExportFormat> = formats.iter().map(|&f| f.into()).collect();
    fm.dedup();
    match file_fmt {
        Some(f) => {
            if fm.is_empty() {
                fm.push(f);
            }
            let dir = out.parent().filter(|d| !d.as_os_str().is_empty()).map(Path::to_path_buf).unwrap_or_else(|| ".".into());
            let stem = out.file_stem().and_then(|s| s.to_str()).unwrap_or(default_stem).to_string();
            Ok(Outputs { dir, stem, formats: fm })
        }
        None => {
            if fm.is_empty() {
                fm.push(ExportFormat::ThreeMf);
            }
            Ok(Outputs { dir: out.into(), stem: default_stem.into(), formats: fm })
        }
    }
}

struct Written {
    path: PathBuf,
    bytes: u64,
}

/// Writes every part in every format. STL: one file per part (`stem.stl` when
/// there is a single part, else `stem_<part>.stl`). 3MF: with `beds` (the core's
/// placements, already in each bed's own coordinates) one file per bed —
/// `stem.3mf`, then `stem_bed2.3mf`, … — with no re-packing; without, one file
/// with the parts packed on the bed.
fn write_parts(
    o: &Outputs,
    parts: &[ExportPart],
    beds: Option<&[u32]>,
    meta: &Meta,
    bed: &Bed,
    thumb: Option<&[u8]>,
    cancel: &CancelToken,
) -> R<Vec<Written>> {
    std::fs::create_dir_all(&o.dir).map_err(|e| format!("{}: {e}", o.dir.display()))?;
    let stem = safe_file_name(&o.stem);
    let mut out = Vec::new();
    for f in &o.formats {
        match f {
            ExportFormat::Stl => {
                for p in parts {
                    let name = if parts.len() == 1 { format!("{stem}.stl") } else { format!("{stem}_{}.stl", safe_file_name(&p.name)) };
                    let path = o.dir.join(name);
                    write_stl_cancellable(&path, &p.mesh, &p.transform, cancel)?;
                    out.push(Written { bytes: std::fs::metadata(&path)?.len(), path });
                }
            }
            ExportFormat::ThreeMf => match beds {
                Some(beds) => {
                    let n = beds.iter().copied().max().map_or(0, |b| b + 1);
                    for b in 0..n {
                        let p3: Vec<Part3mf> = parts
                            .iter()
                            .zip(beds)
                            .filter(|(_, pb)| **pb == b)
                            .map(|(p, _)| Part3mf { name: &p.name, mesh: &p.mesh, transform: p.transform })
                            .collect();
                        let path = o.dir.join(if b == 0 { format!("{stem}.3mf") } else { format!("{stem}_bed{}.3mf", b + 1) });
                        let mut m = meta.clone();
                        if b > 0 {
                            m.title = m.title.map(|t| format!("{t} (bed {})", b + 1));
                        }
                        write_3mf(&path, &p3, &m, None, if b == 0 { thumb } else { None }, cancel)?;
                        let names: Vec<&str> = p3.iter().map(|p| p.name).collect();
                        println!("bed {}: {} -> {}", b + 1, names.join(", "), path.display());
                        out.push(Written { bytes: std::fs::metadata(&path)?.len(), path });
                    }
                }
                None => {
                    let path = o.dir.join(format!("{stem}.3mf"));
                    let p3: Vec<Part3mf> = parts.iter().map(|p| Part3mf { name: &p.name, mesh: &p.mesh, transform: p.transform }).collect();
                    let rep = write_3mf(&path, &p3, meta, Some(bed), thumb, cancel)?;
                    if rep.beds > 1 {
                        println!("note: {} parts need {} beds; extra beds are placed beside the first in {}", parts.len(), rep.beds, path.display());
                    }
                    for (pl, ob) in rep.placements.iter().zip(&rep.objects) {
                        if !pl.fits {
                            println!("warning: {} does not fit the bed even alone", ob.name);
                        }
                    }
                    out.push(Written { bytes: std::fs::metadata(&path)?.len(), path });
                }
            },
        }
    }
    Ok(out)
}

fn load_image(path: &Path) -> R<(LinearLuma, f64)> {
    let t = Instant::now();
    let l = LinearLuma::load(path)?;
    Ok((l, ms(t)))
}

/// Generates, validates and writes a Squama job. Returns the exit code.
fn run_squama(
    params: &Value,
    luma: Option<&LinearLuma>,
    export: &ExportSettings,
    printer: &PrinterProfile,
    outs: &Outputs,
    strict: bool,
    source: &str,
) -> R<(ExitCode, adapter::GenResult)> {
    let bed = export.bed(printer);
    let cancel = CancelToken::new();
    let t0 = Instant::now();
    let r = adapter::generate(params, luma, export, printer, &cancel)?;
    let gen_ms = ms(t0);
    let l = &r.plate.layout;
    let s = &r.plate.timings;
    println!("source: {source}");
    println!("printer: {} | bed {} x {} x {} mm, margin {} mm", printer.name, bed.x_mm, bed.y_mm, bed.z_mm, bed.margin_mm);
    println!("grid: {} x {} cells of {} mm -> plate {:.1} x {:.1} mm", l.nx, l.ny, l.cell_mm, l.w_mm, l.h_mm);
    println!(
        "plate mesh: {} vertices, {} triangles | whole-plate stage timings (ms): target {:.3} solve {:.3} quantise {:.3} mesh {:.3} | generate + export all parts {:.1}",
        r.plate.mesh.vertex_count(),
        r.plate.mesh.triangle_count(),
        s.target_ms,
        s.solve_ms,
        s.quantise_ms,
        s.mesh_ms,
        gen_ms
    );
    println!("tiles: {} x {} -> {} part(s)", r.tiles[0], r.tiles[1], r.parts.len());
    if let Some(o) = &r.report.optimise {
        println!(
            "optimiser: {} sweep(s), match r {:.3} -> {:.3}, rel. RMSE {:.3} -> {:.3}{}",
            o.sweeps,
            o.before.pearson,
            o.after.pearson,
            o.before.rmse_rel,
            o.after.rmse_rel,
            if o.timed_out { " (time budget)" } else { "" }
        );
    }
    for w in &r.warnings {
        println!("warning [{}]: {}", w.code, w.message);
    }
    if r.report.beds_used > 1 {
        println!("beds: {} (one 3MF per bed)", r.report.beds_used);
    }
    if let Some(b) = &r.report.brim {
        println!("print advice: {b}");
    }
    let mut ok = true;
    for gp in &r.parts {
        let t = Instant::now();
        let rep = validate(&gp.part.mesh, &ValidateOpts { check_self_intersection: strict, ..gp.validate.clone() });
        let b = gp.part.mesh.map_positions(|q| gp.part.transform.apply_f32(q)).bounds();
        let size = b.map(|b| b.size()).unwrap_or([0.0; 3]);
        let turns = match gp.quarter_turns {
            Some(k) => format!("standing, {k} quarter turn(s)"),
            None => "flat".into(),
        };
        let foot = (size[0].min(size[1]) as f64).max(1e-9);
        let ratio = size[2] as f64 / foot;
        let ledge = gp.max_ledge_mm.map(|l| format!(", max downward ledge {l:.2} mm")).unwrap_or_default();
        println!(
            "part {}: {} triangles, bed {}, print size {:.1} x {:.1} x {:.1} mm (footprint {:.1} mm wide, height/footprint {:.1}{}){}, {} | validate {} ({:.1} ms)",
            gp.part.name,
            gp.part.mesh.triangle_count(),
            gp.bed + 1,
            size[0],
            size[1],
            size[2],
            foot,
            ratio,
            if ratio > ad3d_core::squama::export::BRIM_RATIO { ", needs a brim" } else { "" },
            ledge,
            turns,
            if rep.ok { "OK" } else { "FAILED" },
            ms(t)
        );
        if !rep.ok {
            print!("{}", rep.summary());
            ok = false;
        }
    }
    let meta = Meta {
        title: Some(outs.stem.clone()),
        designer: None,
        description: Some("Squama shadow plate generated by AD3D".into()),
        creation_date: None,
        custom: vec![
            ("generator".into(), "squama".into()),
            ("params".into(), serde_json::to_string(params)?),
            ("export".into(), serde_json::to_string(export)?),
            ("printer".into(), printer.id.clone()),
        ],
    };
    let thumb = render_relief_png(&r.plate.mesh, r.job.params.light_azimuth, r.job.params.light_elevation, 256).ok();
    let parts: Vec<ExportPart> = r.parts.iter().map(|g| g.part.clone()).collect();
    let t = Instant::now();
    let beds: Vec<u32> = r.parts.iter().map(|g| g.bed).collect();
    let written = write_parts(outs, &parts, Some(&beds), &meta, &bed, thumb.as_deref(), &cancel)?;
    let w_ms = ms(t);
    for w in &written {
        println!("wrote {} ({} bytes)", w.path.display(), w.bytes);
    }
    if let Some(key) = &r.report.calibration_key {
        std::fs::create_dir_all(&outs.dir)?;
        let key_path = outs.dir.join(format!("{}.key.json", safe_file_name(&outs.stem)));
        std::fs::write(&key_path, serde_json::to_string_pretty(key)?)?;
        println!("wrote {} (calibration key)", key_path.display());
    }
    println!("write total {w_ms:.1} ms");
    Ok((if ok { ExitCode::SUCCESS } else { ExitCode::from(1) }, r))
}

/// Shell-count rule for one validated object.
#[derive(Clone, Copy)]
enum Shells {
    Exactly(usize),
    /// Any N ≥ 1 genus-0 shells (Euler = 2·N).
    Any,
}

/// The rule for an object named `name`: an explicit `--components`, `--multi-shell`,
/// or by part kind (key splints are one multi-shell part).
fn shells_for(name: &str, components: Option<usize>, multi_shell: bool) -> Shells {
    match components {
        Some(n) => Shells::Exactly(n),
        None if multi_shell || name.contains("key_splints") => Shells::Any,
        None => Shells::Exactly(1),
    }
}

fn validate_mesh(m: &Mesh, min_distance: f64, shells: Shells, min_altitude: f64, strict: bool) -> ad3d_core::mesh::validate::Report {
    let mut opts = ValidateOpts {
        soup: true,
        min_distance: if min_distance > 0.0 { Some(min_distance) } else { None },
        expect_components: 1,
        min_altitude: if min_altitude > 0.0 { Some(min_altitude) } else { None },
        check_self_intersection: strict,
        ..Default::default()
    };
    if let Shells::Exactly(n) = shells {
        opts.expect_components = n;
        return validate(m, &opts);
    }
    // Count the shells with a cheap pass, then require Euler = 2 × that count.
    let probe = validate(m, &ValidateOpts { check_self_intersection: false, min_distance: None, ..opts.clone() });
    opts.expect_components = probe.written.components.max(1);
    validate(m, &opts)
}

fn real_main() -> R<ExitCode> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Squama { args, out, save_project } => {
            let (params, export, image, printer) = squama_job(&args)?;
            let (luma, load_ms) = match &image {
                Some(p) => {
                    let (l, t) = load_image(p)?;
                    (Some(l), t)
                }
                None => (None, 0.0),
            };
            if image.is_some() {
                println!("image load {load_ms:.1} ms");
            }
            let outs = outputs(&out.out, &out.format, "squama")?;
            let src = image.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "synthetic".into());
            let (code, r) = run_squama(&params, luma.as_ref(), &export, &printer, &outs, out.strict, &src)?;
            if let Some(pp) = save_project {
                let bytes = match &image {
                    Some(p) => Some((p.to_string_lossy().to_string(), std::fs::read(p)?)),
                    None => None,
                };
                save_project_file(&pp, params, bytes, export, printer, Some(&r))?;
            }
            Ok(code)
        }
        Cmd::Calibration { cell, azimuth, elevation, max_tilt, base, steps, max_size, orientation, plate_turns, bed, out } => {
            let p = SquamaParams {
                width_mm: 100.0,
                height_mm: None,
                cell_mm: cell,
                light_azimuth: azimuth.to_radians(),
                light_elevation: elevation.to_radians(),
                max_tilt: max_tilt.to_radians(),
                base_mm: base,
            };
            let mut spec = CalibrationSpec::from_params(&p);
            spec.steps = steps;
            spec.max_size_mm = parse_dims::<2>(&max_size)?;
            let t = Instant::now();
            let gopts = GenOptions::default();
            let mut tile = calibration_tile(&spec, &gopts, &CancelToken::new())?;
            let gen_ms = ms(t);
            let printer = printer_of(&bed)?;
            let tbed = printer.tile_bed(&printer.bed());
            use ad3d_core::orient::{Orientation, QuarterTurns, fixed_quarter_turns, standing_xform};
            let o = OrientationDto::parse(&orientation)?.orientation();
            let (k, ledge) = match o {
                Orientation::Flat => (None, None),
                Orientation::Standing(QuarterTurns::Auto) => {
                    let (k, l) = tile.best_turn(plate_turns, &gopts.tol);
                    (Some(k), Some(l))
                }
                o => {
                    let k = fixed_quarter_turns(o).unwrap_or(0);
                    let all: Vec<u32> = (0..tile.partition.pieces.len() as u32).collect();
                    let l = ad3d_core::orient::max_downward_ledge(&tile.partition, &tile.heights, &all, ad3d_core::orient::down_in_plate(k), &gopts.tol);
                    (Some(k), Some(l))
                }
            };
            tile.set_print_turn(k);
            // Face parallel to the bed's motion, placed inside the margins like the plate's parts.
            let base_x = k.map_or(ad3d_core::orient::Xform::IDENTITY, |k| standing_xform(k, tile.key.w_mm, tile.key.h_mm, tbed.motion));
            let (lo, hi) = ad3d_core::squama::parts::mesh_box(&tile.mesh);
            let item = ad3d_core::squama::parts::LayoutItem { base: base_x, min: lo, max: hi, may_rotate: k.is_none() };
            let placed = ad3d_core::squama::parts::layout_on_beds(&[item], &tbed)?;
            let x = placed[0].transform;
            let rep = validate(&tile.mesh, &ValidateOpts { check_self_intersection: out.strict, ..tile.validate_opts(x) });
            println!(
                "calibration tile: {} x {} mm, {} steps ({:.1}° .. {:.1}°), light side {:?}, {} triangles ({gen_ms:.1} ms)",
                tile.key.w_mm,
                tile.key.h_mm,
                tile.key.steps(),
                tile.key.tilt_min_deg,
                tile.key.tilt_max_deg,
                tile.key.light_side,
                tile.mesh.triangle_count()
            );
            println!(
                "orientation: {}",
                k.map(|k| format!("standing, {k} quarter turn(s), largest downward ledge {:.2} mm", ledge.unwrap_or(0.0)))
                    .unwrap_or_else(|| "flat".into())
            );
            if let (Some(k), Some(pk)) = (k, plate_turns)
                && (k + 4 - pk) % 2 != 0
            {
                println!(
                    "warning [calibration_turn_differs]: the tile prints with {k} quarter turn(s), the plate with {pk}: its layer lines run the other way relative to the light"
                );
            }
            if ledge.is_some_and(|l| l > ad3d_core::squama::calibration::MAX_LEDGE_MM) {
                println!("warning [downward_ledge]: largest downward-facing ledge {:.2} mm (above 0.6 mm may sag)", ledge.unwrap_or(0.0));
            }
            for w in &tile.warnings {
                println!("warning [{}]: {}", w.code, w.message);
            }
            print!("{}", rep.summary());
            let outs = outputs(&out.out, &out.format, "calibration")?;
            let key_json = serde_json::to_string_pretty(&tile.key)?;
            let meta = Meta {
                title: Some("AD3D calibration tile".into()),
                description: Some("Print with the plate's settings, photograph under the lamp, read with the key".into()),
                custom: vec![("generator".into(), "squama-calibration".into()), ("key".into(), key_json.clone())],
                ..Default::default()
            };
            let part = ExportPart { name: "calibration".into(), kind: ad3d_core::io::sink::PartKind::Calibration, mesh: tile.mesh.clone(), transform: x };
            for w in write_parts(&outs, &[part], None, &meta, &printer.bed(), None, &CancelToken::new())? {
                println!("wrote {} ({} bytes)", w.path.display(), w.bytes);
            }
            let key_path = outs.dir.join(format!("{}.key.json", safe_file_name(&outs.stem)));
            std::fs::write(&key_path, key_json)?;
            println!("wrote {}", key_path.display());
            Ok(if rep.ok { ExitCode::SUCCESS } else { ExitCode::from(1) })
        }
        Cmd::Validate { file, min_distance, components, multi_shell, min_altitude, strict } => {
            let t0 = Instant::now();
            let is_3mf = file.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("3mf"));
            let mut all_ok = true;
            if is_3mf {
                let m = read_3mf(&file)?;
                println!("file: {} (3MF, unit {}, {} objects, {} build items)", file.display(), m.unit, m.objects.len(), m.items.len());
                if m.objects.is_empty() {
                    println!("no objects");
                    all_ok = false;
                }
                for o in &m.objects {
                    let rep = validate_mesh(&o.mesh, min_distance, shells_for(&o.name, components, multi_shell), min_altitude, strict);
                    if let Some(b) = o.mesh.bounds() {
                        let s = b.size();
                        println!("object {} {:?}: {} triangles, size {:.3} x {:.3} x {:.3} mm", o.id, o.name, o.mesh.triangle_count(), s[0], s[1], s[2]);
                    }
                    print!("{}", rep.summary());
                    all_ok &= rep.ok;
                }
            } else {
                let m = read_stl(&file)?;
                let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let rep = validate_mesh(&m, min_distance, shells_for(name, components, multi_shell), min_altitude, strict);
                println!("file: {} ({} triangles)", file.display(), m.triangle_count());
                if let Some(b) = m.bounds() {
                    let s = b.size();
                    println!("bounds: [{:.3}, {:.3}, {:.3}] size {:.3} x {:.3} x {:.3} mm", b.min[0], b.min[1], b.min[2], s[0], s[1], s[2]);
                }
                print!("{}", rep.summary());
                all_ok = rep.ok;
            }
            println!("({:.1} ms)", ms(t0));
            Ok(if all_ok { ExitCode::SUCCESS } else { ExitCode::from(1) })
        }
        Cmd::Project { cmd: ProjectCmd::Open { file, out, format, strict, json } } => {
            let p = Project::load(&file)?;
            let d = &p.doc;
            println!("project: {} (schema {}, generator {}, written by AD3D {})", file.display(), d.schema_version, d.generator, d.app_version);
            match &d.image {
                Some(i) => println!("image: {} ({} bytes, fnv64 {})", i.original_name, i.size, i.fnv64),
                None => println!("image: synthetic"),
            }
            println!("printer: {} ({} x {} x {} mm)", d.printer.name, d.printer.build_mm[0], d.printer.build_mm[1], d.printer.build_mm[2]);
            println!("thumbnail: {}", if p.thumbnail_png.is_some() { "yes" } else { "no" });
            if json {
                println!("{}", serde_json::to_string_pretty(d)?);
            }
            match out {
                Some(o) => {
                    let mut export = d.export.clone();
                    if !format.is_empty() {
                        export.formats = format.iter().map(|&f| f.into()).collect();
                    }
                    let fmts: Vec<FormatArg> = export
                        .formats
                        .iter()
                        .map(|f| if *f == ExportFormat::Stl { FormatArg::Stl } else { FormatArg::ThreeMf })
                        .collect();
                    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("project").to_string();
                    let outs = outputs(&o, &fmts, &stem)?;
                    let luma = p.luma()?;
                    let src = d.image.as_ref().map(|i| i.original_name.clone()).unwrap_or_else(|| "synthetic".into());
                    let (code, _) = run_squama(&p.params_dto(0), luma.as_ref(), &export, &d.printer, &outs, strict, &src)?;
                    Ok(code)
                }
                None => Ok(ExitCode::SUCCESS),
            }
        }
        Cmd::Project { cmd: ProjectCmd::Save { args, out } } => {
            let (params, export, image, printer) = squama_job(&args)?;
            let bytes = match &image {
                Some(p) => Some((p.to_string_lossy().to_string(), std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?)),
                None => None,
            };
            // Generate once for the thumbnail (and to refuse params that cannot run).
            let luma = match &image {
                Some(p) => Some(LinearLuma::load(p)?),
                None => None,
            };
            let r = adapter::generate(&params, luma.as_ref(), &ExportSettings { tiles: TilesDto::Mode(ad3d_core::project::TileMode::None), ..export.clone() }, &printer, &CancelToken::new())?;
            save_project_file(&out, params, bytes, export, printer, Some(&r))?;
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Profiles => {
            println!("printers (id: name, build X x Y x Z mm, moving bed axis):");
            let def = default_printer().id;
            for p in printers() {
                println!(
                    "  {}{}: {}, {} x {} x {}, {:?}",
                    p.id,
                    if p.id == def { " (default)" } else { "" },
                    p.name,
                    p.build_mm[0],
                    p.build_mm[1],
                    p.build_mm[2],
                    p.moving_bed_axis
                );
            }
            println!("lasers (stubs):");
            for l in ad3d_core::profiles::lasers() {
                println!("  {}: {}", l.id, l.name);
            }
            println!("cnc (stubs):");
            for c in ad3d_core::profiles::cncs() {
                println!("  {}: {}", c.id, c.name);
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn save_project_file(
    path: &Path,
    mut params: Value,
    image: Option<(String, Vec<u8>)>,
    export: ExportSettings,
    printer: PrinterProfile,
    generated: Option<&adapter::GenResult>,
) -> R<()> {
    // imageOpts live beside params in the project document.
    let io: Option<ImageOptsDto> = match params.as_object_mut().and_then(|o| o.remove("imageOpts")) {
        Some(Value::Null) | None => None,
        Some(v) => Some(serde_json::from_value(v)?),
    };
    let mut p = Project::new_squama(params, io, image.as_ref().map(|(n, b)| (n.as_str(), b.clone())), export, printer)?;
    if let Some(r) = generated {
        p.thumbnail_png = render_relief_png(&r.plate.mesh, r.job.params.light_azimuth, r.job.params.light_elevation, 256).ok();
    }
    p.save(path)?;
    println!("saved project {} ({} bytes)", path.display(), std::fs::metadata(path)?.len());
    Ok(())
}
