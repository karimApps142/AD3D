import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { buildProjectDoc, documentName, paramsFromDoc, pathKind, suggestedName, useDocumentStore, windowTitle } from '../state/document';
import { paramsHistory, startParamsHistory } from '../state/paramsHistory';
import { DEFAULT_SETTINGS, formatLength, normaliseSettings, pushRecent, useSettingsStore } from '../state/settings';
import { DEFAULT_PARAMS, DEFAULT_VIEW, useAppStore } from '../state/store';
import { type UnsavedChoice, useUiStore } from '../state/ui';
import * as actions from './actions';
import { createBrowserHost, type Host, type KeyValue, setHost } from './host';

function memoryStorage(): KeyValue {
  const m = new Map<string, string>();
  return { getItem: (k) => m.get(k) ?? null, setItem: (k, v) => void m.set(k, v) };
}

let answers: (string | null)[] = [];
let host: Host;
let stop: () => void;

/** Answers the next unsaved-changes prompt. */
function answerPrompt(choice: UnsavedChoice): void {
  const off = useUiStore.subscribe((s) => {
    if (s.unsaved) {
      off();
      s.unsaved.resolve(choice);
    }
  });
}

beforeEach(() => {
  answers = [];
  host = createBrowserHost({ storage: memoryStorage(), ask: () => answers.shift() ?? null });
  setHost(host);
  useAppStore.setState({ params: DEFAULT_PARAMS, view: DEFAULT_VIEW });
  useDocumentStore.setState({ path: null, savedParams: DEFAULT_PARAMS });
  useSettingsStore.setState({ settings: DEFAULT_SETTINGS });
  useUiStore.setState({ unsaved: null, toasts: [] });
  stop = startParamsHistory();
});

afterEach(() => {
  stop();
  setHost(null);
});

const setParam = useAppStore.getState().setParam;

describe('project flows', () => {
  it('save as, edit, save, reopen', async () => {
    setParam('widthMm', 196);
    expect(actions.currentDirty()).toBe(true);
    answers.push('mona');
    expect(await actions.saveProject()).toBe(true);
    expect(useDocumentStore.getState().path).toBe('browser:/mona.ad3d');
    expect(actions.currentDirty()).toBe(false);
    expect(useSettingsStore.getState().settings.recentFiles).toEqual(['browser:/mona.ad3d']);

    setParam('cellMm', 4);
    expect(await actions.saveProject()).toBe(true); // no dialog: already has a path
    await actions.newProject();
    expect(useAppStore.getState().params).toEqual(DEFAULT_PARAMS);
    expect(useDocumentStore.getState().path).toBeNull();

    await actions.openPath('browser:/mona.ad3d');
    expect(useAppStore.getState().params.widthMm).toBe(196);
    expect(useAppStore.getState().params.cellMm).toBe(4);
    expect(actions.currentDirty()).toBe(false);
    expect(paramsHistory.canUndo).toBe(false);
  });

  it('cancel in the save dialog keeps the document', async () => {
    setParam('widthMm', 100);
    answers.push(null);
    expect(await actions.saveProjectAs()).toBe(false);
    expect(useDocumentStore.getState().path).toBeNull();
    expect(actions.currentDirty()).toBe(true);
  });

  it('new project with unsaved changes asks first: cancel keeps them', async () => {
    setParam('widthMm', 111);
    answerPrompt('cancel');
    await actions.newProject();
    expect(useAppStore.getState().params.widthMm).toBe(111);
  });

  it('… discard drops them', async () => {
    setParam('widthMm', 111);
    answerPrompt('discard');
    await actions.newProject();
    expect(useAppStore.getState().params.widthMm).toBe(DEFAULT_PARAMS.widthMm);
  });

  it('… save saves, then continues', async () => {
    setParam('widthMm', 111);
    answers.push('kept');
    answerPrompt('save');
    await actions.newProject();
    expect(useAppStore.getState().params.widthMm).toBe(DEFAULT_PARAMS.widthMm);
    const opened = await host.readProject('browser:/kept.ad3d');
    expect(opened.doc.params.widthMm).toBe(111);
  });

  it('close asks about unsaved changes and only then confirms', async () => {
    const confirm = vi.spyOn(host, 'confirmClose');
    setParam('widthMm', 222);
    answerPrompt('cancel');
    await actions.requestClose();
    expect(confirm).not.toHaveBeenCalled();
    answerPrompt('discard');
    await actions.requestClose();
    expect(confirm).toHaveBeenCalledTimes(1);
  });

  it('undoing back to the saved state is clean again', async () => {
    setParam('cellMm', 4);
    expect(actions.currentDirty()).toBe(true);
    paramsHistory.undo();
    expect(actions.currentDirty()).toBe(false);
  });

  it('opening a broken project reports an error and keeps the document', async () => {
    setParam('widthMm', 150);
    useDocumentStore.setState({ savedParams: useAppStore.getState().params }); // clean, so no prompt
    await actions.openPath('browser:/missing.ad3d');
    expect(useUiStore.getState().toasts.at(-1)?.kind).toBe('error');
    expect(useAppStore.getState().params.widthMm).toBe(150);
  });

  it('images become the current image (one undo step)', async () => {
    await actions.openPath('/photos/mona.JPG');
    expect(useAppStore.getState().params.image).toBe('/photos/mona.JPG');
    paramsHistory.undo();
    expect(useAppStore.getState().params.image).toBe('synthetic');
  });

  it('unknown file types are refused', async () => {
    await actions.openPath('/x/readme.txt');
    expect(useUiStore.getState().toasts.at(-1)?.kind).toBe('error');
  });

  it('settings round-trip through the host and clear recent', async () => {
    await actions.updateSettings({ theme: 'light', unitsDisplay: 'inch' });
    await actions.loadSettings();
    expect(useSettingsStore.getState().settings.theme).toBe('light');
    await host.addRecent('browser:/a.ad3d');
    await actions.loadSettings();
    expect(useSettingsStore.getState().settings.recentFiles).toEqual(['browser:/a.ad3d']);
    await actions.clearRecent();
    expect(useSettingsStore.getState().settings.recentFiles).toEqual([]);
  });

  it('export goes through the registered handler', async () => {
    const fn = vi.fn();
    actions.setExportHandler(fn);
    await actions.exportModel();
    expect(fn).toHaveBeenCalled();
    actions.setExportHandler(null);
    await actions.exportModel();
    expect(useUiStore.getState().toasts.length).toBeGreaterThan(0);
  });
});

describe('document helpers', () => {
  it('names and titles', () => {
    expect(documentName(null)).toBe('Untitled');
    expect(documentName('/a/b/plate.ad3d')).toBe('plate.ad3d');
    expect(documentName('C:\\x\\y.ad3d')).toBe('y.ad3d');
    expect(windowTitle('/a/plate.ad3d', true)).toBe('plate.ad3d — Edited — AD3D');
    expect(suggestedName(null, { ...DEFAULT_PARAMS, image: '/p/MonaLisa.jpg' })).toBe('MonaLisa.ad3d');
    expect(suggestedName(null, DEFAULT_PARAMS)).toBe('Untitled.ad3d');
    expect(pathKind('/a/b.AD3D')).toBe('project');
    expect(pathKind('/a/b.webp')).toBe('image');
    expect(pathKind('/a/b')).toBe('unknown');
  });

  it('paramsFromDoc fills defaults, keeps unknown fields and rejects bad input', () => {
    const doc = buildProjectDoc({ params: DEFAULT_PARAMS, view: DEFAULT_VIEW }, 'creality-cr6-se');
    expect(doc.generator).toBe('shadow_plate');
    expect(paramsFromDoc(doc)).toEqual(DEFAULT_PARAMS);
    const partial = { ...doc, params: { widthMm: 100, layout: { kind: 'hex' } } as never };
    const p = paramsFromDoc(partial) as unknown as Record<string, unknown>;
    expect(p.widthMm).toBe(100);
    expect(p.cellMm).toBe(DEFAULT_PARAMS.cellMm);
    expect(p.layout).toEqual({ kind: 'hex' });
    expect(() => paramsFromDoc({ ...doc, generator: 'lithophane' })).toThrow(/generator/);
    expect(() => paramsFromDoc({ ...doc, params: { ...DEFAULT_PARAMS, widthMm: 'x' } as never })).toThrow(/widthMm/);
  });
});

describe('settings helpers', () => {
  it('normalises bad values', () => {
    const s = normaliseSettings({ theme: 'neon' as never, undoDepth: 3, recentMax: 2, recentFiles: ['a', 'a', '', 'b', 'c'], defaultPrinter: 'x' });
    expect(s.theme).toBe('system');
    expect(s.undoDepth).toBe(10);
    expect(s.recentFiles).toEqual(['a', 'b']);
    expect(s.defaultPrinter).toBe('creality-cr6-se');
  });

  it('pushRecent moves to front and caps', () => {
    let s = { ...DEFAULT_SETTINGS, recentMax: 2 };
    s = pushRecent(pushRecent(pushRecent(s, '1'), '2'), '1');
    expect(s.recentFiles).toEqual(['1', '2']);
    s = pushRecent(s, '3');
    expect(s.recentFiles).toEqual(['3', '1']);
  });

  it('formats lengths stored in mm', () => {
    expect(formatLength(254, 'mm')).toBe('254 mm');
    expect(formatLength(254, 'cm')).toBe('25.4 cm');
    expect(formatLength(254, 'inch')).toBe('10 in');
  });
});
