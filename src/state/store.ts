// App state (zustand). Parameters drive the generator; view settings only affect the preview.

import { create } from 'zustand';
import type { MeshPayload, SquamaParams } from '../engine/types';

/** LuBan's Squama defaults (docs/research/01-squama-video.md). */
export const DEFAULT_PARAMS: SquamaParams = {
  widthMm: 300,
  cellMm: 6,
  lightAzimuthDeg: 180,
  lightElevationDeg: 5,
  maxTiltDeg: 10,
  baseMm: 3,
  image: 'synthetic',
  // Not document state (see paramsHistory TRANSIENT_KEYS): only a slider drag asks for 'preview'.
  quality: 'final',
  // The shadow-aware optimiser is cheap at final quality (PLAN §4): on by default.
  // An explicit { kind: 'none' } turns it off (and survives save/open).
  optimiser: { kind: 'shadow', maxSweeps: 8 },
};

export interface ViewSettings {
  /** Room light as a fraction of the lamp's irradiance at normal incidence. */
  ambient: number;
  /** When true the preview light sits where the design light is. */
  followDesignLight: boolean;
  /** Preview light, used when followDesignLight is false (degrees). */
  previewAzimuthDeg: number;
  previewElevationDeg: number;
  /** True while Alt+drag is swinging the light. */
  lightDragging: boolean;
}

export const DEFAULT_VIEW: ViewSettings = {
  ambient: 0.02,
  followDesignLight: true,
  previewAzimuthDeg: DEFAULT_PARAMS.lightAzimuthDeg,
  previewElevationDeg: DEFAULT_PARAMS.lightElevationDeg,
  lightDragging: false,
};

export interface DisplayMesh {
  payload: MeshPayload;
  /** Increments on every mesh shown. */
  version: number;
  kind: 'squama' | 'synthetic';
  /** Params that produced it (squama only). */
  params?: SquamaParams;
  /** performance.now() of the input that led to this mesh, for input → frame latency. */
  inputAt?: number;
}

export interface SpikeStatus {
  running: boolean;
  phase: string;
  summary?: string;
}

export interface AppState {
  params: SquamaParams;
  view: ViewSettings;
  mesh: DisplayMesh | null;
  engine: { name: 'mock' | 'tauri'; isNative: boolean } | null;
  engineError: string | null;
  previewBusy: boolean;
  previewError: string | null;
  /** Bumped to ask the viewport to frame the model. */
  fitRequest: number;
  spike: SpikeStatus | null;

  setParam: <K extends keyof SquamaParams>(key: K, value: SquamaParams[K]) => void;
  setParams: (patch: Partial<SquamaParams>) => void;
  setView: (patch: Partial<ViewSettings>) => void;
  showMesh: (mesh: Omit<DisplayMesh, 'version'>) => void;
  requestFit: () => void;
}

let meshVersion = 0;

export const useAppStore = create<AppState>()((set) => ({
  params: DEFAULT_PARAMS,
  view: DEFAULT_VIEW,
  mesh: null,
  engine: null,
  engineError: null,
  previewBusy: false,
  previewError: null,
  fitRequest: 0,
  spike: null,

  setParam: (key, value) =>
    set((s) => (s.params[key] === value ? s : { params: { ...s.params, [key]: value } })),
  setParams: (patch) => set((s) => ({ params: { ...s.params, ...patch } })),
  setView: (patch) => set((s) => ({ view: { ...s.view, ...patch } })),
  showMesh: (mesh) => set({ mesh: { ...mesh, version: ++meshVersion } }),
  requestFit: () => set((s) => ({ fitRequest: s.fitRequest + 1 })),
}));

/** The light the viewport renders with (degrees). */
export function selectPreviewLight(s: AppState): { azimuthDeg: number; elevationDeg: number } {
  return s.view.followDesignLight
    ? { azimuthDeg: s.params.lightAzimuthDeg, elevationDeg: s.params.lightElevationDeg }
    : { azimuthDeg: s.view.previewAzimuthDeg, elevationDeg: s.view.previewElevationDeg };
}
