import { describe, expect, it } from 'vitest';
import {
  brightnessToTilt,
  degToRad,
  gridFromBBox,
  lightDirection,
  pivotHeight,
  snapGrid,
  SYNTHETIC_ASPECT,
  wrapDeg,
} from './squamaMath';

describe('grid snapping', () => {
  it('gives the spike grids for the synthetic image', () => {
    expect(snapGrid(300, 6, SYNTHETIC_ASPECT)).toEqual({ nx: 50, ny: 63, widthMm: 300, heightMm: 378 });
    expect(snapGrid(600, 6, SYNTHETIC_ASPECT)).toMatchObject({ nx: 100, ny: 126 });
    expect(snapGrid(300, 3, SYNTHETIC_ASPECT)).toMatchObject({ nx: 100, ny: 126 });
    expect(snapGrid(200, 4, SYNTHETIC_ASPECT)).toMatchObject({ nx: 50, ny: 63, heightMm: 252 });
  });

  it('rounds to the nearest whole cell like LuBan (200 → 198, 202 → 204)', () => {
    expect(snapGrid(200, 6, 1).widthMm).toBe(198);
    expect(snapGrid(202, 6, 1).widthMm).toBe(204);
  });

  it('never returns an empty grid', () => {
    expect(snapGrid(1, 6, 0.01)).toMatchObject({ nx: 1, ny: 1 });
  });

  it('recovers the grid from bounds', () => {
    expect(gridFromBBox([0, 0, 0], [300, 378, 4.2], 6)).toEqual({ nx: 50, ny: 63 });
  });
});

describe('Lambert inverse', () => {
  const alpha = degToRad(5);
  const S = degToRad(10);

  it('maps black to N·L = max(0, sin(α − S)) and white to sin(α + S)', () => {
    expect(Math.sin(alpha + brightnessToTilt(0, alpha, S))).toBeCloseTo(0, 9);
    expect(Math.sin(alpha + brightnessToTilt(1, alpha, S))).toBeCloseTo(Math.sin(alpha + S), 9);
  });

  it('is monotonic, linear in brightness, and bounded by ±S', () => {
    let prev = -Infinity;
    for (let g = 0; g <= 1.0001; g += 0.05) {
      const t = brightnessToTilt(g, alpha, S);
      expect(t).toBeGreaterThanOrEqual(prev);
      expect(Math.abs(t)).toBeLessThanOrEqual(S + 1e-12);
      const e = Math.sin(alpha + t);
      expect(e).toBeCloseTo(g * Math.sin(alpha + S), 9);
      prev = t;
    }
  });

  it('clamps out-of-range brightness', () => {
    expect(brightnessToTilt(-1, alpha, S)).toBe(brightnessToTilt(0, alpha, S));
    expect(brightnessToTilt(2, alpha, S)).toBe(brightnessToTilt(1, alpha, S));
  });
});

describe('pivot height', () => {
  it('keeps every corner of every facet at or above the base', () => {
    for (const azDeg of [0, 30, 90, 135, 180, 270, 333]) {
      const az = degToRad(azDeg);
      const S = degToRad(20);
      const c = pivotHeight(3, 6, az, S);
      const a = [Math.cos(az), Math.sin(az)];
      for (const [dx, dy] of [[-3, -3], [3, -3], [3, 3], [-3, 3]]) {
        for (const theta of [-S, S]) {
          const w = c - Math.tan(theta) * (a[0] * dx + a[1] * dy);
          expect(w).toBeGreaterThanOrEqual(3 - 1e-9);
        }
      }
    }
  });
});

describe('light direction', () => {
  it('points from the plate toward the light', () => {
    const left = lightDirection(Math.PI, degToRad(5));
    expect(left[0]).toBeCloseTo(-Math.cos(degToRad(5)), 9);
    expect(left[1]).toBeCloseTo(0, 9);
    expect(left[2]).toBeCloseTo(Math.sin(degToRad(5)), 9);
    const top = lightDirection(Math.PI / 2, 0);
    expect(top[1]).toBeCloseTo(1, 9);
    expect(Math.hypot(...lightDirection(1, 0.3))).toBeCloseTo(1, 12);
  });

  it('wraps degrees', () => {
    expect(wrapDeg(370)).toBe(10);
    expect(wrapDeg(-10)).toBe(350);
  });
});
