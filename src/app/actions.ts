// File and app actions behind the menu, palette and shortcuts. Each flow asks about unsaved
// changes first where it would discard them, and reports problems as toasts instead of throwing.

import { documentName, buildProjectDoc, isDirty, paramsFromDoc, pathKind, suggestedName, useDocumentStore, viewFromDoc } from '../state/document';
import { paramsHistory } from '../state/paramsHistory';
import { normaliseSettings, type Settings, type Theme, useSettingsStore } from '../state/settings';
import { DEFAULT_PARAMS, DEFAULT_VIEW, useAppStore } from '../state/store';
import { askUnsaved, toast } from '../state/ui';
import { getEngine } from '../engine';
import { useSquamaUi } from '../panels/squamaUi';
import { getHost } from './host';

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export function currentDirty(): boolean {
  return isDirty(useAppStore.getState().params, useDocumentStore.getState());
}

function setSettings(s: Settings): void {
  useSettingsStore.setState({ settings: normaliseSettings(s), loaded: true });
  paramsHistory.setLimit(s.undoDepth);
}

/** Resolves true when it is fine to discard the current document (saved, discarded or clean). */
export async function guardUnsaved(): Promise<boolean> {
  if (!currentDirty()) return true;
  const choice = await askUnsaved(documentName(useDocumentStore.getState().path));
  if (choice === 'cancel') return false;
  if (choice === 'discard') return true;
  return saveProject();
}

export async function newProject(): Promise<void> {
  if (!(await guardUnsaved())) return;
  useAppStore.getState().setView({ ...DEFAULT_VIEW, lightDragging: false });
  paramsHistory.load(DEFAULT_PARAMS);
  void loadImageInfo(DEFAULT_PARAMS.image);
  useDocumentStore.setState({ path: null, savedParams: DEFAULT_PARAMS });
  useAppStore.getState().requestFit();
}

/** Opens a project or image by path (dispatching on the extension). */
export async function openPath(path: string): Promise<void> {
  const kind = pathKind(path);
  if (kind === 'image') return useImage(path);
  if (kind !== 'project') {
    toast(`${documentName(path)}: not a project (.ad3d) or an image`, 'error');
    return;
  }
  if (!(await guardUnsaved())) return;
  const host = getHost();
  try {
    const opened = await host.readProject(path);
    const params = paramsFromDoc(opened.doc);
    const view = viewFromDoc(opened.doc);
    if (Object.keys(view).length) useAppStore.getState().setView(view);
    paramsHistory.load(params);
    useDocumentStore.setState({ path: opened.path, savedParams: params });
    useAppStore.getState().requestFit();
    for (const w of opened.warnings) toast(w);
    void loadImageInfo(params.image);
    await rememberRecent(opened.path);
  } catch (err) {
    toast(`Could not open ${documentName(path)}: ${message(err)}`, 'error', 8000);
  }
}

export async function openDialog(): Promise<void> {
  const path = await getHost().pickOpenPath('any');
  if (path) await openPath(path);
}

/**
 * Puts a photo into the current project: the one entry point for File → Open, Import, drop and
 * the Image panel's "Open photo…". A new photo starts uncropped, unrotated and with the height
 * from its aspect (one undo step), then its size is loaded so the panels and the point lamp use
 * the real plate size.
 */
export async function useImage(path: string): Promise<void> {
  const { imageOpts: _o, heightMm: _h, ...rest } = useAppStore.getState().params;
  useAppStore.setState({ params: { ...rest, image: path, quality: 'final' } });
  useAppStore.getState().requestFit();
  await loadImageInfo(path);
}

/** Loads (and caches in the core) the size of the project's photo for the panels. */
export async function loadImageInfo(path: string): Promise<void> {
  const ui = useSquamaUi.getState();
  if (path === 'synthetic') {
    ui.set({ image: null, imageError: null, imageLoading: false });
    return;
  }
  ui.set({ imageLoading: true, imageError: null });
  try {
    const engine = await getEngine();
    const info = await engine.imageInfo(path);
    // A later photo may have replaced this one meanwhile.
    if (useAppStore.getState().params.image === path) useSquamaUi.getState().set({ image: info, imageError: null });
  } catch (err) {
    if (useAppStore.getState().params.image === path) {
      useSquamaUi.getState().set({ imageError: message(err) });
      toast(`Could not open ${documentName(path)}: ${message(err)}`, 'error', 8000);
    }
  } finally {
    if (useAppStore.getState().params.image === path) useSquamaUi.getState().set({ imageLoading: false });
  }
}

export async function importImageDialog(): Promise<void> {
  const path = await getHost().pickOpenPath('image');
  if (path) await useImage(path);
}

async function writeTo(path: string): Promise<boolean> {
  const host = getHost();
  const state = useAppStore.getState();
  const params = state.params;
  try {
    const written = await host.writeProject(path, buildProjectDoc(state, useSettingsStore.getState().settings.defaultPrinter));
    useDocumentStore.setState({ path: written, savedParams: params });
    await rememberRecent(written);
    return true;
  } catch (err) {
    toast(`Could not save: ${message(err)}`, 'error', 8000);
    return false;
  }
}

/** Save; asks for a path when the project is untitled. Resolves true when saved. */
export async function saveProject(): Promise<boolean> {
  const path = useDocumentStore.getState().path;
  return path ? writeTo(path) : saveProjectAs();
}

export async function saveProjectAs(): Promise<boolean> {
  const doc = useDocumentStore.getState();
  const path = await getHost().pickSavePath(suggestedName(doc.path, useAppStore.getState().params));
  if (!path) return false;
  return writeTo(path);
}

async function rememberRecent(path: string): Promise<void> {
  try {
    setSettings(await getHost().addRecent(path));
  } catch (err) {
    console.warn('[shell] recent list not updated', err);
  }
}

export async function openRecent(path: string | undefined): Promise<void> {
  if (path) await openPath(path);
}

export async function clearRecent(): Promise<void> {
  try {
    setSettings(await getHost().clearRecent());
  } catch (err) {
    toast(`Could not clear the recent list: ${message(err)}`, 'error');
  }
}

export async function updateSettings(patch: Partial<Settings>): Promise<void> {
  const next = { ...useSettingsStore.getState().settings, ...patch };
  setSettings(next); // optimistic, so the UI reacts at once
  try {
    setSettings(await getHost().saveSettings(next));
  } catch (err) {
    toast(`Could not save settings: ${message(err)}`, 'error');
  }
}

export function setTheme(theme: Theme): Promise<void> {
  return updateSettings({ theme });
}

export async function loadSettings(): Promise<void> {
  const host = getHost();
  try {
    const [settings, printers] = await Promise.all([host.loadSettings(), host.printerProfiles()]);
    useSettingsStore.setState({ printers });
    setSettings(settings);
  } catch (err) {
    console.warn('[shell] settings not loaded; using defaults', err);
    useSettingsStore.setState({ loaded: true });
  }
}

/** Window close / quit: ask about unsaved changes, then let the host exit. */
export async function requestClose(): Promise<void> {
  if (!(await guardUnsaved())) return;
  await getHost().confirmClose();
}

// ---- Export slot -------------------------------------------------------------------------

export type ExportHandler = () => void | Promise<void>;
let exportHandler: ExportHandler | null = null;

/**
 * The export panel (panels/ExportPanel.tsx) registers its save-dialog-and-export flow here
 * (squama_export, docs/design/squama-mesher.md §12, §14.4); File → Export… and ⌘E call it.
 */
export function setExportHandler(fn: ExportHandler | null): void {
  exportHandler = fn;
}

export async function exportModel(): Promise<void> {
  if (exportHandler) return exportHandler();
  toast('Open the Tiling & export panel to export.');
}
