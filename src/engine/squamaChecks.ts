// Client-side Squama maths for the M1 panels: §2.1 snapping in integer micrometres, §16 tone
// curve, a Lambert appearance estimate and the warnings the UI can predict before the core
// answers. The core stays the authority; these only make the panels responsive.

import { brightnessToTilt, DEG } from './squamaMath';
import type { CoreWarning, ImageOpts, LayoutSpec, SquamaParams } from './types';

// ---- §2.1 snapping (integer µm, half-up, never f64 division on decimals).

export function toUm(mm: number): number {
  return Math.round(mm * 1000);
}

/** max(1, (len + cell/2) / cell) in integer micrometres. */
export function snapCells(lenMm: number, cellMm: number): number {
  const cell = toUm(cellMm);
  if (cell <= 0) return 1;
  return Math.max(1, Math.floor((toUm(lenMm) + Math.floor(cell / 2)) / cell));
}

export interface PredictedLayout {
  nx: number;
  ny: number;
  wMm: number;
  hMm: number;
  cellMm: number;
}

/**
 * §2.1 layout for a crop of `cropWPx × cropHPx` pixels. With heightMm the height snaps on its
 * own (the core then crops to fill); otherwise ny = (2·nx·ch + cw) / (2·cw).
 *
 * Hex layouts mirror the core's `layout_for` (§6.2): nx counts half-cells across the flats,
 * ny rows of pitch √3/2·cell, axes swapped for flat-top, and an explicit height snaps down.
 */
export function predictLayout(
  widthMm: number,
  cellMm: number,
  cropWPx: number,
  cropHPx: number,
  heightMm?: number,
  layout?: LayoutSpec,
): PredictedLayout {
  const cellUm = toUm(cellMm);
  const cell = cellUm / 1000;
  const cw = Math.max(1, Math.round(cropWPx));
  const ch = Math.max(1, Math.round(cropHPx));
  if (layout?.kind === 'hex' && cellUm > 0) {
    const pitch = (3 * cell) / (2 * Math.sqrt(3));
    const halfCells = (lenUm: number) => Math.max(2, Math.floor((2 * lenUm + Math.floor(cellUm / 2)) / cellUm));
    const halfCellsDown = (lenUm: number) => Math.max(2, Math.floor((2 * lenUm) / cellUm));
    const rows = (lenMm: number) => Math.max(1, Math.floor(lenMm / pitch + 0.5));
    const rowsDown = (lenMm: number) => Math.max(1, Math.floor(lenMm / pitch + 1e-9));
    if (layout.orientation !== 'flat') {
      const m = halfCells(toUm(widthMm));
      const w = (m * cell) / 2;
      const n = heightMm !== undefined ? rowsDown(heightMm) : rows((w * ch) / cw);
      return { nx: m, ny: n, wMm: w, hMm: n * pitch, cellMm: cell };
    }
    const n = rows(widthMm);
    const w = n * pitch;
    const m = heightMm !== undefined ? halfCellsDown(toUm(heightMm)) : halfCells(toUm((w * ch) / cw));
    return { nx: m, ny: n, wMm: w, hMm: (m * cell) / 2, cellMm: cell };
  }
  const nx = snapCells(widthMm, cellMm);
  const ny = heightMm !== undefined ? snapCells(heightMm, cellMm) : Math.max(1, Math.floor((2 * nx * ch + cw) / (2 * cw)));
  return { nx, ny, wMm: nx * cell, hMm: ny * cell, cellMm: cell };
}

// ---- §16 tone curve (perceptual value v = srgb_encode(Y)).

export function srgbEncode(y: number): number {
  const c = Math.min(1, Math.max(0, y));
  return c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055;
}

export function srgbDecode(v: number): number {
  const c = Math.min(1, Math.max(0, v));
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
}

export interface ToneSettings {
  invert: boolean;
  brightness: number;
  contrast: number;
  gamma: number;
}

/** Defaults filled in and values clamped to the §16 domains (mirrors Rust `Tone::from_opts`). */
export function toneFromOpts(o: ImageOpts | undefined): ToneSettings {
  const fin = (v: number | undefined, d: number) => (v !== undefined && Number.isFinite(v) ? v : d);
  return {
    invert: !!o?.invert,
    brightness: Math.min(1, Math.max(-1, fin(o?.brightness, 0))),
    contrast: Math.min(3, Math.max(0, fin(o?.contrast, 1))),
    gamma: Math.min(5, Math.max(0.2, fin(o?.gamma, 1))),
  };
}

/** invert → levels (lo, hi) → brightness → contrast → gamma, on the perceptual value. */
export function toneCurve(v: number, t: ToneSettings, lo = 0, hi = 1): number {
  let x = t.invert ? 1 - v : v;
  if (hi - lo > 1e-6) x = Math.min(1, Math.max(0, (x - lo) / (hi - lo)));
  x += t.brightness;
  x = (x - 0.5) * t.contrast + 0.5;
  x = Math.min(1, Math.max(0, x));
  return x ** (1 / t.gamma);
}

/** Linear luminance through the tone curve (no auto-levels). */
export function toneLinear(y: number, t: ToneSettings): number {
  return srgbDecode(toneCurve(srgbEncode(y), t));
}

// ---- Appearance estimate (Lambert, no cast shadows) for the 2D "what you will see" view.

export interface Appearance {
  /** Display brightness in [0, 1] per cell: (ambient + E) / (ambient + E_max). */
  values: Float32Array;
  /** (ambient + E_max) / (ambient + E_min): how much contrast survives room light. */
  contrast: number;
}

export function simulateLambert(g: ArrayLike<number>, elevationDeg: number, maxTiltDeg: number, ambient: number): Appearance {
  const alpha = Math.max(1, elevationDeg) * DEG;
  const s = maxTiltDeg * DEG;
  const amb = Math.max(0, ambient);
  const eMax = Math.sin(Math.min(Math.PI / 2, alpha + s));
  const eMin = Math.max(0, Math.sin(alpha - s));
  const top = amb + eMax;
  const values = new Float32Array(g.length);
  for (let k = 0; k < g.length; k++) {
    const theta = brightnessToTilt(g[k], alpha, s);
    values[k] = top > 0 ? (amb + Math.sin(alpha + theta)) / top : 0;
  }
  const contrast = amb + eMin > 0 ? top / (amb + eMin) : Number.POSITIVE_INFINITY;
  return { values, contrast };
}

/**
 * The core's simulated appearance (§15.1: unnormalised radiance per facet, the lamp alone, cast
 * shadows included) as the 3D preview shows it: plus the preview ambient, relative to the
 * brightest facet. Same terms as the heightfield shader (`irr · max(0, N·L) · vis + ambient`).
 * `contrast` is p99 / p1 of the displayed radiance, so it follows the ambient slider.
 */
export function appearanceFromCore(sim: ArrayLike<number>, ambient: number): Appearance {
  const amb = Math.max(0, ambient);
  let top = 0;
  for (let k = 0; k < sim.length; k++) top = Math.max(top, sim[k]);
  top += amb;
  const values = new Float32Array(sim.length);
  for (let k = 0; k < sim.length; k++) values[k] = top > 0 ? (Math.max(0, sim[k]) + amb) / top : 0;
  const sorted = Array.from(values).sort((a, b) => a - b);
  const pct = (q: number) => sorted[Math.min(sorted.length - 1, Math.max(0, Math.floor(q * (sorted.length - 1))))] ?? 0;
  const lo = pct(0.01);
  const contrast = lo > 0 ? pct(0.99) / lo : Number.POSITIVE_INFINITY;
  return { values, contrast };
}

// ---- Predicted warnings (codes match the core where the core has the same check).

/** Ledge depth above which a down-facing step is flagged (FEATURES `ledge_warn_mm`). */
export const LEDGE_WARN_MM = 1.0;

/** Centre-pivot relief 2·e_max·tan S (§4) for square cells. */
export function reliefMm(cellMm: number, azimuthDeg: number, maxTiltDeg: number): number {
  const a = azimuthDeg * DEG;
  const eMax = 0.5 * cellMm * (Math.abs(Math.cos(a)) + Math.abs(Math.sin(a)));
  return 2 * eMax * Math.tan(maxTiltDeg * DEG);
}

export function predictWarnings(p: SquamaParams): CoreWarning[] {
  const w: CoreWarning[] = [];
  const relief = reliefMm(p.cellMm, p.lightAzimuthDeg, p.maxTiltDeg);
  if (relief < 0.15) {
    w.push({ code: 'relief_too_low', message: `Relief ${relief.toFixed(3)} mm is below 0.15 mm; raise max tilt or cell size.` });
  } else if (relief < 0.4) {
    w.push({ code: 'low_relief', message: `Relief ${relief.toFixed(3)} mm is below printable detail; raise max tilt or cell size.` });
  }
  if (p.cellMm < 3) w.push({ code: 'small_cell', message: 'Cells under 3 mm may not show (unconfirmed).' });
  if (p.lightElevationDeg < 3) w.push({ code: 'low_elevation', message: 'Light elevation below 3°.' });
  if (p.lightElevationDeg > 20) {
    w.push({ code: 'high_elevation', message: 'Light above 20°: facets distort and contrast drops (LuBan caps at 20°).' });
  }
  if (p.maxTiltDeg < p.lightElevationDeg) {
    w.push({
      code: 'no_true_black',
      message: `Max tilt ${p.maxTiltDeg.toFixed(1)}° is below the light angle ${p.lightElevationDeg.toFixed(1)}°, so no facet goes fully dark.`,
    });
  }
  if (p.optimiser?.kind === 'shadow' && p.maxReliefMm !== undefined && p.maxReliefMm < relief) {
    w.push({ code: 'relief_budget', message: `Max relief ${p.maxReliefMm} mm is below the plain Lambert relief ${relief.toFixed(2)} mm.` });
  }
  return w;
}

/** Warning for steps facing down in a standing print: at most cell·tan S deep. */
export function ledgeWarning(cellMm: number, maxTiltDeg: number, standing: boolean, lightAlongPrintZ: boolean): CoreWarning | null {
  if (!standing || !lightAlongPrintZ) return null;
  const depth = cellMm * Math.tan(maxTiltDeg * DEG);
  if (depth <= LEDGE_WARN_MM) return null;
  return {
    code: 'ledge_depth',
    message: `Steps up to ${depth.toFixed(1)} mm deep face down in this orientation; they may need support.`,
  };
}

/** Keeps the first warning per code. */
export function mergeWarnings(...lists: ReadonlyArray<ReadonlyArray<CoreWarning>>): CoreWarning[] {
  const seen = new Set<string>();
  const out: CoreWarning[] = [];
  for (const list of lists) {
    for (const w of list) {
      if (seen.has(w.code)) continue;
      seen.add(w.code);
      out.push(w);
    }
  }
  return out;
}
