// Printer profiles, export orientation and the tile plan preview (§9, §14.1, square grid).
// Mirrors plan_tiles_square / max_tile_extent in src-tauri/src/squama_cmds.rs, which mirror
// the core's plan_tiles contract; the export itself is planned again in Rust.

import type { ExportOrientation, Vec3 } from '../engine/types';

export interface PrinterProfile {
  id: string;
  name: string;
  /** Build volume X × Y × Z, mm. */
  bedMm: Vec3;
  /** Axis the bed moves along (bed-slingers), for the hand-over notes. */
  bedMotion: 'x' | 'y' | null;
}

export const PRINTERS: readonly PrinterProfile[] = [
  { id: 'cr6se', name: 'Creality CR-6 SE', bedMm: [235, 235, 250], bedMotion: 'y' },
  { id: 'custom', name: 'Custom', bedMm: [220, 220, 250], bedMotion: null },
];

export const DEFAULT_PRINTER_ID = 'cr6se';
export const DEFAULT_MARGIN_MM = 5;

export const ORIENTATIONS: ReadonlyArray<{ id: ExportOrientation; label: string }> = [
  { id: 'standing-auto', label: 'Standing, auto (light from below)' },
  { id: 'standing-q0', label: 'Standing, as designed' },
  { id: 'standing-q1', label: 'Standing, turned 90°' },
  { id: 'standing-q2', label: 'Standing, turned 180°' },
  { id: 'standing-q3', label: 'Standing, turned 270°' },
  { id: 'flat', label: 'Flat on the bed (not recommended)' },
];

/** §9 auto: the quarter turn k minimising sin(φ + k·90°), ties to the smaller k. */
export function autoQuarterTurns(azimuthDeg: number): number {
  let best = 0;
  let bestS = Number.POSITIVE_INFINITY;
  for (let k = 0; k < 4; k++) {
    const s = Math.round(Math.sin(((azimuthDeg + 90 * k) * Math.PI) / 180) * 1e12);
    if (s < bestS) {
      bestS = s;
      best = k;
    }
  }
  return best;
}

/** Quarter turns of a standing orientation, or null for flat. */
export function quarterTurns(o: ExportOrientation, azimuthDeg: number): number | null {
  switch (o) {
    case 'flat':
      return null;
    case 'standing-auto':
      return autoQuarterTurns(azimuthDeg);
    default:
      return Number(o.slice(-1));
  }
}

/** Largest plate-space tile extent (u, v) that fits the bed (standing: u′ → X, v′ → Z). */
export function maxTileExtent(o: ExportOrientation, bedMm: Vec3, marginMm: number, azimuthDeg: number): [number, number] {
  const bx = bedMm[0] - 2 * marginMm;
  const by = bedMm[1] - 2 * marginMm;
  const bz = bedMm[2] - marginMm;
  const k = quarterTurns(o, azimuthDeg);
  if (k === null) return [bx, by];
  return k % 2 === 0 ? [bx, bz] : [bz, bx];
}

/** n cells in `parts` near-equal runs, larger first (63 → 32 + 31). */
export function splitEven(n: number, parts: number): number[] {
  const p = Math.min(Math.max(1, parts), Math.max(1, n));
  const base = Math.floor(n / p);
  const extra = n % p;
  return Array.from({ length: p }, (_, k) => base + (k < extra ? 1 : 0));
}

export interface TilePlan {
  /** Cells per tile column, left → right. */
  cols: number[];
  /** Cells per tile row, bottom → top (plate v). */
  rows: number[];
  /** Whole plate fits in one piece. */
  fitsWhole: boolean;
  /** Plate-space limit per tile. */
  maxUv: [number, number];
  /** Error when not even one cell fits. */
  error: string | null;
}

export function planTiles(nx: number, ny: number, cellMm: number, maxUv: [number, number]): TilePlan {
  const mu = Math.floor((maxUv[0] + 1e-9) / cellMm);
  const mv = Math.floor((maxUv[1] + 1e-9) / cellMm);
  if (mu < 1 || mv < 1) {
    return { cols: [nx], rows: [ny], fitsWhole: false, maxUv, error: `A ${cellMm} mm cell does not fit the bed this way.` };
  }
  const cols = splitEven(nx, Math.ceil(nx / mu));
  const rows = splitEven(ny, Math.ceil(ny / mv));
  return { cols, rows, fitsWhole: cols.length === 1 && rows.length === 1, maxUv, error: null };
}

/** Print-space size X × Y × Z of a w × h plate of depth d (§9 transforms). */
export function printSize(wMm: number, hMm: number, depthMm: number, k: number | null): Vec3 {
  if (k === null) return [wMm, hMm, depthMm];
  return k % 2 === 0 ? [wMm, depthMm, hMm] : [hMm, depthMm, wMm];
}

/** Footprint may turn on the bed; the height must fit Z. Mirrors `fits_bed` in Rust. */
export function fitsBed(size: Vec3, bedMm: Vec3, marginMm: number): boolean {
  const bx = bedMm[0] - 2 * marginMm;
  const by = bedMm[1] - 2 * marginMm;
  const bz = bedMm[2] - marginMm;
  const e = 1e-6;
  const flat = (size[0] <= bx + e && size[1] <= by + e) || (size[0] <= by + e && size[1] <= bx + e);
  return flat && size[2] <= bz + e;
}

/** Whether the light runs along print Z (Top/Bottom in print space), where steps face down. */
export function lightAlongPrintZ(azimuthDeg: number, k: number | null): boolean {
  if (k === null) return false;
  const a = ((azimuthDeg + 90 * k) * Math.PI) / 180;
  return Math.abs(Math.sin(a)) > Math.abs(Math.cos(a));
}
