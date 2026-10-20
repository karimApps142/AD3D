// App settings (mirrors src-tauri/src/settings.rs). Persisted by the host in the app-config
// folder; this slice holds the loaded copy and the display helpers that depend on it.

import { create } from 'zustand';

export type Theme = 'system' | 'dark' | 'light';
export type UnitsDisplay = 'mm' | 'cm' | 'inch';

export interface Settings {
  schemaVersion: number;
  theme: Theme;
  unitsDisplay: UnitsDisplay;
  defaultPrinter: string;
  recentFiles: string[];
  recentMax: number;
  undoDepth: number;
  showHelpOnStart: boolean;
}

export interface PrinterProfile {
  id: string;
  name: string;
  /** Usable build volume X × Y × Z (mm). */
  bedMm: [number, number, number];
  bedMoves: string | null;
}

export const DEFAULT_PRINTER_ID = 'creality-cr6-se';

export const DEFAULT_SETTINGS: Settings = {
  schemaVersion: 1,
  theme: 'system',
  unitsDisplay: 'mm',
  defaultPrinter: DEFAULT_PRINTER_ID,
  recentFiles: [],
  recentMax: 10,
  undoDepth: 200,
  showHelpOnStart: true,
};

/** Same list as the Rust side; used when no host is available (browser dev). */
export const BUILTIN_PRINTERS: PrinterProfile[] = [
  { id: DEFAULT_PRINTER_ID, name: 'Creality CR-6 SE', bedMm: [235, 235, 250], bedMoves: 'y' },
  { id: 'generic-220', name: 'Generic 220 × 220 × 250', bedMm: [220, 220, 250], bedMoves: 'y' },
  { id: 'generic-256', name: 'Generic 256 × 256 × 256', bedMm: [256, 256, 256], bedMoves: null },
];

/** Client-side mirror of `Settings::normalised` (used by the browser host). */
export function normaliseSettings(raw: Partial<Settings> | null | undefined): Settings {
  const s = { ...DEFAULT_SETTINGS, ...(raw ?? {}) };
  if (!['system', 'dark', 'light'].includes(s.theme)) s.theme = 'system';
  if (!['mm', 'cm', 'inch'].includes(s.unitsDisplay)) s.unitsDisplay = 'mm';
  if (!BUILTIN_PRINTERS.some((p) => p.id === s.defaultPrinter)) s.defaultPrinter = DEFAULT_PRINTER_ID;
  s.recentMax = clampInt(s.recentMax, 0, 50, DEFAULT_SETTINGS.recentMax);
  s.undoDepth = clampInt(s.undoDepth, 10, 2000, DEFAULT_SETTINGS.undoDepth);
  const seen = new Set<string>();
  s.recentFiles = (Array.isArray(s.recentFiles) ? s.recentFiles : [])
    .filter((p): p is string => typeof p === 'string' && p.trim() !== '' && !seen.has(p) && !!seen.add(p))
    .slice(0, s.recentMax);
  s.schemaVersion = 1;
  return s;
}

function clampInt(v: unknown, lo: number, hi: number, fallback: number): number {
  const n = typeof v === 'number' && Number.isFinite(v) ? Math.floor(v) : fallback;
  return Math.min(hi, Math.max(lo, n));
}

export function pushRecent(s: Settings, path: string): Settings {
  const recentFiles = [path, ...s.recentFiles.filter((p) => p !== path)].slice(0, s.recentMax);
  return { ...s, recentFiles };
}

const UNIT_FACTOR: Record<UnitsDisplay, number> = { mm: 1, cm: 10, inch: 25.4 };
const UNIT_LABEL: Record<UnitsDisplay, string> = { mm: 'mm', cm: 'cm', inch: 'in' };

/** Formats a length given in millimetres in the chosen display unit. Storage stays in mm. */
export function formatLength(mm: number, units: UnitsDisplay, digits?: number): string {
  const v = mm / UNIT_FACTOR[units];
  const d = digits ?? (units === 'mm' ? 1 : units === 'cm' ? 2 : 3);
  return `${Number(v.toFixed(d))} ${UNIT_LABEL[units]}`;
}

/** Converts a value typed in display units back to millimetres. */
export function toMm(value: number, units: UnitsDisplay): number {
  return value * UNIT_FACTOR[units];
}

export function unitLabel(units: UnitsDisplay): string {
  return UNIT_LABEL[units];
}

/** Resolves "system" against the OS preference. */
export function effectiveTheme(theme: Theme, prefersDark: boolean): 'dark' | 'light' {
  return theme === 'system' ? (prefersDark ? 'dark' : 'light') : theme;
}

export interface SettingsState {
  settings: Settings;
  printers: PrinterProfile[];
  loaded: boolean;
}

export const useSettingsStore = create<SettingsState>()(() => ({
  settings: DEFAULT_SETTINGS,
  printers: BUILTIN_PRINTERS,
  loaded: false,
}));

export function selectDefaultPrinter(s: SettingsState): PrinterProfile {
  return s.printers.find((p) => p.id === s.settings.defaultPrinter) ?? s.printers[0] ?? BUILTIN_PRINTERS[0];
}
