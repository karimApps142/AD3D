// Browser stand-in for squama_target / squama_image_thumbnail: the built-in synthetic image put
// through the same orientation, crop and tone steps as src-tauri/src/squama_cmds.rs (without
// auto-levels), then averaged per cell.

import { predictLayout, srgbEncode, toneFromOpts, toneLinear } from './squamaChecks';
import { SYNTHETIC_ASPECT } from './squamaMath';
import { syntheticBrightness } from './syntheticImage';
import type { GrayImage, ImageOpts, SquamaParams, TargetGrid } from './types';

/** Virtual pixel size of the synthetic image (same aspect as the Rust one, 500×630). */
export const MOCK_IMAGE_PX: [number, number] = [500, Math.round(500 * SYNTHETIC_ASPECT)];

/** Size of an image after a clockwise rotation. */
export function orientedSize(w: number, h: number, rotate: number): [number, number] {
  return Math.round(rotate / 90) % 2 === 1 ? [h, w] : [w, h];
}

/**
 * Maps a normalised point (x right, y down) in the rotated/mirrored image back to the source
 * image (x right, y down). Matches `orient_image` in Rust.
 */
export function unorientPoint(x: number, y: number, rotate: number, mirror: boolean): [number, number] {
  const xs = mirror ? 1 - x : x;
  switch (((Math.round(rotate / 90) % 4) + 4) % 4) {
    case 1:
      return [y, 1 - xs];
    case 2:
      return [1 - xs, 1 - y];
    case 3:
      return [1 - y, xs];
    default:
      return [xs, y];
  }
}

/** Linear brightness of the synthetic image at a normalised source point (y down). */
function sourceAt(x: number, y: number): number {
  return syntheticBrightness(x, 1 - y);
}

export function mockThumbnail(maxPx: number, rotate: number, mirror: boolean): GrayImage {
  const [sw, sh] = MOCK_IMAGE_PX;
  const scale = Math.min(1, maxPx / Math.max(sw, sh));
  const [w, h] = orientedSize(Math.max(1, Math.round(sw * scale)), Math.max(1, Math.round(sh * scale)), rotate);
  const data = new Uint8Array(w * h);
  for (let j = 0; j < h; j++) {
    for (let i = 0; i < w; i++) {
      const [x, y] = unorientPoint((i + 0.5) / w, (j + 0.5) / h, rotate, mirror);
      data[j * w + i] = Math.round(srgbEncode(sourceAt(x, y)) * 255);
    }
  }
  return { width: w, height: h, data };
}

export function mockTarget(params: SquamaParams, samples = 3): TargetGrid {
  const o: ImageOpts = params.imageOpts ?? {};
  const rotate = o.rotate ?? 0;
  const [iw, ih] = orientedSize(MOCK_IMAGE_PX[0], MOCK_IMAGE_PX[1], rotate);
  const [x0, y0, x1, y1] = o.crop ?? [0, 0, 1, 1];
  const cw = Math.max(1e-6, x1 - x0);
  const ch = Math.max(1e-6, y1 - y0);
  const l = predictLayout(params.widthMm, params.cellMm, cw * iw, ch * ih, params.heightMm);
  const tone = toneFromOpts(o);
  const g = new Float32Array(l.nx * l.ny);
  for (let j = 0; j < l.ny; j++) {
    for (let i = 0; i < l.nx; i++) {
      let s = 0;
      for (let b = 0; b < samples; b++) {
        for (let a = 0; a < samples; a++) {
          const u = (i + (a + 0.5) / samples) / l.nx;
          const v = (j + (b + 0.5) / samples) / l.ny; // plate v up
          const [x, y] = unorientPoint(x0 + u * cw, y0 + (1 - v) * ch, rotate, !!o.mirror);
          s += toneLinear(sourceAt(x, y), tone);
        }
      }
      g[j * l.nx + i] = s / (samples * samples);
    }
  }
  return { nx: l.nx, ny: l.ny, g };
}
