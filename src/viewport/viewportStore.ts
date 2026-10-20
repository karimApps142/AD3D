// Viewport-only display state (none of it changes the model). Kept apart from the app store so the
// viewport owns its settings; the shared settings it reads (ambient, the preview light angles and
// "light follows the design light") stay in state/store.ts.

import { create } from 'zustand';
import type { TilePlanOverlay } from './tiles';

export type RenderMode =
  /** Lambert + exact ray-marched cast shadows over the cell planes (default). */
  | 'analytic'
  /** The spike's three.js shadow-map light (fallback). */
  | 'shadowMap'
  /** Facets coloured by height, other face kinds in flat colours. */
  | 'relief';

export type PreviewLightKind = 'directional' | 'point';

export interface ViewportSettings {
  renderMode: RenderMode;
  /** Light model used when the preview light is free (or the design light is directional). */
  lightKind: PreviewLightKind;
  /** Lamp distance from the plate centre for a free point light (mm). */
  lampDistanceMm: number;
  /** Orthographic, straight-on view: the plate as seen hanging on a wall. */
  frontView: boolean;
  /** Draw the tile outlines and part labels. */
  showTiles: boolean;
  /** Tile layout from the core; null = use the viewport's fallback planner. */
  tilePlan: TilePlanOverlay | null;
  /** Last "Save preview image" result, for the toolbar's status line. */
  lastSaved: string | null;
}

export const DEFAULT_LAMP_DISTANCE_MM = 800;
export const LAMP_DISTANCE_RANGE: [number, number] = [100, 5000];

function initialRenderMode(): RenderMode {
  try {
    const q = new URLSearchParams(globalThis.location?.search ?? '');
    const m = q.get('render');
    if (m === 'analytic' || m === 'shadowMap' || m === 'relief') return m;
  } catch {
    // no location (tests)
  }
  return 'analytic';
}

export const DEFAULT_VIEWPORT: ViewportSettings = {
  renderMode: initialRenderMode(),
  lightKind: 'directional',
  lampDistanceMm: DEFAULT_LAMP_DISTANCE_MM,
  frontView: false,
  showTiles: false,
  tilePlan: null,
  lastSaved: null,
};

export interface ViewportState extends ViewportSettings {
  set: (patch: Partial<ViewportSettings>) => void;
}

export const useViewportStore = create<ViewportState>()((set) => ({
  ...DEFAULT_VIEWPORT,
  set: (patch) => set(patch),
}));
