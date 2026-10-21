// The open document: where it lives on disk and what was last saved, so the shell knows the
// title and whether there are unsaved changes. Parameters themselves stay in the app store.

import { create } from 'zustand';
import type { SquamaParams } from '../engine/types';
import { paramsEqual } from './paramsHistory';
import type { ExportSettings } from '../panels/squamaDto';
import { type AppState, DEFAULT_PARAMS, type ViewSettings } from './store';

export const GENERATOR_ID = 'shadow_plate';
export const GENERATOR_VERSION = 1;

/** Project document as the host reads and writes it (src-tauri/src/project.rs ProjectDoc). */
export interface ProjectDoc {
  format?: string;
  version?: number;
  generator: string;
  generatorVersion: number;
  params: SquamaParams & Record<string, unknown>;
  view?: Partial<Pick<ViewSettings, 'ambient' | 'followDesignLight' | 'previewAzimuthDeg' | 'previewElevationDeg'>>;
  printer?: string;
  /** The export panel's settings (tiling, keys, stand, calibration tile, orientation, bed). */
  export?: Partial<ExportSettings>;
  appVersion?: string;
}

export interface OpenedProject {
  path: string;
  doc: ProjectDoc;
  warnings: string[];
}

export interface DocumentState {
  /** Absolute path, or null for an untitled project. */
  path: string | null;
  /** Params as last saved or opened (the clean state). */
  savedParams: SquamaParams;
}

export const useDocumentStore = create<DocumentState>()(() => ({
  path: null,
  savedParams: DEFAULT_PARAMS,
}));

export function isDirty(params: SquamaParams, doc: DocumentState): boolean {
  return !paramsEqual(params, doc.savedParams);
}

/** "plate.ad3d" or "Untitled". */
export function documentName(path: string | null): string {
  if (!path) return 'Untitled';
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || 'Untitled';
}

/** Name for the Save dialog: the image's base name, else "Untitled". */
export function suggestedName(path: string | null, params: SquamaParams): string {
  if (path) return documentName(path);
  if (params.image && params.image !== 'synthetic') {
    const base = documentName(params.image).replace(/\.[^.]+$/, '');
    if (base) return `${base}.ad3d`;
  }
  return 'Untitled.ad3d';
}

export function windowTitle(path: string | null, dirty: boolean): string {
  return `${documentName(path)}${dirty ? ' — Edited' : ''} — AD3D`;
}

export function buildProjectDoc(state: Pick<AppState, 'params' | 'view'>, printer?: string, exportSettings?: ExportSettings): ProjectDoc {
  const { ambient, followDesignLight, previewAzimuthDeg, previewElevationDeg } = state.view;
  const doc: ProjectDoc = {
    generator: GENERATOR_ID,
    generatorVersion: GENERATOR_VERSION,
    params: { ...state.params },
    view: { ambient, followDesignLight, previewAzimuthDeg, previewElevationDeg },
    printer: exportSettings?.printerId ?? printer,
  };
  if (exportSettings) doc.export = { ...exportSettings, bedMm: [...exportSettings.bedMm], formats: [...exportSettings.formats] };
  return doc;
}

/** The export settings a document carries, over `base` (fields of the wrong type are ignored). */
export function exportFromDoc(doc: ProjectDoc, base: ExportSettings): ExportSettings {
  const e = doc.export;
  if (!e || typeof e !== 'object') return base;
  const out: ExportSettings = { ...base, bedMm: [...base.bedMm], formats: [...base.formats] };
  if (typeof e.printerId === 'string') out.printerId = e.printerId;
  if (Array.isArray(e.bedMm) && e.bedMm.length === 3 && e.bedMm.every((v) => typeof v === 'number' && v > 0)) out.bedMm = [...e.bedMm] as ExportSettings['bedMm'];
  if (typeof e.marginMm === 'number' && e.marginMm >= 0) out.marginMm = e.marginMm;
  if (typeof e.orientation === 'string') out.orientation = e.orientation;
  if (Array.isArray(e.formats) && e.formats.length && e.formats.every((f) => f === 'stl' || f === '3mf')) out.formats = [...e.formats];
  if (e.keys === 'none' || e.keys === 'pegs' || e.keys === 'auto') out.keys = e.keys;
  if (e.bedMotion === 'x' || e.bedMotion === 'y' || e.bedMotion === 'none') out.bedMotion = e.bedMotion;
  if (typeof e.stand === 'boolean') out.stand = e.stand;
  if (typeof e.calibrationTile === 'boolean') out.calibrationTile = e.calibrationTile;
  if (typeof e.tile === 'boolean') out.tile = e.tile;
  return out;
}

const NUMERIC_KEYS = ['widthMm', 'cellMm', 'lightAzimuthDeg', 'lightElevationDeg', 'maxTiltDeg', 'baseMm'] as const;

/**
 * Validates an opened document and returns the params to load. Missing fields take defaults,
 * unknown fields (later M1 options) pass through. Throws on another generator or bad values.
 */
export function paramsFromDoc(doc: ProjectDoc): SquamaParams {
  if (doc.generator !== GENERATOR_ID) {
    throw new Error(`this project uses the "${doc.generator}" generator, which this build does not have`);
  }
  if (!doc.params || typeof doc.params !== 'object') throw new Error('project has no parameters');
  const merged = { ...DEFAULT_PARAMS, ...doc.params } as SquamaParams & Record<string, unknown>;
  // No optimiser field means none (the core's SquamaJob meaning; spike/v1 projects and CLI
  // projects saved without --optimise). The optimiser is on only for new documents.
  if (doc.params.optimiser === undefined || doc.params.optimiser === null) merged.optimiser = { kind: 'none' };
  for (const k of NUMERIC_KEYS) {
    const v = merged[k];
    if (typeof v !== 'number' || !Number.isFinite(v)) throw new Error(`project parameter ${k} is not a number`);
  }
  if (typeof merged.image !== 'string' || merged.image === '') merged.image = DEFAULT_PARAMS.image;
  delete (merged as Record<string, unknown>).jobId;
  // Saved projects carry no quality (project.rs strips it): an opened plate is a final plate.
  merged.quality = 'final';
  return merged;
}

export function viewFromDoc(doc: ProjectDoc): Partial<ViewSettings> {
  const v = doc.view ?? {};
  const out: Partial<ViewSettings> = {};
  if (typeof v.ambient === 'number') out.ambient = v.ambient;
  if (typeof v.followDesignLight === 'boolean') out.followDesignLight = v.followDesignLight;
  if (typeof v.previewAzimuthDeg === 'number') out.previewAzimuthDeg = v.previewAzimuthDeg;
  if (typeof v.previewElevationDeg === 'number') out.previewElevationDeg = v.previewElevationDeg;
  return out;
}

export const PROJECT_EXT = 'ad3d';
/** Photo extensions every entry point accepts (File → Open, Import, drop). The same list as
 *  the host (src-tauri project.rs IMAGE_EXTS) and the engine (tauriEngine IMAGE_EXTENSIONS);
 *  HEIC/HEIF (iPhone photos) decode through sips. */
export const IMAGE_EXTS = ['png', 'jpg', 'jpeg', 'bmp', 'tif', 'tiff', 'webp', 'heic', 'heif'];

export type PathKind = 'project' | 'image' | 'unknown';

export function pathKind(path: string): PathKind {
  const ext = path.toLowerCase().match(/\.([a-z0-9]+)$/)?.[1] ?? '';
  if (ext === PROJECT_EXT) return 'project';
  if (IMAGE_EXTS.includes(ext)) return 'image';
  return 'unknown';
}
