//! Native menu bar (APP-SHORTCUTS, APP-FILES) and the unsaved-changes close guard.
//!
//! Menu items carry command ids that match the UI's command registry (`src/commands/`); a click
//! or accelerator emits the `menu` event `{ id, arg }` and the UI runs the command. Only
//! modifier accelerators live here: single-letter viewport keys (F, E, …) are handled by the UI
//! so they never swallow typing in text fields.
//!
//! Closing the window or quitting asks the UI first (it may hold unsaved changes): the request
//! is held and `app.closeRequested` is emitted; the UI answers with `confirm_close` once the user
//! has saved or discarded. Until the UI has said it is ready (first `menu_set_state`), and in
//! spike mode, closing is never blocked.
//!
//! Commands (mirrors `src/app/host.ts`):
//! - `menu_set_state { state: { canUndo, canRedo, title } }` → ()
//! - `confirm_close`                                       → () (the app exits)

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use tauri::menu::{
    AboutMetadata, Menu, MenuEvent, MenuItem, MenuItemBuilder, PredefinedMenuItem, Submenu, SubmenuBuilder,
};
use tauri::{AppHandle, Emitter, Manager, RunEvent, Window, WindowEvent, Wry};

/// Name of the event the UI listens to.
pub const MENU_EVENT: &str = "menu";
/// Emitted (as a menu id) when the user asks to close the window or quit.
pub const CLOSE_REQUESTED: &str = "app.closeRequested";

/// Every command id the menu can emit. Kept in sync with `src/commands/ids.ts` (a vitest test
/// reads this file and checks each id is registered).
pub const MENU_IDS: &[&str] = &[
    "help.about",
    "app.settings",
    "app.quit",
    "file.new",
    "file.open",
    "file.openRecent",
    "file.clearRecent",
    "file.save",
    "file.saveAs",
    "file.importImage",
    "file.export",
    "file.close",
    "edit.undo",
    "edit.redo",
    "view.commandPalette",
    "view.frame",
    "view.theme.system",
    "view.theme.dark",
    "view.theme.light",
    "help.open",
    "help.shortcuts",
    CLOSE_REQUESTED,
];

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MenuPayload {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arg: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct MenuState {
    pub can_undo: bool,
    pub can_redo: bool,
    /// Window title, e.g. "plate.ad3d — AD3D" (the UI adds the edited marker).
    pub title: Option<String>,
}

struct Handles {
    undo: MenuItem<Wry>,
    redo: MenuItem<Wry>,
    recent: Submenu<Wry>,
}

/// Per-app menu and close-guard state.
#[derive(Default)]
pub struct MenuShell {
    handles: Mutex<Option<Handles>>,
    recent_paths: Mutex<Vec<String>>,
    /// The UI has registered its listeners and can answer a close request.
    ui_ready: AtomicBool,
    /// The UI confirmed; the next close/exit goes through.
    allow_exit: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn shell(app: &AppHandle) -> std::sync::Arc<MenuShell> {
    app.state::<std::sync::Arc<MenuShell>>().inner().clone()
}

/// "plate.ad3d — Projects/2026" style label for a recent file.
pub fn recent_label(path: &str) -> String {
    let p = std::path::Path::new(path);
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_owned());
    match p.parent().and_then(|d| d.file_name()) {
        Some(dir) => format!("{name} — {}", dir.to_string_lossy()),
        None => name,
    }
}

/// Maps a clicked menu item id to the payload sent to the UI.
pub fn payload_for(id: &str, recent: &[String]) -> Option<MenuPayload> {
    if let Some(i) = id.strip_prefix("file.recent.") {
        let path = recent.get(i.parse::<usize>().ok()?)?.clone();
        return Some(MenuPayload { id: "file.openRecent".into(), arg: Some(path) });
    }
    MENU_IDS.contains(&id).then(|| MenuPayload { id: id.to_owned(), arg: None })
}

fn item(app: &AppHandle, id: &str, label: &str, accel: Option<&str>) -> tauri::Result<MenuItem<Wry>> {
    let mut b = MenuItemBuilder::with_id(id, label);
    if let Some(a) = accel {
        b = b.accelerator(a);
    }
    b.build(app)
}

fn build(app: &AppHandle) -> tauri::Result<(Menu<Wry>, Handles)> {
    let about = PredefinedMenuItem::about(
        app,
        Some("About AD3D"),
        Some(AboutMetadata {
            name: Some("AD3D".into()),
            version: Some(env!("CARGO_PKG_VERSION").into()),
            comments: Some(format!("Generative fabrication design ({})", std::env::consts::ARCH)),
            ..Default::default()
        }),
    )?;
    let app_menu = SubmenuBuilder::new(app, "AD3D")
        .item(&about)
        .separator()
        .item(&item(app, "app.settings", "Settings…", Some("CmdOrCtrl+,"))?)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .item(&item(app, "app.quit", "Quit AD3D", Some("CmdOrCtrl+Q"))?)
        .build()?;

    let recent = SubmenuBuilder::with_id(app, "file.recentMenu", "Open Recent").build()?;
    let file = SubmenuBuilder::new(app, "File")
        .item(&item(app, "file.new", "New", Some("CmdOrCtrl+N"))?)
        .item(&item(app, "file.open", "Open…", Some("CmdOrCtrl+O"))?)
        .item(&recent)
        .separator()
        .item(&item(app, "file.save", "Save", Some("CmdOrCtrl+S"))?)
        .item(&item(app, "file.saveAs", "Save As…", Some("CmdOrCtrl+Shift+S"))?)
        .separator()
        .item(&item(app, "file.importImage", "Import Image…", Some("CmdOrCtrl+I"))?)
        .item(&item(app, "file.export", "Export…", Some("CmdOrCtrl+E"))?)
        .separator()
        .item(&item(app, "file.close", "Close Window", Some("CmdOrCtrl+W"))?)
        .build()?;

    let undo = item(app, "edit.undo", "Undo", Some("CmdOrCtrl+Z"))?;
    let redo = item(app, "edit.redo", "Redo", Some("CmdOrCtrl+Shift+Z"))?;
    undo.set_enabled(false)?;
    redo.set_enabled(false)?;
    let edit = SubmenuBuilder::new(app, "Edit")
        .item(&undo)
        .item(&redo)
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;

    let theme = SubmenuBuilder::new(app, "Theme")
        .item(&item(app, "view.theme.system", "System", None)?)
        .item(&item(app, "view.theme.dark", "Dark", None)?)
        .item(&item(app, "view.theme.light", "Light", None)?)
        .build()?;
    let view = SubmenuBuilder::new(app, "View")
        .item(&item(app, "view.commandPalette", "Command Palette…", Some("CmdOrCtrl+K"))?)
        .item(&item(app, "view.frame", "Frame Model  (F)", None)?)
        .separator()
        .item(&theme)
        .separator()
        .fullscreen()
        .build()?;

    let window = SubmenuBuilder::new(app, "Window").minimize().maximize().build()?;

    let help = SubmenuBuilder::new(app, "Help")
        .item(&item(app, "help.open", "AD3D Help", Some("CmdOrCtrl+Shift+/"))?)
        .item(&item(app, "help.shortcuts", "Keyboard Shortcuts", None)?)
        .build()?;

    let menu = Menu::with_items(app, &[&app_menu, &file, &edit, &view, &window, &help])?;
    Ok((menu, Handles { undo, redo, recent }))
}

fn fill_recent(app: &AppHandle, sub: &Submenu<Wry>, paths: &[String]) -> tauri::Result<()> {
    for old in sub.items()? {
        sub.remove(&old)?;
    }
    if paths.is_empty() {
        let none = item(app, "file.recent.none", "No Recent Projects", None)?;
        none.set_enabled(false)?;
        sub.append(&none)?;
        return Ok(());
    }
    for (i, p) in paths.iter().enumerate() {
        sub.append(&item(app, &format!("file.recent.{i}"), &recent_label(p), None)?)?;
    }
    sub.append(&PredefinedMenuItem::separator(app)?)?;
    sub.append(&item(app, "file.clearRecent", "Clear Menu", None)?)?;
    Ok(())
}

/// Rebuilds File → Open Recent. Called whenever the settings' recent list changes.
pub fn set_recent(app: &AppHandle, paths: &[String]) {
    let sh = shell(app);
    *lock(&sh.recent_paths) = paths.to_vec();
    if let Some(h) = lock(&sh.handles).as_ref() {
        if let Err(e) = fill_recent(app, &h.recent, paths) {
            eprintln!("[ad3d] recent menu: {e}");
        }
    }
}

fn emit(app: &AppHandle, p: MenuPayload) {
    if let Err(e) = app.emit(MENU_EVENT, p) {
        eprintln!("[ad3d] menu event: {e}");
    }
}

fn on_menu(app: &AppHandle, event: MenuEvent) {
    let recent = lock(&shell(app).recent_paths).clone();
    if let Some(p) = payload_for(event.id().as_ref(), &recent) {
        emit(app, p);
    }
}

/// Installs the menu. Call from `setup`; needs [`MenuShell`] managed first (see [`manage`]).
pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let (menu, handles) = build(app)?;
    app.set_menu(menu)?;
    app.on_menu_event(on_menu);
    let recent = app.state::<std::sync::Arc<crate::settings::SettingsStore>>().get().recent_files;
    fill_recent(app, &handles.recent, &recent)?;
    let sh = shell(app);
    *lock(&sh.recent_paths) = recent;
    *lock(&sh.handles) = Some(handles);
    Ok(())
}

/// Registers the managed state this module and `settings` need.
pub fn manage(app: &AppHandle) {
    app.manage(std::sync::Arc::new(MenuShell::default()));
    app.manage(std::sync::Arc::new(crate::settings::SettingsStore::new(app)));
}

fn guard_active(app: &AppHandle) -> bool {
    if crate::spike_mode() {
        return false;
    }
    let sh = shell(app);
    sh.ui_ready.load(Ordering::SeqCst) && !sh.allow_exit.load(Ordering::SeqCst)
}

/// Window-event hook: holds a close request until the UI confirms.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        let app = window.app_handle();
        if guard_active(app) {
            api.prevent_close();
            emit(app, MenuPayload { id: CLOSE_REQUESTED.into(), arg: None });
        }
    }
}

/// Run-event hook: a user-initiated quit (Dock, logout) is held the same way.
pub fn on_run_event(app: &AppHandle, event: &RunEvent) {
    if let RunEvent::ExitRequested { code: None, api, .. } = event {
        if guard_active(app) {
            api.prevent_exit();
            emit(app, MenuPayload { id: CLOSE_REQUESTED.into(), arg: None });
        }
    }
}

#[tauri::command]
pub fn menu_set_state(state: MenuState, app: AppHandle) -> Result<(), String> {
    let sh = shell(&app);
    sh.ui_ready.store(true, Ordering::SeqCst);
    if let Some(h) = lock(&sh.handles).as_ref() {
        h.undo.set_enabled(state.can_undo).map_err(|e| e.to_string())?;
        h.redo.set_enabled(state.can_redo).map_err(|e| e.to_string())?;
    }
    if let (Some(title), Some(w)) = (state.title, app.get_webview_window("main")) {
        w.set_title(&title).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn confirm_close(app: AppHandle) {
    shell(&app).allow_exit.store(true, Ordering::SeqCst);
    app.exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_ids_map_to_paths() {
        let recent = vec!["/a/x.ad3d".to_owned(), "/b/y.ad3d".to_owned()];
        assert_eq!(
            payload_for("file.recent.1", &recent),
            Some(MenuPayload { id: "file.openRecent".into(), arg: Some("/b/y.ad3d".into()) })
        );
        assert_eq!(payload_for("file.recent.7", &recent), None);
        assert_eq!(payload_for("file.recent.none", &recent), None);
        assert_eq!(payload_for("file.save", &recent), Some(MenuPayload { id: "file.save".into(), arg: None }));
        assert_eq!(payload_for("bogus", &recent), None);
    }

    #[test]
    fn labels() {
        assert_eq!(recent_label("/Users/me/Plates/mona.ad3d"), "mona.ad3d — Plates");
        assert_eq!(recent_label("mona.ad3d"), "mona.ad3d");
    }

    #[test]
    fn ids_are_unique() {
        let mut ids = MENU_IDS.to_vec();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), MENU_IDS.len());
    }
}
