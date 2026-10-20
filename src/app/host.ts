// The desktop host as the app shell sees it: settings, project files, dialogs, the native menu
// and file drops. `createTauriHost` talks to src-tauri (menu.rs, project.rs, settings.rs);
// `createBrowserHost` keeps `pnpm dev` in a plain browser usable (localStorage, prompt()).
//
// Command contract (src-tauri):
//   settings_load / settings_save { settings } / recent_add { path } / recent_clear → Settings
//   printer_profiles_list                                   → PrinterProfile[]
//   pick_open_path { kind } / pick_save_path { suggestedName } / pick_folder → string | null
//   project_read { path } → OpenedProject;  project_write { path, doc } → string
//   menu_set_state { state: { canUndo, canRedo, title } };  confirm_close
//   event "menu" → { id, arg? }

import { isTauri } from '../engine';
import type { OpenedProject, ProjectDoc } from '../state/document';
import {
  BUILTIN_PRINTERS,
  normaliseSettings,
  type PrinterProfile,
  pushRecent,
  type Settings,
} from '../state/settings';

export interface MenuMessage {
  id: string;
  arg?: string;
}

export type DropEvent =
  | { type: 'enter' | 'over'; paths: string[] }
  | { type: 'leave' }
  | { type: 'drop'; paths: string[]; files?: File[] };

export interface MenuState {
  canUndo: boolean;
  canRedo: boolean;
  title: string;
}

export interface Host {
  readonly native: boolean;
  loadSettings(): Promise<Settings>;
  saveSettings(s: Settings): Promise<Settings>;
  addRecent(path: string): Promise<Settings>;
  clearRecent(): Promise<Settings>;
  printerProfiles(): Promise<PrinterProfile[]>;
  pickOpenPath(kind: 'any' | 'project' | 'image'): Promise<string | null>;
  pickSavePath(suggestedName: string): Promise<string | null>;
  pickFolder(): Promise<string | null>;
  readProject(path: string): Promise<OpenedProject>;
  writeProject(path: string, doc: ProjectDoc): Promise<string>;
  setMenuState(state: MenuState): Promise<void>;
  /** The user confirmed closing; the native host exits. */
  confirmClose(): Promise<void>;
  onMenu(cb: (m: MenuMessage) => void): Promise<() => void>;
  onDrop(cb: (e: DropEvent) => void): Promise<() => void>;
}

export function createTauriHost(): Host {
  // Lazy imports keep @tauri-apps/api out of tests and the browser path.
  const core = () => import('@tauri-apps/api/core');
  const call = async <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => (await core()).invoke<T>(cmd, args);

  return {
    native: true,
    loadSettings: () => call<Settings>('settings_load'),
    saveSettings: (settings) => call<Settings>('settings_save', { settings }),
    addRecent: (path) => call<Settings>('recent_add', { path }),
    clearRecent: () => call<Settings>('recent_clear'),
    printerProfiles: () => call<PrinterProfile[]>('printer_profiles_list'),
    pickOpenPath: (kind) => call<string | null>('pick_open_path', { kind }),
    pickSavePath: (suggestedName) => call<string | null>('pick_save_path', { suggestedName }),
    pickFolder: () => call<string | null>('pick_folder'),
    readProject: (path) => call<OpenedProject>('project_read', { path }),
    writeProject: (path, doc) => call<string>('project_write', { path, doc }),
    setMenuState: (state) => call<void>('menu_set_state', { state }),
    confirmClose: () => call<void>('confirm_close'),
    async onMenu(cb) {
      const { listen } = await import('@tauri-apps/api/event');
      return listen<MenuMessage>('menu', (e) => cb(e.payload));
    },
    async onDrop(cb) {
      const { getCurrentWebview } = await import('@tauri-apps/api/webview');
      return getCurrentWebview().onDragDropEvent((e) => {
        const p = e.payload;
        if (p.type === 'leave') cb({ type: 'leave' });
        else if (p.type === 'drop') cb({ type: 'drop', paths: p.paths });
        else if (p.type === 'enter') cb({ type: 'enter', paths: p.paths });
        else cb({ type: 'over', paths: [] });
      });
    },
  };
}

/** Minimal storage interface so the browser host is testable without a DOM. */
export interface KeyValue {
  getItem(k: string): string | null;
  setItem(k: string, v: string): void;
}

const SETTINGS_KEY = 'ad3d.settings';
const PROJECT_PREFIX = 'ad3d.project:';

function safeStorage(): KeyValue {
  const mem = new Map<string, string>();
  const memory: KeyValue = { getItem: (k) => mem.get(k) ?? null, setItem: (k, v) => void mem.set(k, v) };
  try {
    if (typeof localStorage !== 'undefined') {
      localStorage.getItem('');
      return {
        getItem: (k) => {
          try {
            return localStorage.getItem(k);
          } catch {
            return memory.getItem(k);
          }
        },
        setItem: (k, v) => {
          try {
            localStorage.setItem(k, v);
          } catch {
            memory.setItem(k, v);
          }
        },
      };
    }
  } catch {
    /* blocked storage: fall through */
  }
  return memory;
}

export interface BrowserHostOptions {
  storage?: KeyValue;
  /** Asks for a name (window.prompt in the browser). */
  ask?: (message: string, def: string) => string | null;
}

/**
 * Browser stand-in: settings and projects live in storage under "browser:/<name>.ad3d" paths.
 * Good enough to exercise every shell flow with `pnpm dev`.
 */
export function createBrowserHost(opts: BrowserHostOptions = {}): Host {
  const store = opts.storage ?? safeStorage();
  const ask = opts.ask ?? ((m: string, d: string) => (typeof window !== 'undefined' ? window.prompt(m, d) : null));
  const read = (): Settings => {
    try {
      return normaliseSettings(JSON.parse(store.getItem(SETTINGS_KEY) ?? 'null'));
    } catch {
      return normaliseSettings(null);
    }
  };
  const write = (s: Settings): Settings => {
    const n = normaliseSettings(s);
    store.setItem(SETTINGS_KEY, JSON.stringify(n));
    return n;
  };
  const toPath = (name: string) => `browser:/${name.endsWith('.ad3d') ? name : `${name}.ad3d`}`;

  return {
    native: false,
    loadSettings: async () => read(),
    saveSettings: async (s) => write(s),
    addRecent: async (path) => write(pushRecent(read(), path)),
    clearRecent: async () => write({ ...read(), recentFiles: [] }),
    printerProfiles: async () => BUILTIN_PRINTERS,
    async pickOpenPath(kind) {
      const name = ask(kind === 'image' ? 'Image URL or path' : 'Project name to open', '');
      if (!name) return null;
      return kind === 'image' ? name : toPath(name);
    },
    async pickSavePath(suggestedName) {
      const name = ask('Save project as', suggestedName);
      return name ? toPath(name) : null;
    },
    pickFolder: async () => null,
    async readProject(path) {
      const text = store.getItem(PROJECT_PREFIX + path);
      if (text === null) throw new Error(`${path}: no such project`);
      return { path, doc: JSON.parse(text) as ProjectDoc, warnings: [] };
    },
    async writeProject(path, doc) {
      const p = path.endsWith('.ad3d') ? path : `${path}.ad3d`;
      store.setItem(PROJECT_PREFIX + p, JSON.stringify({ format: 'ad3d.project', version: 1, ...doc }));
      return p;
    },
    async setMenuState(state) {
      if (typeof document !== 'undefined') document.title = state.title;
    },
    confirmClose: async () => {},
    onMenu: async () => () => {},
    async onDrop(cb) {
      if (typeof window === 'undefined') return () => {};
      let depth = 0;
      const enter = (e: DragEvent) => {
        if (!e.dataTransfer?.types.includes('Files')) return;
        e.preventDefault();
        if (depth++ === 0) cb({ type: 'enter', paths: [] });
      };
      const over = (e: DragEvent) => {
        if (e.dataTransfer?.types.includes('Files')) e.preventDefault();
      };
      const leave = () => {
        if (depth > 0 && --depth === 0) cb({ type: 'leave' });
      };
      const drop = (e: DragEvent) => {
        e.preventDefault();
        depth = 0;
        const files = [...(e.dataTransfer?.files ?? [])];
        cb({ type: 'drop', paths: files.map((f) => f.name), files });
      };
      window.addEventListener('dragenter', enter);
      window.addEventListener('dragover', over);
      window.addEventListener('dragleave', leave);
      window.addEventListener('drop', drop);
      return () => {
        window.removeEventListener('dragenter', enter);
        window.removeEventListener('dragover', over);
        window.removeEventListener('dragleave', leave);
        window.removeEventListener('drop', drop);
      };
    },
  };
}

let host: Host | null = null;

export function getHost(): Host {
  host ??= isTauri() ? createTauriHost() : createBrowserHost();
  return host;
}

/** Tests inject a fake host. */
export function setHost(h: Host | null): void {
  host = h;
}
