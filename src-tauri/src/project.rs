//! Project files (APP-FILES): new / open / save / save-as, plus the native file dialogs.
//!
//! The UI orchestrates the flow (unsaved-changes prompt, recent list, history reset); these
//! commands only pick paths and read or write documents, always off the UI thread.
//!
//! Commands (mirrors `src/app/host.ts`):
//! - `pick_open_path { kind: "any" | "project" | "image" }` → `string | null` (null = cancelled)
//! - `pick_save_path { suggestedName }`                      → `string | null` (always ends in .ad3d)
//! - `pick_folder`                                           → `string | null` (export folder)
//! - `project_read { path }`                                 → `OpenedProject`
//! - `project_write { path, doc }`                           → `string` (path written)
//!
//! The document format belongs to the core crate (`ad3d_core::project`, a zip with the photo
//! embedded); [`adapter`] maps the UI's document onto it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

pub const PROJECT_EXT: &str = "ad3d";
pub const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp", "heic", "heif"];

/// One saved design: which generator, its parameters and the view it was saved with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDoc {
    /// Always [`adapter::FORMAT`].
    #[serde(default = "adapter::format_tag")]
    pub format: String,
    /// Document schema version ([`adapter::FORMAT_VERSION`]).
    #[serde(default = "adapter::format_version")]
    pub version: u32,
    /// Generator id, e.g. `"shadow_plate"`.
    pub generator: String,
    #[serde(default = "one")]
    pub generator_version: u32,
    /// Generator parameters exactly as the UI sends them (the DTO without `jobId`). Kept as
    /// JSON so optional M1 fields pass through untouched.
    pub params: Value,
    /// Viewport-only settings (preview light, ambient). Optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<Value>,
    /// Printer profile id the design was made for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub printer: Option<String>,
    /// The export panel's settings (UI shape: printerId, bedMm, marginMm, orientation,
    /// formats, keys, bedMotion, stand, calibrationTile, tile). Optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export: Option<Value>,
    /// App version that wrote the file (informational).
    #[serde(default)]
    pub app_version: String,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedProject {
    pub path: String,
    pub doc: ProjectDoc,
    /// Non-fatal problems found while opening (e.g. a moved image that was found beside the project).
    pub warnings: Vec<String>,
}

/// Reading and writing `.ad3d` files through the core project module
/// (`ad3d_core::project`, FEATURES APP-FILES): a zip with `project.json`, the
/// input photo embedded, and a thumbnail. The UI's [`ProjectDoc`] maps onto the
/// core document; its field names are the UI contract and do not change.
///
/// Plain-JSON documents written by earlier builds are still read.
pub mod adapter {
    use super::*;
    use ad3d_core::project::{EMBEDDED_IMAGE, ExportSettings, ImageRef, Project, ProjectDoc as CoreDoc, SCHEMA_VERSION};

    pub const FORMAT: &str = "ad3d.project";
    pub const FORMAT_VERSION: u32 = 1;
    /// The UI's id of the Squama generator; the core calls it `squama`.
    pub const UI_SQUAMA: &str = "shadow_plate";

    pub fn format_tag() -> String {
        FORMAT.into()
    }
    pub fn format_version() -> u32 {
        FORMAT_VERSION
    }

    fn validate(doc: &ProjectDoc) -> Result<(), String> {
        if doc.generator.trim().is_empty() {
            return Err("project has no generator".into());
        }
        if !doc.params.is_object() {
            return Err("project params must be an object".into());
        }
        Ok(())
    }

    fn printer_for(id: Option<&str>) -> ad3d_core::profiles::PrinterProfile {
        let mut p = id.and_then(ad3d_core::profiles::printer_by_id).unwrap_or_else(ad3d_core::profiles::default_printer);
        if let Some(id) = id {
            p.id = id.to_owned();
        }
        p
    }

    /// UI document → core project. A photo the params point to is embedded, so the
    /// project opens on the other machine; a missing photo stays a path reference.
    pub fn to_core(doc: &ProjectDoc) -> Result<Project, String> {
        validate(doc)?;
        let mut params = doc.params.clone();
        let obj = params.as_object_mut().ok_or("project params must be an object")?;
        obj.remove("jobId");
        obj.remove("quality");
        let mut image = None;
        let mut bytes = None;
        if let Some(path) = obj.get("image").and_then(Value::as_str).map(str::to_owned)
            && path != "synthetic"
            && Path::new(&path).is_file()
        {
            let b = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
            let name = Path::new(&path).file_name().and_then(|s| s.to_str()).unwrap_or("image").to_owned();
            image = Some(ImageRef {
                entry: format!("image/{}", ad3d_core::io::sink::safe_file_name(&name)),
                original_name: name,
                size: b.len() as u64,
                fnv64: ad3d_core::io::hash::fnv64_hex(&b),
            });
            obj.insert("image".into(), Value::from(EMBEDDED_IMAGE));
            bytes = Some(b);
        }
        let generator = if doc.generator == UI_SQUAMA { "squama".to_owned() } else { doc.generator.clone() };
        let core = CoreDoc {
            schema_version: SCHEMA_VERSION,
            generator,
            app_version: env!("CARGO_PKG_VERSION").into(),
            params,
            image_opts: None,
            export: export_to_core(doc.export.as_ref()),
            printer: printer_with_motion(printer_for(doc.printer.as_deref()), doc.export.as_ref()),
            image,
            view: doc.view.clone(),
        };
        Ok(Project { doc: core, image_bytes: bytes, thumbnail_png: None })
    }

    /// Where an embedded photo is unpacked so the generator can read it by path.
    pub fn embedded_dir() -> PathBuf {
        std::env::temp_dir().join("ad3d-embedded-images")
    }

    /// Core project → UI document. An embedded photo is unpacked once (by content
    /// hash) and `params.image` points to the copy.
    pub fn from_core(p: &Project) -> Result<ProjectDoc, String> {
        let d = &p.doc;
        let mut params = d.params.clone();
        if let Some(io) = &d.image_opts
            && let Some(o) = params.as_object_mut()
        {
            o.entry("imageOpts").or_insert(serde_json::to_value(io).map_err(|e| e.to_string())?);
        }
        if let (Some(r), Some(b)) = (&d.image, &p.image_bytes) {
            let dir = embedded_dir().join(&r.fnv64);
            let path = dir.join(ad3d_core::io::sink::safe_file_name(&r.original_name));
            if !path.is_file() {
                std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                crate::settings::write_atomic(&path, b)?;
            }
            params["image"] = Value::String(path.to_string_lossy().into_owned());
        }
        let generator = if d.generator == "squama" { UI_SQUAMA.to_owned() } else { d.generator.clone() };
        Ok(ProjectDoc {
            format: FORMAT.into(),
            version: FORMAT_VERSION,
            generator,
            generator_version: 1,
            params,
            view: d.view.clone(),
            printer: Some(d.printer.id.clone()),
            export: Some(export_from_core(&d.export, &d.printer)),
            app_version: d.app_version.clone(),
        })
    }

    /// The export panel's settings → the core's export settings. Missing or bad fields keep
    /// the core defaults.
    pub fn export_to_core(ui: Option<&Value>) -> ExportSettings {
        use ad3d_core::project::{TileMode, TilesDto};
        let mut e = ExportSettings::default();
        let Some(ui) = ui.and_then(Value::as_object) else { return e };
        let get = |k: &str| ui.get(k).cloned();
        if let Some(v) = get("formats").and_then(|v| serde_json::from_value(v).ok()) {
            e.formats = v;
        }
        if let Some(v) = get("orientation").and_then(|v| serde_json::from_value(v).ok()) {
            e.orientation = v;
        }
        if let Some(v) = get("keys").and_then(|v| serde_json::from_value(v).ok()) {
            e.keys = v;
        }
        e.bed_mm = get("bedMm").and_then(|v| serde_json::from_value(v).ok());
        e.margin_mm = get("marginMm").and_then(|v| v.as_f64());
        e.stand = get("stand").and_then(|v| v.as_bool()).unwrap_or(false);
        e.calibration_tile = get("calibrationTile").and_then(|v| v.as_bool()).unwrap_or(false);
        if get("tile").and_then(|v| v.as_bool()) == Some(false) {
            e.tiles = TilesDto::Mode(TileMode::None);
        }
        e
    }

    /// The printer with the export panel's bed motion (kept when the bed size is edited).
    fn printer_with_motion(mut p: ad3d_core::profiles::PrinterProfile, ui: Option<&Value>) -> ad3d_core::profiles::PrinterProfile {
        use ad3d_core::profiles::Axis;
        match ui.and_then(|v| v.get("bedMotion")).and_then(Value::as_str) {
            Some("x") => p.moving_bed_axis = Some(Axis::X),
            Some("y") => p.moving_bed_axis = Some(Axis::Y),
            Some("none") => p.moving_bed_axis = None,
            _ => {}
        }
        p
    }

    /// The core's export settings → the export panel's shape.
    pub fn export_from_core(e: &ExportSettings, printer: &ad3d_core::profiles::PrinterProfile) -> Value {
        use ad3d_core::profiles::Axis;
        use ad3d_core::project::{TileMode, TilesDto};
        serde_json::json!({
            "printerId": printer.id,
            "bedMm": e.bed_mm.unwrap_or(printer.build_mm),
            "marginMm": e.margin_mm.unwrap_or(printer.margin_mm),
            "orientation": e.orientation,
            "formats": e.formats,
            "keys": e.keys,
            "bedMotion": match printer.moving_bed_axis { Some(Axis::X) => "x", Some(Axis::Y) => "y", _ => "none" },
            "stand": e.stand,
            "calibrationTile": e.calibration_tile,
            "tile": e.tiles != TilesDto::Mode(TileMode::None),
        })
    }

    /// Plain-JSON document of earlier builds.
    pub fn decode_json(bytes: &[u8]) -> Result<ProjectDoc, String> {
        let doc: ProjectDoc = serde_json::from_slice(bytes).map_err(|e| format!("not an AD3D project: {e}"))?;
        if doc.format != FORMAT {
            return Err(format!("not an AD3D project (format {:?})", doc.format));
        }
        if doc.version > FORMAT_VERSION {
            return Err(format!(
                "project format v{} is newer than this app supports (v{FORMAT_VERSION}); update AD3D",
                doc.version
            ));
        }
        validate(&doc)?;
        Ok(doc)
    }

    pub fn encode(doc: &ProjectDoc) -> Result<Vec<u8>, String> {
        to_core(doc)?.to_bytes().map_err(|e| e.to_string())
    }

    pub fn decode(bytes: &[u8]) -> Result<ProjectDoc, String> {
        if bytes.starts_with(b"PK\x03\x04") {
            let p = Project::from_bytes(bytes).map_err(|e| e.to_string())?;
            from_core(&p)
        } else {
            decode_json(bytes)
        }
    }

    pub fn write(path: &Path, doc: &ProjectDoc) -> Result<(), String> {
        crate::settings::write_atomic(path, &encode(doc)?)
    }

    pub fn read(path: &Path) -> Result<ProjectDoc, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        decode(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// Appends `.ad3d` unless the path already ends with it (case-insensitive).
pub fn with_project_ext(path: &Path) -> PathBuf {
    let has = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(PROJECT_EXT));
    if has {
        path.to_path_buf()
    } else {
        let mut s = path.as_os_str().to_owned();
        s.push(".");
        s.push(PROJECT_EXT);
        PathBuf::from(s)
    }
}

/// If the image a project points to has moved, look for a file of the same name beside the
/// project (the usual case after copying a folder to the second machine).
pub fn resolve_image(doc: &mut ProjectDoc, project_path: &Path) -> Option<String> {
    let img = doc.params.get("image").and_then(Value::as_str)?.to_owned();
    if img == "synthetic" || img.is_empty() || Path::new(&img).exists() {
        return None;
    }
    let name = Path::new(&img).file_name()?;
    let candidate = project_path.parent()?.join(name);
    if candidate.exists() {
        let found = candidate.to_string_lossy().into_owned();
        doc.params["image"] = Value::String(found.clone());
        Some(format!("image {img} not found; using {found}"))
    } else {
        Some(format!("image {img} not found; choose it again"))
    }
}

pub fn read_project(path: &Path) -> Result<OpenedProject, String> {
    let mut doc = adapter::read(path)?;
    let warnings = resolve_image(&mut doc, path).into_iter().collect();
    Ok(OpenedProject { path: path.to_string_lossy().into_owned(), doc, warnings })
}

pub fn write_project(path: &Path, doc: &ProjectDoc) -> Result<PathBuf, String> {
    let path = with_project_ext(path);
    adapter::write(&path, doc)?;
    Ok(path)
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| format!("worker failed: {e}"))?
}

fn to_string(p: tauri_plugin_dialog::FilePath) -> Result<String, String> {
    p.into_path().map(|p| p.to_string_lossy().into_owned()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn pick_open_path(kind: String, app: AppHandle) -> Result<Option<String>, String> {
    blocking(move || {
        let mut d = app.dialog().file().set_title("Open");
        d = match kind.as_str() {
            "project" => d.add_filter("AD3D project", &[PROJECT_EXT]),
            "image" => d.add_filter("Image", IMAGE_EXTS),
            _ => {
                let mut all: Vec<&str> = vec![PROJECT_EXT];
                all.extend_from_slice(IMAGE_EXTS);
                d.add_filter("Projects and images", &all)
                    .add_filter("AD3D project", &[PROJECT_EXT])
                    .add_filter("Image", IMAGE_EXTS)
            }
        };
        d.blocking_pick_file().map(to_string).transpose()
    })
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn pick_save_path(suggested_name: Option<String>, app: AppHandle) -> Result<Option<String>, String> {
    blocking(move || {
        let name = suggested_name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "Untitled".into());
        let picked = app
            .dialog()
            .file()
            .set_title("Save project")
            .add_filter("AD3D project", &[PROJECT_EXT])
            .set_file_name(with_project_ext(Path::new(&name)).to_string_lossy())
            .blocking_save_file();
        match picked {
            None => Ok(None),
            Some(p) => Ok(Some(with_project_ext(Path::new(&to_string(p)?)).to_string_lossy().into_owned())),
        }
    })
    .await
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> Result<Option<String>, String> {
    blocking(move || app.dialog().file().set_title("Choose a folder").blocking_pick_folder().map(to_string).transpose())
        .await
}

#[tauri::command]
pub async fn project_read(path: String) -> Result<OpenedProject, String> {
    blocking(move || read_project(Path::new(&path))).await
}

#[tauri::command]
pub async fn project_write(path: String, doc: ProjectDoc) -> Result<String, String> {
    blocking(move || write_project(Path::new(&path), &doc).map(|p| p.to_string_lossy().into_owned())).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ad3d-project-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn doc() -> ProjectDoc {
        ProjectDoc {
            format: String::new(),
            version: 0,
            generator: "shadow_plate".into(),
            generator_version: 1,
            params: json!({ "widthMm": 196, "cellMm": 4, "lightAzimuthDeg": 180, "lightElevationDeg": 10,
                            "maxTiltDeg": 10, "baseMm": 3, "image": "synthetic",
                            "imageOpts": { "gamma": 1.2 } }),
            view: Some(json!({ "ambient": 0.05 })),
            printer: Some("creality-cr6-se".into()),
            export: Some(json!({ "printerId": "creality-cr6-se", "bedMm": [235.0, 235.0, 250.0], "marginMm": 6.0,
                                 "orientation": "standing-q2", "formats": ["3mf", "stl"], "keys": "pegs",
                                 "bedMotion": "y", "stand": true, "calibrationTile": true, "tile": false })),
            app_version: String::new(),
        }
    }

    #[test]
    fn extension_is_added_once() {
        assert_eq!(with_project_ext(Path::new("/a/b")), PathBuf::from("/a/b.ad3d"));
        assert_eq!(with_project_ext(Path::new("/a/b.ad3d")), PathBuf::from("/a/b.ad3d"));
        assert_eq!(with_project_ext(Path::new("/a/b.AD3D")), PathBuf::from("/a/b.AD3D"));
        assert_eq!(with_project_ext(Path::new("/a/b.png")), PathBuf::from("/a/b.png.ad3d"));
    }

    #[test]
    fn save_then_open_round_trips() {
        let d = tmpdir("rt");
        let written = write_project(&d.join("plate"), &doc()).unwrap();
        assert_eq!(written, d.join("plate.ad3d"));
        let opened = read_project(&written).unwrap();
        assert!(opened.warnings.is_empty());
        assert_eq!(opened.doc.format, adapter::FORMAT);
        assert_eq!(opened.doc.version, adapter::FORMAT_VERSION);
        assert_eq!(opened.doc.params, doc().params, "unknown params fields survive");
        assert_eq!(opened.doc.view, doc().view);
        assert_eq!(opened.doc.printer.as_deref(), Some("creality-cr6-se"));
        assert_eq!(opened.doc.export, doc().export, "export settings survive");
        assert_eq!(opened.doc.app_version, env!("CARGO_PKG_VERSION"));
        // The file is the core's zip container.
        assert!(std::fs::read(&written).unwrap().starts_with(b"PK\x03\x04"));
    }

    #[test]
    fn photo_is_embedded_and_unpacked() {
        let d = tmpdir("embed");
        let src = d.join("photo.png");
        std::fs::write(&src, b"\x89PNG not decoded here").unwrap();
        let mut dd = doc();
        dd.params["image"] = json!(src.to_string_lossy());
        let p = write_project(&d.join("e.ad3d"), &dd).unwrap();
        std::fs::remove_file(&src).unwrap();
        let opened = read_project(&p).unwrap();
        let path = opened.doc.params["image"].as_str().unwrap().to_owned();
        assert!(Path::new(&path).is_file(), "{path}");
        assert!(path.ends_with("photo.png"));
        assert!(opened.warnings.is_empty());
    }

    #[test]
    fn rejects_foreign_and_future_files() {
        assert!(adapter::decode(b"hello").is_err());
        assert!(adapter::decode(br#"{"format":"other","generator":"x","params":{}}"#).is_err());
        let future = json!({ "format": adapter::FORMAT, "version": 99, "generator": "shadow_plate", "params": {} });
        let err = adapter::decode(future.to_string().as_bytes()).unwrap_err();
        assert!(err.contains("newer"), "{err}");
        let no_params = json!({ "format": adapter::FORMAT, "version": 1, "generator": "shadow_plate", "params": 3 });
        assert!(adapter::decode(no_params.to_string().as_bytes()).is_err());
        assert!(adapter::decode(b"PK\x03\x04rest").is_err());
        assert!(adapter::encode(&ProjectDoc { generator: " ".into(), ..doc() }).is_err());
    }

    #[test]
    fn moved_image_is_found_beside_the_project() {
        let d = tmpdir("moved");
        std::fs::write(d.join("mona.png"), b"x").unwrap();
        let mut dd = doc();
        dd.params["image"] = json!("/nowhere/at/all/mona.png");
        let p = write_project(&d.join("p.ad3d"), &dd).unwrap();
        let opened = read_project(&p).unwrap();
        assert_eq!(opened.doc.params["image"], json!(d.join("mona.png").to_string_lossy()));
        assert_eq!(opened.warnings.len(), 1);

        dd.params["image"] = json!("/nowhere/gone.png");
        let p = write_project(&d.join("q.ad3d"), &dd).unwrap();
        let opened = read_project(&p).unwrap();
        assert_eq!(opened.doc.params["image"], json!("/nowhere/gone.png"));
        assert!(opened.warnings[0].contains("choose it again"));
    }

    #[test]
    fn overwrite_is_atomic_and_leaves_no_temp_files() {
        let d = tmpdir("atomic");
        let p = d.join("x.ad3d");
        write_project(&p, &doc()).unwrap();
        let mut d2 = doc();
        d2.params["widthMm"] = json!(100);
        write_project(&p, &d2).unwrap();
        assert_eq!(read_project(&p).unwrap().doc.params["widthMm"], json!(100));
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
    }
}
