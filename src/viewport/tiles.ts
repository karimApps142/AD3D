// Tile layout overlay (docs/design/squama-mesher.md §14.1): rectangles on the plate with labels.
// The real plan comes from the core's plan_tiles; until that is wired through the engine the
// viewport computes the same cuts itself (cell lines with near-equal counts for square cells, hex
// centre lines for hex cells) so the overlay can be shown.

import type { CellLayout } from './heightfield';

export interface TileOverlay {
  /** (column, row), row 0 at the bottom of the plate. */
  id: [number, number];
  /** Plate-space rectangle [u0, v0, u1, v1] (mm). */
  rectMm: [number, number, number, number];
  label: string;
}

export interface TilePlanOverlay {
  tiles: TileOverlay[];
  source: 'core' | 'fallback';
  /** Largest tile size allowed by the bed, for the overlay's caption. */
  maxTileMm: [number, number];
}

export interface BedSpec {
  xMm: number;
  yMm: number;
  zMm: number;
  marginMm: number;
}

/** The owner's Creality CR-6 SE (START_HERE): 235 × 235 × 250 mm. */
export const CR6_SE_BED: BedSpec = { xMm: 235, yMm: 235, zMm: 250, marginMm: 5 };

export type TileOrientation = 'standing' | 'flat';

export function tileLabel(col: number, row: number): string {
  return `R${row + 1}C${col + 1}`;
}

/** Splits n units into k near-equal counts, larger ones first (63 → 32 + 31). */
export function splitCounts(n: number, k: number): number[] {
  const base = Math.floor(n / k);
  const extra = n - base * k;
  return Array.from({ length: k }, (_, i) => base + (i < extra ? 1 : 0));
}

/** Cut-line spacing along u and v: whole cells, or the §6.2 hex centre/row lines. */
export function cutUnits(l: CellLayout): [number, number] {
  if (l.kind === 'square') return [l.cellMm, l.cellMm];
  const half = l.cellMm / 2;
  const pitch = (Math.sqrt(3) / 2) * l.cellMm;
  return l.orientation === 'pointy' ? [half, pitch] : [pitch, half];
}

function axisCuts(length: number, unit: number, maxLen: number): number[] {
  const n = Math.max(1, Math.round(length / unit));
  let k = Math.max(1, Math.ceil(length / Math.max(maxLen, unit)));
  for (; k <= n; k++) {
    const counts = splitCounts(n, k);
    if (counts[0] * unit <= maxLen + 1e-9) break;
  }
  const counts = splitCounts(n, Math.min(k, n));
  const edges = [0];
  for (const c of counts) edges.push(edges[edges.length - 1] + c * unit);
  edges[edges.length - 1] = length; // absorb snapping round-off
  return edges;
}

/**
 * Tiles that fit the bed. Standing: u along the bed's X, v up the Z axis (the facets face the
 * bed's Y, the axis a bed-slinger moves). Flat: u along X, v along Y.
 *
 * Only used without the core's plan: panels/useCoreTilePlan.ts puts squama::plan_tiles' result
 * in useViewportStore.tilePlan (source 'core') whenever the engine provides it.
 */
export function planTilesFallback(l: CellLayout, bed: BedSpec = CR6_SE_BED, orientation: TileOrientation = 'standing'): TilePlanOverlay {
  const maxU = bed.xMm - 2 * bed.marginMm;
  const maxV = orientation === 'standing' ? bed.zMm - bed.marginMm : bed.yMm - 2 * bed.marginMm;
  const [unitU, unitV] = cutUnits(l);
  const us = axisCuts(l.widthMm, unitU, maxU);
  const vs = axisCuts(l.heightMm, unitV, maxV);
  const tiles: TileOverlay[] = [];
  for (let row = 0; row + 1 < vs.length; row++) {
    for (let col = 0; col + 1 < us.length; col++) {
      tiles.push({ id: [col, row], rectMm: [us[col], vs[row], us[col + 1], vs[row + 1]], label: tileLabel(col, row) });
    }
  }
  return { tiles, source: 'fallback', maxTileMm: [maxU, maxV] };
}
