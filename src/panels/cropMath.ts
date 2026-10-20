// Crop rectangle logic for the Image panel. Crops are normalised [x0, y0, x1, y1] in the
// rotated/mirrored image, y down (the §12.1 imageOpts.crop convention).
//
// "Snapped to whole cells": the plate is nx × ny whole cells, so the crop's pixel aspect is forced
// to exactly ny : nx. Each cell then covers the same whole share of the crop, and the core's
// §2.1 height rule (ny from the crop size) gives back the same ny.

import { predictLayout, snapCells } from '../engine/squamaChecks';

export type Crop = [number, number, number, number];

export const FULL_CROP: Crop = [0, 0, 1, 1];

/** Smallest crop side, as a fraction of the image. */
export const MIN_CROP = 0.02;

const clamp01 = (v: number) => Math.min(1, Math.max(0, v));

/** Ordered, inside [0, 1], at least MIN_CROP on each side. */
export function normalizeCrop(c: Crop): Crop {
  let [x0, y0, x1, y1] = c.map((v) => (Number.isFinite(v) ? clamp01(v) : 0)) as Crop;
  if (x1 < x0) [x0, x1] = [x1, x0];
  if (y1 < y0) [y0, y1] = [y1, y0];
  if (x1 - x0 < MIN_CROP) {
    x1 = Math.min(1, x0 + MIN_CROP);
    x0 = x1 - MIN_CROP;
  }
  if (y1 - y0 < MIN_CROP) {
    y1 = Math.min(1, y0 + MIN_CROP);
    y0 = y1 - MIN_CROP;
  }
  return [x0, y0, x1, y1];
}

export function isFullCrop(c: Crop | undefined): boolean {
  if (!c) return true;
  return c[0] <= 1e-6 && c[1] <= 1e-6 && c[2] >= 1 - 1e-6 && c[3] >= 1 - 1e-6;
}

export interface SnappedCrop {
  crop: Crop;
  nx: number;
  ny: number;
}

/**
 * Forces the crop to the plate's whole-cell aspect, keeping its centre where possible.
 * `imgW × imgH` is the oriented image size in pixels. With `heightMm` the plate height is fixed
 * (ny from it); otherwise ny follows the crop (§2.1) and the crop is trimmed to ny : nx.
 */
export function snapCrop(crop: Crop, imgW: number, imgH: number, widthMm: number, cellMm: number, heightMm?: number): SnappedCrop {
  const c = normalizeCrop(crop);
  const cwPx = (c[2] - c[0]) * imgW;
  const chPx = (c[3] - c[1]) * imgH;
  const nx = snapCells(widthMm, cellMm);
  const ny = heightMm !== undefined ? snapCells(heightMm, cellMm) : predictLayout(widthMm, cellMm, cwPx, chPx).ny;
  const want = ny / nx; // crop height / width in pixels
  // Largest rectangle of the wanted aspect inside the current crop.
  let w = cwPx;
  let h = cwPx * want;
  if (h > chPx) {
    h = chPx;
    w = h / want;
  }
  const cx = ((c[0] + c[2]) / 2) * imgW;
  const cy = ((c[1] + c[3]) / 2) * imgH;
  let x0 = cx - w / 2;
  let y0 = cy - h / 2;
  x0 = Math.min(Math.max(0, x0), imgW - w);
  y0 = Math.min(Math.max(0, y0), imgH - h);
  return { crop: [x0 / imgW, y0 / imgH, (x0 + w) / imgW, (y0 + h) / imgH], nx, ny };
}

export type CropHandle = 'move' | 'nw' | 'ne' | 'sw' | 'se' | 'n' | 's' | 'e' | 'w' | 'new';

/** Which handle a normalised point grabs; `tol` is the grab radius (normalised). */
export function hitCrop(c: Crop, x: number, y: number, tolX: number, tolY: number): CropHandle {
  const near = (a: number, b: number, t: number) => Math.abs(a - b) <= t;
  const inX = x >= c[0] - tolX && x <= c[2] + tolX;
  const inY = y >= c[1] - tolY && y <= c[3] + tolY;
  if (!inX || !inY) return 'new';
  const l = near(x, c[0], tolX);
  const r = near(x, c[2], tolX);
  const t = near(y, c[1], tolY);
  const b = near(y, c[3], tolY);
  if (t && l) return 'nw';
  if (t && r) return 'ne';
  if (b && l) return 'sw';
  if (b && r) return 'se';
  if (t) return 'n';
  if (b) return 's';
  if (l) return 'w';
  if (r) return 'e';
  return 'move';
}

/** The crop while dragging `handle` from (sx, sy) to (x, y), all normalised. */
export function dragCrop(start: Crop, handle: CropHandle, sx: number, sy: number, x: number, y: number): Crop {
  const dx = x - sx;
  const dy = y - sy;
  let [x0, y0, x1, y1] = start;
  switch (handle) {
    case 'move': {
      const w = x1 - x0;
      const h = y1 - y0;
      x0 = Math.min(Math.max(0, x0 + dx), 1 - w);
      y0 = Math.min(Math.max(0, y0 + dy), 1 - h);
      return [x0, y0, x0 + w, y0 + h];
    }
    case 'new':
      return normalizeCrop([sx, sy, x, y]);
    default:
      if (handle.includes('w')) x0 += dx;
      if (handle.includes('e')) x1 += dx;
      if (handle.includes('n')) y0 += dy;
      if (handle.includes('s')) y1 += dy;
      return normalizeCrop([x0, y0, x1, y1]);
  }
}

/**
 * The crop in the new orientation after rotating the image clockwise by `quarters` and/or
 * mirroring, so a crop follows the picture instead of jumping.
 */
export function rotateCrop(c: Crop, quarters: number, mirror: boolean): Crop {
  let [x0, y0, x1, y1] = c;
  const q = ((quarters % 4) + 4) % 4;
  for (let k = 0; k < q; k++) {
    // Clockwise: (x, y) → (1 − y, x).
    [x0, y0, x1, y1] = [1 - y1, x0, 1 - y0, x1];
  }
  if (mirror) [x0, x1] = [1 - x1, 1 - x0];
  return normalizeCrop([x0, y0, x1, y1]);
}
