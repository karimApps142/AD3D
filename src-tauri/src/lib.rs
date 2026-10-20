//! AD3D desktop shell: Tauri 2 commands around `ad3d-core`.
//!
//! Every command that does CPU or file work is `async` and moves that work to
//! `tauri::async_runtime::spawn_blocking`, so neither the main (UI) thread nor the
//! async runtime's worker threads are ever blocked by geometry.
//!
//! Command contract (mirrors `src/engine/tauriEngine.ts`):
//! - Squama commands (`generate_squama`, `squama_*`, `export_squama`, `cancel_export`, `cancel_generate`,
//!   `reveal_in_finder`) live in `squama_cmds.rs`.
//! - `synthetic_mesh { nTriangles }` → raw `AD3M` payload bytes (mesh cached, so C4 measures transfer)
//! - `get_launch_flags`            → `{ spike }` from `AD3D_SPIKE=1`
//! - `save_artifact`               → raw body = file bytes, header `x-artifact-name` (spike mode only)
//! - `finish_spike { report }`     → writes `report.json`, then exits with code 0 (spike mode only)
//! - `save_preview_image`          → raw body = PNG bytes, header `x-file-name`; writes to
//!   `~/Pictures/AD3D/` (falls back to Downloads, then the app data dir) and returns the path
//!
//! Spike mode (`AD3D_SPIKE=1`) writes into `AD3D_SPIKE_OUT`, or `<app data dir>/spike`
//! when that is unset. No build-machine path is compiled into the binary.
//! `AD3D_SPIKE_MAXIMIZE=1` maximises the window before the harness starts.

mod menu;
mod project;
mod settings;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use ad3d_core::{Mesh, payload};
use serde::Serialize;
use tauri::ipc::{InvokeBody, Request, Response};
use tauri::{AppHandle, Manager, State};

mod squama_cmds;
#[cfg(test)]
mod e2e_m1;
#[cfg(test)]
mod m1_review_tests;

#[derive(Default)]
struct AppState {
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

/// Folder for "Save preview image": `~/Pictures/AD3D`, else Downloads, else app data.
fn preview_image_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let p = app.path();
    p.picture_dir()
        .or_else(|_| p.download_dir())
        .or_else(|_| p.app_data_dir())
        .map(|d| d.join("AD3D"))
        .map_err(|e| e.to_string())
}

fn png_body(request: &Request<'_>) -> Result<Vec<u8>, String> {
    let bytes = match request.body() {
        InvokeBody::Raw(b) => b.clone(),
        _ => return Err("preview image body must be raw bytes".into()),
    };
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("preview image is not a PNG".into());
    }
    Ok(bytes)
}

#[tauri::command]
async fn save_preview_image(request: Request<'_>, app: AppHandle) -> Result<String, String> {
    let name = request
        .headers()
        .get("x-file-name")
        .and_then(|v| v.to_str().ok())
        .ok_or("missing x-file-name header")?
        .to_owned();
    if !valid_artifact_name(&name) || !name.ends_with(".png") {
        return Err(format!("invalid preview image name {name:?}"));
    }
    let bytes = png_body(&request)?;
    let dir = preview_image_dir(&app)?;
    blocking(move || {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = dir.join(&name);
        std::fs::write(&path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path.display().to_string())
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
        .manage(Arc::new(squama_cmds::SquamaState::default()))
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
            squama_cmds::generate_squama,
            squama_cmds::squama_job_info,
            squama_cmds::squama_target,
            squama_cmds::squama_tile_plan,
            squama_cmds::squama_image_open,
            squama_cmds::squama_image_thumbnail,
            squama_cmds::export_squama,
            squama_cmds::cancel_export,
            squama_cmds::cancel_generate,
            squama_cmds::reveal_in_finder,
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
            settings::printer_profiles_list,
            save_preview_image
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
}
