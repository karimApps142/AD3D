//! `ad3d`: headless AD3D generators and mesh checks.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use ad3d_core::image::LinearLuma;
use ad3d_core::io::stl::{read_stl, write_stl};
use ad3d_core::mesh::validate::{ValidateOpts, validate};
use ad3d_core::orient::{Orientation, QuarterTurns, export_xform};
use ad3d_core::squama::{GenOptions, ImageSource, SquamaParams, run, validate_opts};
use ad3d_core::CancelToken;
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "ad3d", version, about = "AD3D generators and mesh checks (mm, degrees)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum Orient {
    /// Standing, quarter turn chosen so the light comes from below (design §9 Auto).
    Standing,
    /// Lying flat, front face up.
    Flat,
}

#[derive(Subcommand)]
enum Cmd {
    /// Generate a Squama shadow plate and write it as binary STL.
    Squama {
        /// Grayscale or colour image (EXIF orientation applied, linearised).
        #[arg(long, conflicts_with = "synthetic")]
        image: Option<PathBuf>,
        /// Use the built-in test target (default when no image is given).
        #[arg(long)]
        synthetic: bool,
        #[arg(long, default_value_t = 300.0)]
        width: f64,
        /// Optional plate height; the image is centre-cropped to fill.
        #[arg(long)]
        height: Option<f64>,
        #[arg(long, default_value_t = 6.0)]
        cell: f64,
        /// Direction the light comes from: 180 = Left, 0 = Right, 90 = Top, 270 = Bottom.
        #[arg(long, default_value_t = 180.0)]
        azimuth: f64,
        #[arg(long, default_value_t = 5.0)]
        elevation: f64,
        #[arg(long = "max-tilt", default_value_t = 10.0)]
        max_tilt: f64,
        #[arg(long, default_value_t = 3.0)]
        base: f64,
        #[arg(long, value_enum, default_value_t = Orient::Standing)]
        orientation: Orient,
        #[arg(long)]
        out: PathBuf,
        /// Also run the triangle-triangle self-intersection stage of validate.
        #[arg(long)]
        strict: bool,
    },
    /// Validate a binary STL (welds bit-identical f32 positions). Exit code 1 on failure.
    Validate {
        file: PathBuf,
        /// Minimum distinct-vertex distance gate in mm (0 disables).
        #[arg(long, default_value_t = 0.05)]
        min_distance: f64,
        /// Expected number of closed shells.
        #[arg(long, default_value_t = 1)]
        components: usize,
        /// Smallest allowed triangle altitude in mm (0 disables the sliver gate).
        #[arg(long, default_value_t = ad3d_core::mesh::validate::DEFAULT_MIN_ALTITUDE_MM)]
        min_altitude: f64,
        /// Also run the triangle-triangle self-intersection stage. Without it, OK means
        /// a closed, consistently wound 2-manifold, not a proven self-intersection-free solid.
        #[arg(long)]
        strict: bool,
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

fn real_main() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Squama { image, synthetic: _, width, height, cell, azimuth, elevation, max_tilt, base, orientation, out, strict } => {
            let p = SquamaParams {
                width_mm: width,
                height_mm: height,
                cell_mm: cell,
                light_azimuth: azimuth.to_radians(),
                light_elevation: elevation.to_radians(),
                max_tilt: max_tilt.to_radians(),
                base_mm: base,
            };
            let t_load = Instant::now();
            let luma = match &image {
                Some(path) => Some(LinearLuma::load(path)?),
                None => None,
            };
            let load_ms = t_load.elapsed().as_secs_f64() * 1e3;
            let src = match &luma {
                Some(l) => ImageSource::Luma(l),
                None => ImageSource::Synthetic,
            };
            let o = run(&p, src, &GenOptions::default(), &CancelToken::new())?;
            let l = o.layout;
            println!(
                "source: {}",
                image.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "synthetic".into())
            );
            println!("grid: {} x {} cells of {} mm -> plate {} x {} mm", l.nx, l.ny, l.cell_mm, l.w_mm, l.h_mm);
            println!(
                "mesh: {} vertices, {} triangles | saddles {} crossings {} walls {} outline {}",
                o.mesh.vertex_count(),
                o.mesh.triangle_count(),
                o.stats.mesh.saddles,
                o.stats.mesh.crossings,
                o.stats.mesh.walls,
                o.stats.mesh.outline_vertices
            );
            println!(
                "timings (ms): load {:.2} | sample {:.3} | solve {:.3} | quantise {:.3} | mesh {:.3} | generate total {:.3}",
                load_ms, o.stats.sample_ms, o.stats.solve_ms, o.stats.quantise_ms, o.stats.mesh_ms, o.stats.total_ms
            );
            for w in &o.warnings {
                println!("warning [{}]: {}", w.code, w.message);
            }
            let orient = match orientation {
                Orient::Standing => Orientation::Standing(QuarterTurns::Auto),
                Orient::Flat => Orientation::Flat,
            };
            let (x, k) = export_xform(orient, l.w_mm, l.h_mm, p.light_azimuth);
            match k {
                Some(k) => println!("orientation: standing, {} x 90° quarter turns (light from below in print space)", k),
                None => println!("orientation: flat"),
            }
            let t_v = Instant::now();
            let rep = validate(&o.mesh, &ValidateOpts { check_self_intersection: strict, ..validate_opts(&o, x) });
            let v_ms = t_v.elapsed().as_secs_f64() * 1e3;
            let t_w = Instant::now();
            if let Some(dir) = out.parent() {
                if !dir.as_os_str().is_empty() {
                    std::fs::create_dir_all(dir)?;
                }
            }
            write_stl(&out, &o.mesh, &x)?;
            let w_ms = t_w.elapsed().as_secs_f64() * 1e3;
            let b = o.mesh.bounds().unwrap();
            println!(
                "plate-space bounds: [{:.3}, {:.3}, {:.3}] .. [{:.3}, {:.3}, {:.3}] | volume {:.1} mm3",
                b.min[0], b.min[1], b.min[2], b.max[0], b.max[1], b.max[2], o.mesh.signed_volume()
            );
            println!("validate ({v_ms:.1} ms):");
            print!("{}", rep.summary());
            println!("wrote {} ({w_ms:.1} ms)", out.display());
            Ok(if rep.ok { ExitCode::SUCCESS } else { ExitCode::from(1) })
        }
        Cmd::Validate { file, min_distance, components, min_altitude, strict } => {
            let t0 = Instant::now();
            let m = read_stl(&file)?;
            let opts = ValidateOpts {
                soup: true,
                min_distance: if min_distance > 0.0 { Some(min_distance) } else { None },
                expect_components: components,
                min_altitude: if min_altitude > 0.0 { Some(min_altitude) } else { None },
                check_self_intersection: strict,
                ..Default::default()
            };
            let rep = validate(&m, &opts);
            let b = m.bounds();
            println!("file: {} ({} triangles)", file.display(), m.triangle_count());
            if let Some(b) = b {
                let s = b.size();
                println!("bounds: [{:.3}, {:.3}, {:.3}] size {:.3} x {:.3} x {:.3} mm", b.min[0], b.min[1], b.min[2], s[0], s[1], s[2]);
            }
            print!("{}", rep.summary());
            println!("({:.1} ms)", t0.elapsed().as_secs_f64() * 1e3);
            Ok(if rep.ok { ExitCode::SUCCESS } else { ExitCode::from(1) })
        }
    }
}
