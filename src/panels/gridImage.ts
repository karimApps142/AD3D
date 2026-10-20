// Pixel helpers for the 2D views (pure, so they are unit-tested).

import { srgbEncode } from '../engine/squamaChecks';
import type { GrayImage } from '../engine/types';

/**
 * RGBA bytes for a cell grid (row 0 = bottom of the plate) drawn top row first.
 * `linear` values are sRGB-encoded for display.
 */
export function gridToRgba(values: ArrayLike<number>, nx: number, ny: number, linear = true): Uint8ClampedArray<ArrayBuffer> {
  const out = new Uint8ClampedArray(new ArrayBuffer(nx * ny * 4));
  for (let j = 0; j < ny; j++) {
    const src = (ny - 1 - j) * nx;
    for (let i = 0; i < nx; i++) {
      const raw = values[src + i];
      const v = Number.isFinite(raw) ? Math.min(1, Math.max(0, raw)) : 0;
      const c = Math.round((linear ? srgbEncode(v) : v) * 255);
      const o = (j * nx + i) * 4;
      out[o] = c;
      out[o + 1] = c;
      out[o + 2] = c;
      out[o + 3] = 255;
    }
  }
  return out;
}

export function grayToRgba(img: GrayImage): Uint8ClampedArray<ArrayBuffer> {
  const out = new Uint8ClampedArray(new ArrayBuffer(img.width * img.height * 4));
  for (let k = 0; k < img.data.length; k++) {
    const c = img.data[k];
    out[4 * k] = c;
    out[4 * k + 1] = c;
    out[4 * k + 2] = c;
    out[4 * k + 3] = 255;
  }
  return out;
}

/** Puts RGBA bytes on a canvas sized to the data. */
export function paint(canvas: HTMLCanvasElement | null, rgba: Uint8ClampedArray<ArrayBuffer>, w: number, h: number): void {
  if (!canvas || w <= 0 || h <= 0) return;
  if (canvas.width !== w) canvas.width = w;
  if (canvas.height !== h) canvas.height = h;
  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  ctx.putImageData(new ImageData(rgba, w, h), 0, 0);
}
