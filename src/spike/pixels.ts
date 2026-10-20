// Pixel comparisons used as objective evidence that shadows render and the light moves.

import type { PixelReadback } from '../viewport/viewportBus';

function luma(d: Uint8Array, i: number): number {
  return 0.299 * d[i] + 0.587 * d[i + 1] + 0.114 * d[i + 2];
}

export interface PixelDiff {
  pixels: number;
  /** Pixels darker in `a` than in `b` by more than the threshold. */
  darker: number;
  /** Pixels whose luma differs by more than the threshold either way. */
  changed: number;
  darkerFraction: number;
  changedFraction: number;
}

export function comparePixels(a: PixelReadback, b: PixelReadback, threshold = 8): PixelDiff {
  if (a.width !== b.width || a.height !== b.height) {
    throw new Error(`readback sizes differ: ${a.width}×${a.height} vs ${b.width}×${b.height}`);
  }
  const pixels = a.width * a.height;
  let darker = 0;
  let changed = 0;
  for (let i = 0, n = pixels * 4; i < n; i += 4) {
    const d = luma(a.data, i) - luma(b.data, i);
    if (d < -threshold) darker++;
    if (Math.abs(d) > threshold) changed++;
  }
  return {
    pixels,
    darker,
    changed,
    darkerFraction: pixels ? darker / pixels : 0,
    changedFraction: pixels ? changed / pixels : 0,
  };
}
