//! AD3D desktop shell: Tauri 2 commands around `ad3d-core`.
//!
//! Every command that does CPU or file work is `async` and moves that work to
//! `tauri::async_runtime::spawn_blocking`, so neither the main (UI) thread nor the
//! async runtime's worker threads are ever blocked by geometry.
//!
//! Command contract (mirrors `src/engine/tauriEngine.ts`):
//! - `generate_squama { params }`  → raw `AD3M` payload bytes (latest-wins, cancellable)
//! - `synthetic_mesh { nTriangles }` → raw `AD3M` payload bytes (mesh cached, so C4 measures transfer)
//! - `get_launch_flags`            → `{ spike }` from `AD3D_SPIKE=1`
//! - `save_artifact`               → raw body = file bytes, header `x-artifact-name` (spike mode only)
//! - `finish_spike { report }`     → writes `report.json`, then exits with code 0 (spike mode only)
//!
//! Spike mode (`AD3D_SPIKE=1`) writes into `AD3D_SPIKE_OUT`, or `<app data dir>/spike`
//! when that is unset. No build-machine path is compiled into the binary.
//! `AD3D_SPIKE_MAXIMIZE=1` maximises the window before the harness starts.

mod menu;
mod project;
mod settings;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use ad3d_core::image::{CellBrightness, LinearLuma, cell_brightness, synthetic_target};
use ad3d_core::squama::{self, GenOptions, SYNTHETIC_PX, SquamaParamsDto};
use ad3d_core::{CancelToken, Mesh, payload};
use serde::Serialize;
use tauri::ipc::{InvokeBody, Request, Response};
use tauri::{AppHandle, Manager, State};

/// Decoded images kept in memory (the synthetic target counts as one).
const IMAGE_CACHE_MAX: usize = 4;
/// Cell-brightness grids kept in memory (per image, grid size and crop).
const CELL_CACHE_MAX: usize = 32;

/// Cache key of the built-in synthetic target (the DTO's `image: "synthetic"`).
const SQUAMA_SYNTHETIC_KEY: &str = "synthetic";

type CellKey = (String, u32, u32, Option<[u64; 4]>);

#[derive(Default)]
struct AppState {
    /// Arrival sequence of `generate_squama` calls; the newest one wins.
    newest_job: AtomicU64,
    /// Cancel token of the job currently running, tagged with its sequence number.
    running: Mutex<Option<(u64, CancelToken)>>,
    images: Mutex<HashMap<String, Arc<LinearLuma>>>,
    cells: Mutex<HashMap<CellKey, Arc<CellBrightness>>>,
    /// `(n_triangles, mesh, generation ms)` of the last synthetic mesh.
    synthetic: Mutex<Option<(u32, Arc<Mesh>, f32)>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn spike_out_dir(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("AD3D_SPIKE_OUT").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    app.path().app_data_dir().map(|d| d.join("spike")).map_err(|e| format!("no app data dir: {e}"))
}

/// Spike-only commands refuse to run outside spike mode, so a stray frontend call can
/// neither write files nor quit the app.
fn require_spike(command: &str) -> Result<(), String> {
    if spike_flag() { Ok(()) } else { Err(format!("{command} is only available with AD3D_SPIKE=1")) }
}

fn spike_flag() -> bool {
    std::env::var("AD3D_SPIKE").is_ok_and(|v| v == "1")
}

/// Spike mode, for the app-shell modules (the close guard is off in spike runs).
pub(crate) fn spike_mode() -> bool {
    spike_flag()
}

async fn blocking<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| format!("worker failed: {e}"))?
}

impl AppState {
    fn image(&self, key: &str) -> Result<Arc<LinearLuma>, String> {
        if let Some(img) = lock(&self.images).get(key) {
            return Ok(img.clone());
        }
        // Decode outside the lock; two racing decodes of one path are harmless.
        let img = if key == SQUAMA_SYNTHETIC_KEY {
            synthetic_target(SYNTHETIC_PX.0, SYNTHETIC_PX.1)
        } else {
            LinearLuma::load(std::path::Path::new(key)).map_err(|e| e.to_string())?
        };
        let img = Arc::new(img);
        let mut cache = lock(&self.images);
        if cache.len() >= IMAGE_CACHE_MAX {
            cache.clear();
        }
        cache.insert(key.to_owned(), img.clone());
        Ok(img)
    }

    fn cells(&self, key: &str, img: &LinearLuma, l: &squama::Layout) -> Result<Arc<CellBrightness>, String> {
        let ck: CellKey = (key.to_owned(), l.nx, l.ny, l.crop.map(|c| c.map(f64::to_bits)));
        if let Some(cb) = lock(&self.cells).get(&ck) {
            return Ok(cb.clone());
        }
        let cb = Arc::new(cell_brightness(img, l.nx, l.ny, l.crop).map_err(|e| e.to_string())?);
        let mut cache = lock(&self.cells);
        if cache.len() >= CELL_CACHE_MAX {
            cache.clear();
        }
        cache.insert(ck, cb.clone());
        Ok(cb)
    }

    fn synthetic_mesh(&self, n: u32) -> (Arc<Mesh>, f32) {
        let mut slot = lock(&self.synthetic);
        if let Some((cached_n, mesh, ms)) = slot.as_ref() {
            if *cached_n == n {
                return (mesh.clone(), *ms);
            }
        }
        let t0 = Instant::now();
        let mesh = Arc::new(ad3d_core::synth::torus(n as usize));
        let ms = t0.elapsed().as_secs_f32() * 1e3;
        *slot = Some((n, mesh.clone(), ms));
        (mesh, ms)
    }
}

#[tauri::command]
async fn generate_squama(params: SquamaParamsDto, state: State<'_, Arc<AppState>>) -> Result<Response, String> {
    let state: Arc<AppState> = state.inner().clone();
    let seq = state.newest_job.fetch_add(1, Ordering::SeqCst) + 1;
    let token = CancelToken::new();
    if let Some((_, old)) = lock(&state.running).replace((seq, token.clone())) {
        old.cancel();
    }
    blocking(move || {
        let result = (|| {
            if state.newest_job.load(Ordering::SeqCst) != seq {
                return Err("cancelled".to_owned());
            }
            let t0 = Instant::now();
            let p = params.params();
            let key = if params.is_synthetic() { SQUAMA_SYNTHETIC_KEY } else { params.image.as_str() };
            let img = state.image(key)?;
            let l = squama::layout(&p, (img.w, img.h)).map_err(|e| e.to_string())?;
            let cb = state.cells(key, &img, &l)?;
            token.check().map_err(|e| e.to_string())?;
            let out = squama::generate_ex(&p, &cb, &GenOptions::default(), &token).map_err(|e| e.to_string())?;
            let gen_ms = t0.elapsed().as_secs_f32() * 1e3;
            Ok(Response::new(payload::encode(&out.mesh, params.job_id, gen_ms)))
        })();
        let mut running = lock(&state.running);
        if running.as_ref().is_some_and(|(s, _)| *s == seq) {
            *running = None;
        }
        result
    })
    .await
}

#[tauri::command]
async fn synthetic_mesh(n_triangles: u32, state: State<'_, Arc<AppState>>) -> Result<Response, String> {
    if n_triangles == 0 || n_triangles > 20_000_000 {
        return Err(format!("n_triangles {n_triangles} outside [1, 20M]"));
    }
    let state: Arc<AppState> = state.inner().clone();
    blocking(move || {
        let (mesh, gen_ms) = state.synthetic_mesh(n_triangles);
        Ok(Response::new(payload::encode(&mesh, 0, gen_ms)))
    })
    .await
}

#[derive(Serialize)]
struct LaunchFlags {
    spike: bool,
}

#[tauri::command]
fn get_launch_flags() -> LaunchFlags {
    LaunchFlags { spike: spike_flag() }
}

fn valid_artifact_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name != "."
        && name != ".."
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

#[tauri::command]
async fn save_artifact(request: Request<'_>, app: AppHandle) -> Result<(), String> {
    require_spike("save_artifact")?;
    let dir = spike_out_dir(&app)?;
    let name = request
        .headers()
        .get("x-artifact-name")
        .and_then(|v| v.to_str().ok())
        .ok_or("missing x-artifact-name header")?
        .to_owned();
    if !valid_artifact_name(&name) {
        return Err(format!("invalid artifact name {name:?}"));
    }
    let bytes = match request.body() {
        InvokeBody::Raw(b) => b.clone(),
        InvokeBody::Json(serde_json::Value::Array(a)) => a
            .iter()
            .map(|v| v.as_u64().filter(|&x| x <= 255).map(|x| x as u8))
            .collect::<Option<Vec<u8>>>()
            .ok_or("artifact body is not a byte array")?,
        _ => return Err("artifact body must be raw bytes".into()),
    };
    blocking(move || {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = dir.join(&name);
        std::fs::write(&path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        eprintln!("[ad3d] saved {} ({} bytes)", path.display(), bytes.len());
        Ok(())
    })
    .await
}

#[tauri::command]
async fn finish_spike(report: serde_json::Value, app: AppHandle) -> Result<(), String> {
    require_spike("finish_spike")?;
    let dir = spike_out_dir(&app)?;
    blocking(move || {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = dir.join("report.json");
        let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        eprintln!("[ad3d] spike report written to {}", path.display());
        Ok(())
    })
    .await?;
    // Let the IPC response reach the page before the process goes away.
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(250));
        app.exit(0);
    });
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = Arc::new(AppState::default());
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state.clone())
        .on_window_event(menu::on_window_event)
        .setup(move |app| {
            menu::manage(app.handle());
            if let Err(e) = menu::install(app.handle()) {
                eprintln!("[ad3d] menu: {e}");
            }
            if let Some(win) = app.get_webview_window("main") {
                // A hidden or unfocused window lets WebKit throttle timers and rAF,
                // which would starve the preview loop and the spike harness.
                let _ = win.show();
                let _ = win.unminimize();
                if spike_flag() && std::env::var("AD3D_SPIKE_MAXIMIZE").is_ok_and(|v| v == "1") {
                    let _ = win.maximize();
                }
                let _ = win.set_focus();
            }
            if spike_flag() {
                // Pre-build the C4 mesh so the spike measures transfer, not generation.
                let st = state.clone();
                std::thread::spawn(move || {
                    let (_, ms) = st.synthetic_mesh(1_000_000);
                    eprintln!("[ad3d] spike: synthetic 1M-triangle mesh ready ({ms:.1} ms)");
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            generate_squama,
            synthetic_mesh,
            get_launch_flags,
            save_artifact,
            finish_spike,
            menu::menu_set_state,
            menu::confirm_close,
            project::pick_open_path,
            project::pick_save_path,
            project::pick_folder,
            project::project_read,
            project::project_write,
            settings::settings_load,
            settings::settings_save,
            settings::recent_add,
            settings::recent_clear,
            settings::printer_profiles_list
        ])
        .build(tauri::generate_context!())
        .expect("error while building the AD3D app")
        .run(|app, event| menu::on_run_event(app, &event));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spike_commands_need_the_flag() {
        // SAFETY: tests in this crate do not read AD3D_SPIKE concurrently with this one.
        unsafe { std::env::remove_var("AD3D_SPIKE") };
        assert!(require_spike("finish_spike").is_err());
        assert!(require_spike("save_artifact").is_err());
    }

    #[test]
    fn artifact_names() {
        assert!(valid_artifact_name("viewport.png"));
        assert!(valid_artifact_name("report-1_a.json"));
        assert!(!valid_artifact_name("../x"));
        assert!(!valid_artifact_name("a/b"));
        assert!(!valid_artifact_name(".."));
        assert!(!valid_artifact_name(""));
    }

    #[test]
    fn squama_payload_echoes_job_and_true_bbox() {
        let st = AppState::default();
        let dto = SquamaParamsDto {
            job_id: 42,
            width_mm: 300.0,
            height_mm: None,
            cell_mm: 6.0,
            light_azimuth_deg: 180.0,
            light_elevation_deg: 5.0,
            max_tilt_deg: 10.0,
            base_mm: 3.0,
            image: "synthetic".into(),
        };
        let p = dto.params();
        let img = st.image(SQUAMA_SYNTHETIC_KEY).unwrap();
        let l = squama::layout(&p, (img.w, img.h)).unwrap();
        assert_eq!((l.nx, l.ny), (50, 63));
        let cb = st.cells(SQUAMA_SYNTHETIC_KEY, &img, &l).unwrap();
        let out = squama::generate_ex(&p, &cb, &GenOptions::default(), &CancelToken::new()).unwrap();
        let buf = payload::encode(&out.mesh, dto.job_id, 1.0);
        let (m, job, _, _) = payload::decode(&buf).unwrap();
        assert_eq!(job, 42);
        let b = m.bounds().unwrap();
        assert_eq!([b.min[0], b.min[1]], [0.0, 0.0]);
        assert!((b.max[0] - 300.0).abs() < 1e-3 && (b.max[1] - 378.0).abs() < 1e-3);
    }
}
