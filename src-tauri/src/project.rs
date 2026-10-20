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
//! The document format belongs to the core crate. Until the core `project` module lands, the
//! [`adapter`] below reads and writes a plain JSON document with the same fields.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

pub const PROJECT_EXT: &str = "ad3d";
pub const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp"];

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

/// Stand-in for the core project module (docs/spec/FEATURES.md APP-FILES: `.ad3d` = zip of
/// JSON parameters + input assets). This adapter writes the JSON part only and references the
/// input image by path.
///
// MERGE: wire to ad3d_core::project::{read, write} (zip container with embedded assets);
// keep `ProjectDoc` field names so the UI contract does not change.
pub mod adapter {
    use super::*;

    pub const FORMAT: &str = "ad3d.project";
    pub const FORMAT_VERSION: u32 = 1;

    pub fn format_tag() -> String {
        FORMAT.into()
    }
    pub fn format_version() -> u32 {
        FORMAT_VERSION
    }

    pub fn encode(doc: &ProjectDoc) -> Result<Vec<u8>, String> {
        let mut doc = doc.clone();
        doc.format = FORMAT.into();
        doc.version = FORMAT_VERSION;
        if doc.app_version.is_empty() {
            doc.app_version = env!("CARGO_PKG_VERSION").into();
        }
        validate(&doc)?;
        serde_json::to_vec_pretty(&doc).map_err(|e| e.to_string())
    }

    pub fn decode(bytes: &[u8]) -> Result<ProjectDoc, String> {
        if bytes.starts_with(b"PK\x03\x04") {
            return Err("this project uses the zip container, which this build cannot read yet".into());
        }
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

    fn validate(doc: &ProjectDoc) -> Result<(), String> {
        if doc.generator.trim().is_empty() {
            return Err("project has no generator".into());
        }
        if !doc.params.is_object() {
            return Err("project params must be an object".into());
        }
        Ok(())
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
        assert_eq!(opened.doc.app_version, env!("CARGO_PKG_VERSION"));
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
        assert!(adapter::decode(b"PK\x03\x04rest").unwrap_err().contains("zip"));
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
