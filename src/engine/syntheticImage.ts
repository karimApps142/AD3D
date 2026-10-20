// The built-in "synthetic" test image: a moonlit night (crescent moon with a halo, stars, two
// hills). Large shapes and strong contrast so it reads even at 50×63 cells. Pure function of the
// normalised position, so any generator can reproduce it; aspect H/W = SYNTHETIC_ASPECT.

import { SYNTHETIC_ASPECT } from './squamaMath';

const STARS: ReadonlyArray<readonly [number, number]> = [
  [0.14, 1.13],
  [0.29, 0.96],
  [0.09, 0.82],
  [0.41, 1.17],
  [0.88, 1.16],
  [0.93, 0.72],
  [0.33, 0.72],
  [0.52, 1.05],
];

function smoothstep(e0: number, e1: number, x: number): number {
  const t = Math.min(1, Math.max(0, (x - e0) / (e1 - e0)));
  return t * t * (3 - 2 * t);
}

/** Coverage of a disc with a soft (antialiased) edge: 1 inside, 0 outside. */
function disc(dx: number, dy: number, r: number, soft = 0.004): number {
  return 1 - smoothstep(r - soft, r + soft, Math.hypot(dx, dy));
}

/**
 * Brightness in [0, 1] at normalised image position (x, y): x left→right, y bottom→top, both 0..1.
 */
export function syntheticBrightness(x: number, y: number): number {
  const X = x;
  const Y = y * SYNTHETIC_ASPECT; // width units, so circles stay round

  // Sky: darker towards the horizon.
  let g = 0.16 + 0.2 * y;

  // Moon halo.
  const mx = 0.62, my = 0.93, mr = 0.2;
  const dMoon = Math.hypot(X - mx, Y - my);
  if (dMoon > mr) g += 0.22 * Math.exp(-(((dMoon - mr) / 0.09) ** 2));

  // Crescent: the moon disc minus an offset disc.
  const moon = disc(X - mx, Y - my, mr) * (1 - disc(X - 0.705, Y - 0.985, 0.165));
  const limb = 0.97 - 0.18 * (Math.min(1, dMoon / mr) ** 4);
  g = g + (limb - g) * moon;

  // Stars.
  for (const [sx, sy] of STARS) {
    const s = disc(X - sx, Y - sy, 0.019);
    g = g + (1 - g) * s;
  }

  // Back hill (moonlit grey), then front hill (dark).
  const back = 0.43 + 0.07 * Math.sin(2 * Math.PI * (0.9 * X + 0.1)) + 0.03 * Math.sin(2 * Math.PI * 2.3 * X);
  const front = 0.25 + 0.06 * Math.sin(2 * Math.PI * (1.4 * X + 0.6)) + 0.02 * Math.sin(2 * Math.PI * 4.1 * X);
  const inBack = 1 - smoothstep(back - 0.004, back + 0.004, Y);
  g = g + (0.4 + 0.12 * (Y / back) - g) * inBack;
  const inFront = 1 - smoothstep(front - 0.004, front + 0.004, Y);
  g = g + (0.05 - g) * inFront;

  return Math.min(1, Math.max(0, g));
}

/**
 * Mean brightness per cell, row-major from the bottom row (j = 0) up, `samples`² points per cell.
 * Memoised per grid; treat the returned array as read-only.
 */
export function cellBrightness(nx: number, ny: number, samples = 3): Float32Array {
  // The image is a fixed function of (nx, ny, samples), and sampling it costs far more than
  // meshing, so slider drags that keep the grid reuse it. Callers must not mutate the result.
  const key = `${nx}x${ny}x${samples}`;
  const hit = cache.get(key);
  if (hit) return hit;
  const out = sampleCells(nx, ny, samples);
  cache.set(key, out);
  if (cache.size > CACHE_LIMIT) cache.delete(cache.keys().next().value as string);
  return out;
}

const CACHE_LIMIT = 8;
const cache = new Map<string, Float32Array>();

function sampleCells(nx: number, ny: number, samples: number): Float32Array {
  const out = new Float32Array(nx * ny);
  const inv = 1 / (samples * samples);
  for (let j = 0; j < ny; j++) {
    for (let i = 0; i < nx; i++) {
      let sum = 0;
      for (let sj = 0; sj < samples; sj++) {
        const y = (j + (sj + 0.5) / samples) / ny;
        for (let si = 0; si < samples; si++) {
          sum += syntheticBrightness((i + (si + 0.5) / samples) / nx, y);
        }
      }
      out[j * nx + i] = sum * inv;
    }
  }
  return out;
}
