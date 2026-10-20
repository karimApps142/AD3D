//! App settings (APP-SETTINGS): versioned JSON in the per-user app-config folder.
//!
//! File: `<app_config_dir>/settings.json` (macOS: `~/Library/Application Support/dev.ad3d.app/`).
//! Every field has a default, unknown fields are ignored and a file from an older schema is
//! migrated on load, so a missing, old or corrupt file never stops the app from starting. A
//! corrupt file is kept beside the new one as `settings.json.bad` so nothing is silently lost.
//!
//! Writes are atomic (temp file + rename) and happen off the UI thread.
//!
//! Commands (mirrors `src/app/host.ts`):
//! - `settings_load`                 → `Settings`
//! - `settings_save { settings }`    → `Settings` (normalised, as written)
//! - `recent_add { path }`           → `Settings` (path moved to the front of the recent list)
//! - `recent_clear`                  → `Settings`
//! - `printer_profiles_list`         → `PrinterProfile[]`

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};

/// Current settings schema. Bump it and extend [`migrate`] when a field changes meaning.
pub const SCHEMA_VERSION: u32 = 1;
pub const SETTINGS_FILE: &str = "settings.json";
pub const RECENT_MAX_LIMIT: u32 = 50;
pub const UNDO_DEPTH_MIN: u32 = 10;
pub const UNDO_DEPTH_MAX: u32 = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Dark,
    Light,
}

/// How lengths are *shown*. Everything is stored and computed in millimetres (CLAUDE.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum UnitsDisplay {
    #[default]
    Mm,
    Cm,
    Inch,
}

/// A build volume the export can tile and orient for (APP-MACHINES, M1 subset).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrinterProfile {
    pub id: String,
    pub name: String,
    /// Usable build volume X × Y × Z (mm).
    pub bed_mm: [f64; 3],
    /// Axis the bed moves along on a bed-slinger (`"y"`), or `None` for a moving gantry.
    pub bed_moves: Option<String>,
}

pub const DEFAULT_PRINTER: &str = "creality-cr6-se";

/// Built-in profiles. The CR-6 SE is the owner's printer (docs/START_HERE.md).
pub fn printer_profiles() -> Vec<PrinterProfile> {
    vec![
        PrinterProfile {
            id: DEFAULT_PRINTER.into(),
            name: "Creality CR-6 SE".into(),
            bed_mm: [235.0, 235.0, 250.0],
            bed_moves: Some("y".into()),
        },
        PrinterProfile {
            id: "generic-220".into(),
            name: "Generic 220 × 220 × 250".into(),
            bed_mm: [220.0, 220.0, 250.0],
            bed_moves: Some("y".into()),
        },
        PrinterProfile {
            id: "generic-256".into(),
            name: "Generic 256 × 256 × 256".into(),
            bed_mm: [256.0, 256.0, 256.0],
            bed_moves: None,
        },
    ]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub schema_version: u32,
    pub theme: Theme,
    pub units_display: UnitsDisplay,
    pub default_printer: String,
    /// Most recent first; absolute paths of `.ad3d` projects.
    pub recent_files: Vec<String>,
    pub recent_max: u32,
    pub undo_depth: u32,
    /// Open the help drawer on the first launch only.
    pub show_help_on_start: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            theme: Theme::System,
            units_display: UnitsDisplay::Mm,
            default_printer: DEFAULT_PRINTER.into(),
            recent_files: Vec::new(),
            recent_max: 10,
            undo_depth: 200,
            show_help_on_start: true,
        }
    }
}

impl Settings {
    /// Clamps ranges, drops empty/duplicate recent entries and unknown printers.
    pub fn normalised(mut self) -> Self {
        self.schema_version = SCHEMA_VERSION;
        self.recent_max = self.recent_max.min(RECENT_MAX_LIMIT);
        self.undo_depth = self.undo_depth.clamp(UNDO_DEPTH_MIN, UNDO_DEPTH_MAX);
        if !printer_profiles().iter().any(|p| p.id == self.default_printer) {
            self.default_printer = DEFAULT_PRINTER.into();
        }
        let mut seen = std::collections::HashSet::new();
        self.recent_files.retain(|p| !p.trim().is_empty() && seen.insert(p.clone()));
        self.recent_files.truncate(self.recent_max as usize);
        self
    }

    /// Moves `path` to the front of the recent list.
    pub fn push_recent(&mut self, path: &str) {
        self.recent_files.retain(|p| p != path);
        self.recent_files.insert(0, path.to_owned());
        self.recent_files.truncate(self.recent_max as usize);
    }
}

/// Upgrades any earlier JSON shape to the current [`Settings`].
///
/// Schema 0 (pre-release drafts) used snake_case keys and `recent` for the list.
pub fn migrate(mut v: Value) -> Settings {
    let version = v.get("schemaVersion").and_then(Value::as_u64).unwrap_or(0);
    if version == 0 {
        if let Value::Object(map) = &mut v {
            for (old, new) in [
                ("recent", "recentFiles"),
                ("recent_files", "recentFiles"),
                ("units", "unitsDisplay"),
                ("units_display", "unitsDisplay"),
                ("default_printer", "defaultPrinter"),
                ("undo_depth", "undoDepth"),
                ("recent_max", "recentMax"),
            ] {
                if let Some(x) = map.remove(old) {
                    map.entry(new.to_owned()).or_insert(x);
                }
            }
        }
    }
    // Field by field, so one bad value resets only that field.
    let mut s = Settings::default();
    if let Value::Object(map) = &v {
        let d = serde_json::to_value(Settings::default()).expect("settings serialise");
        let mut merged = d.as_object().cloned().unwrap_or_default();
        for (k, val) in map {
            if !merged.contains_key(k) {
                continue;
            }
            let mut trial = merged.clone();
            trial.insert(k.clone(), val.clone());
            if serde_json::from_value::<Settings>(Value::Object(trial)).is_ok() {
                merged.insert(k.clone(), val.clone());
            }
        }
        if let Ok(parsed) = serde_json::from_value::<Settings>(Value::Object(merged)) {
            s = parsed;
        }
    }
    s.normalised()
}

/// Reads settings from `path`. Missing → defaults. Unparseable → defaults, and the bad file is
/// renamed to `<name>.bad` so the next save does not destroy it unseen.
pub fn load_from(path: &Path) -> Settings {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return Settings::default(),
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(v) if v.is_object() => migrate(v),
        _ => {
            let bad = path.with_extension("json.bad");
            let _ = std::fs::rename(path, &bad);
            eprintln!("[ad3d] settings file unreadable; kept as {}", bad.display());
            Settings::default()
        }
    }
}

/// Writes `bytes` to `path` atomically (temp file in the same folder, then rename).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or_else(|| format!("{}: no parent folder", path.display()))?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

pub fn save_to(path: &Path, s: &Settings) -> Result<(), String> {
    let text = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    write_atomic(path, text.as_bytes())
}

/// In-memory copy plus the file it lives in. One per app.
pub struct SettingsStore {
    path: Option<PathBuf>,
    current: Mutex<Option<Settings>>,
}

impl SettingsStore {
    pub fn new(app: &AppHandle) -> Self {
        let path = app.path().app_config_dir().ok().map(|d| d.join(SETTINGS_FILE));
        Self { path, current: Mutex::new(None) }
    }

    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self { path: Some(path), current: Mutex::new(None) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Settings>> {
        self.current.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn get(&self) -> Settings {
        let mut cur = self.lock();
        if cur.is_none() {
            *cur = Some(self.path.as_deref().map(load_from).unwrap_or_default());
        }
        cur.clone().unwrap_or_default()
    }

    /// Applies `f` to the current settings, persists and returns the result.
    pub fn update(&self, f: impl FnOnce(&mut Settings)) -> Result<Settings, String> {
        let mut s = self.get();
        f(&mut s);
        let s = s.normalised();
        if let Some(p) = &self.path {
            save_to(p, &s)?;
        }
        *self.lock() = Some(s.clone());
        Ok(s)
    }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| format!("worker failed: {e}"))?
}

fn store(app: &AppHandle) -> std::sync::Arc<SettingsStore> {
    app.state::<std::sync::Arc<SettingsStore>>().inner().clone()
}

fn apply_theme(app: &AppHandle, theme: Theme) {
    let t = match theme {
        Theme::System => None,
        Theme::Dark => Some(tauri::Theme::Dark),
        Theme::Light => Some(tauri::Theme::Light),
    };
    for w in app.webview_windows().values() {
        let _ = w.set_theme(t);
    }
}

#[tauri::command]
pub async fn settings_load(app: AppHandle) -> Result<Settings, String> {
    let st = store(&app);
    let s = blocking(move || Ok(st.get())).await?;
    apply_theme(&app, s.theme);
    Ok(s)
}

#[tauri::command]
pub async fn settings_save(settings: Settings, app: AppHandle) -> Result<Settings, String> {
    let st = store(&app);
    let s = blocking(move || st.update(|s| *s = settings)).await?;
    apply_theme(&app, s.theme);
    crate::menu::set_recent(&app, &s.recent_files);
    Ok(s)
}

#[tauri::command]
pub async fn recent_add(path: String, app: AppHandle) -> Result<Settings, String> {
    let st = store(&app);
    let s = blocking(move || st.update(|s| s.push_recent(&path))).await?;
    crate::menu::set_recent(&app, &s.recent_files);
    Ok(s)
}

#[tauri::command]
pub async fn recent_clear(app: AppHandle) -> Result<Settings, String> {
    let st = store(&app);
    let s = blocking(move || st.update(|s| s.recent_files.clear())).await?;
    crate::menu::set_recent(&app, &s.recent_files);
    Ok(s)
}

#[tauri::command]
pub fn printer_profiles_list() -> Vec<PrinterProfile> {
    printer_profiles()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ad3d-settings-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn defaults_are_the_owners_setup() {
        let s = Settings::default();
        assert_eq!(s.default_printer, "creality-cr6-se");
        assert_eq!(s.theme, Theme::System);
        assert_eq!(s.units_display, UnitsDisplay::Mm);
        let cr6 = printer_profiles().into_iter().find(|p| p.id == s.default_printer).unwrap();
        assert_eq!(cr6.bed_mm, [235.0, 235.0, 250.0]);
        assert_eq!(cr6.bed_moves.as_deref(), Some("y"));
    }

    #[test]
    fn missing_file_gives_defaults() {
        let d = tmpdir("missing");
        assert_eq!(load_from(&d.join("settings.json")), Settings::default());
    }

    #[test]
    fn round_trip_through_disk() {
        let d = tmpdir("roundtrip");
        let p = d.join("settings.json");
        let mut s = Settings { theme: Theme::Light, units_display: UnitsDisplay::Inch, ..Default::default() };
        s.push_recent("/a/one.ad3d");
        s.push_recent("/a/two.ad3d");
        save_to(&p, &s).unwrap();
        let back = load_from(&p);
        assert_eq!(back, s);
        assert_eq!(back.recent_files, vec!["/a/two.ad3d", "/a/one.ad3d"]);
        // No temp files left behind.
        let names: Vec<_> = std::fs::read_dir(&d).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names.len(), 1, "{names:?}");
    }

    #[test]
    fn corrupt_file_is_kept_aside() {
        let d = tmpdir("corrupt");
        let p = d.join("settings.json");
        std::fs::write(&p, "{ not json").unwrap();
        assert_eq!(load_from(&p), Settings::default());
        assert!(d.join("settings.json.bad").exists());
        assert!(!p.exists());
    }

    #[test]
    fn bad_field_resets_only_that_field() {
        let s = migrate(json!({ "schemaVersion": 1, "theme": "purple", "unitsDisplay": "inch", "recentMax": 5 }));
        assert_eq!(s.theme, Theme::System);
        assert_eq!(s.units_display, UnitsDisplay::Inch);
        assert_eq!(s.recent_max, 5);
    }

    #[test]
    fn schema0_is_migrated() {
        let s = migrate(json!({ "theme": "dark", "recent": ["/x.ad3d", "/x.ad3d", ""], "units": "cm", "undo_depth": 5 }));
        assert_eq!(s.schema_version, SCHEMA_VERSION);
        assert_eq!(s.theme, Theme::Dark);
        assert_eq!(s.units_display, UnitsDisplay::Cm);
        assert_eq!(s.recent_files, vec!["/x.ad3d"]);
        assert_eq!(s.undo_depth, UNDO_DEPTH_MIN, "clamped");
    }

    #[test]
    fn unknown_printer_falls_back() {
        let s = migrate(json!({ "schemaVersion": 1, "defaultPrinter": "nope" }));
        assert_eq!(s.default_printer, DEFAULT_PRINTER);
    }

    #[test]
    fn recent_list_is_capped_and_deduplicated() {
        let mut s = Settings { recent_max: 3, ..Default::default() };
        for p in ["/1", "/2", "/3", "/2", "/4"] {
            s.push_recent(p);
        }
        assert_eq!(s.recent_files, vec!["/4", "/2", "/3"]);
    }

    #[test]
    fn store_update_persists() {
        let d = tmpdir("store");
        let p = d.join("cfg").join("settings.json");
        let st = SettingsStore::at(p.clone());
        let s = st.update(|s| s.push_recent("/p.ad3d")).unwrap();
        assert_eq!(s.recent_files, vec!["/p.ad3d"]);
        assert_eq!(load_from(&p).recent_files, vec!["/p.ad3d"]);
        let s = st.update(|s| s.recent_files.clear()).unwrap();
        assert!(s.recent_files.is_empty());
    }
}
