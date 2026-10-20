// Panel-only state for the M1 Squama workflow: the loaded photo, lamp placement, export settings
// and export progress. Generator parameters stay in the app store (state/store.ts).

import { create } from 'zustand';
import type { CoreWarning, ExportResult, GrayImage, ImageInfo, TargetGrid } from '../engine/types';
import type { ExportSettings } from './squamaDto';
import { DEFAULT_MARGIN_MM, DEFAULT_PRINTER_ID, PRINTERS } from './tilePlan';

export interface ExportStatus {
  phase: 'idle' | 'running' | 'done' | 'error' | 'cancelled';
  /** 0–1 overall. */
  fraction: number;
  stage: string;
  part: string;
  totalParts: number;
  written: string[];
  warnings: CoreWarning[];
  result: ExportResult | null;
  error: string | null;
}

export const IDLE_EXPORT: ExportStatus = {
  phase: 'idle',
  fraction: 0,
  stage: '',
  part: '',
  totalParts: 0,
  written: [],
  warnings: [],
  result: null,
  error: null,
};

export interface SquamaUiState {
  image: ImageInfo | null;
  /** True while a photo is being decoded (large or HEIC photos take a moment). */
  imageLoading: boolean;
  thumbnail: GrayImage | null;
  imageError: string | null;
  target: TargetGrid | null;
  targetError: string | null;
  /** Point lamp, relative to the plate centre. */
  lamp: { distanceMm: number; heightMm: number };
  export: ExportSettings;
  exportStatus: ExportStatus;
  set: (patch: Partial<Omit<SquamaUiState, 'set' | 'setExport' | 'setExportStatus'>>) => void;
  setExport: (patch: Partial<ExportSettings>) => void;
  setExportStatus: (patch: Partial<ExportStatus>) => void;
}

const cr6 = PRINTERS.find((p) => p.id === DEFAULT_PRINTER_ID)!;

export const DEFAULT_EXPORT: ExportSettings = {
  printerId: cr6.id,
  bedMm: [...cr6.bedMm],
  marginMm: DEFAULT_MARGIN_MM,
  orientation: 'standing-auto',
  formats: ['3mf', 'stl'],
  // Keys whenever the plate is tiled and the base leaves room (core KeyKind::Auto).
  keys: 'auto',
  bedMotion: cr6.bedMotion ?? 'none',
  stand: false,
  calibrationTile: false,
  tile: true,
};

export const useSquamaUi = create<SquamaUiState>()((set) => ({
  image: null,
  imageLoading: false,
  thumbnail: null,
  imageError: null,
  target: null,
  targetError: null,
  lamp: { distanceMm: 600, heightMm: 60 },
  export: DEFAULT_EXPORT,
  exportStatus: IDLE_EXPORT,
  set: (patch) => set(patch),
  setExport: (patch) => set((s) => ({ export: { ...s.export, ...patch } })),
  setExportStatus: (patch) => set((s) => ({ exportStatus: { ...s.exportStatus, ...patch } })),
}));

/** Applies one export event to the status (pure; used by the panel and tests). */
export function reduceExportEvent(s: ExportStatus, e: import('../engine/types').ExportEvent): ExportStatus {
  switch (e.kind) {
    case 'started':
      return { ...s, totalParts: e.totalParts };
    case 'progress':
      return { ...s, fraction: e.fraction, stage: e.stage, part: e.part, totalParts: e.totalParts };
    case 'partWritten':
      return { ...s, written: [...s.written, e.path] };
    case 'warning':
      return s.warnings.some((w) => w.code === e.code) ? s : { ...s, warnings: [...s.warnings, { code: e.code, message: e.message }] };
  }
}
