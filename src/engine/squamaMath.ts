// Squama maths shared by the mock generator and the UI (grid prediction, light direction).
// Follows docs/design/squama-mesher.md §2–§4. Radians in code, millimetres everywhere.

import type { Vec3 } from './types';

export const DEG = Math.PI / 180;

export function degToRad(deg: number): number {
  return deg * DEG;
}

export function radToDeg(rad: number): number {
  return rad / DEG;
}

/**
 * Aspect ratio (height / width) of the built-in "synthetic" image. 63/50 matches the 300×378 mm
 * plate from the Squama video, so width 300 at cell 6 gives a 50×63 grid and width 600 at cell 6
 * (or 300 at cell 3) gives 100×126. The Rust synthetic image must use the same ratio.
 */
export const SYNTHETIC_ASPECT = 63 / 50;

export interface SquamaGrid {
  nx: number;
  ny: number;
  /** Snapped plate size. */
  widthMm: number;
  heightMm: number;
}

/**
 * Snaps the plate to whole cells the way LuBan does (typing 200 at cell 6 gives 198, 202 gives
 * 204): nx = round(width / cell). The height follows the image aspect: ny = round(nx · aspect).
 */
export function snapGrid(widthMm: number, cellMm: number, aspect: number): SquamaGrid {
  const cell = Math.max(cellMm, 1e-6);
  const nx = Math.max(1, Math.round(widthMm / cell));
  const ny = Math.max(1, Math.round(nx * aspect));
  return { nx, ny, widthMm: nx * cell, heightMm: ny * cell };
}

/** Recovers the grid from a plate's bounding box (the plate spans [0, nx·cell] × [0, ny·cell]). */
export function gridFromBBox(bboxMin: Vec3, bboxMax: Vec3, cellMm: number): { nx: number; ny: number } {
  return {
    nx: Math.round((bboxMax[0] - bboxMin[0]) / cellMm),
    ny: Math.round((bboxMax[1] - bboxMin[1]) / cellMm),
  };
}

/**
 * Lambert inverse (design §3): brightness g ∈ [0, 1] → facet tilt θ toward the light, in radians.
 * alpha = light elevation, maxTilt = S. Brightness of a facet tilted by θ is sin(alpha + θ).
 */
export function brightnessToTilt(g: number, alpha: number, maxTilt: number): number {
  const eMin = Math.max(0, Math.sin(alpha - maxTilt));
  const eMax = Math.sin(Math.min(Math.PI / 2, alpha + maxTilt));
  const e = eMin + Math.min(1, Math.max(0, g)) * (eMax - eMin);
  const theta = Math.asin(Math.min(1, Math.max(-1, e))) - alpha;
  return Math.min(maxTilt, Math.max(-maxTilt, theta));
}

/**
 * Common pivot height for centre-pivot facets (design §4): c = T + e_max · tan S, where e_max is
 * the largest |a · (corner − centroid)| of a square cell, (cell / 2)(|cos φ| + |sin φ|).
 */
export function pivotHeight(baseMm: number, cellMm: number, azimuth: number, maxTilt: number): number {
  const eMax = (cellMm / 2) * (Math.abs(Math.cos(azimuth)) + Math.abs(Math.sin(azimuth)));
  return baseMm + eMax * Math.tan(maxTilt);
}

/**
 * Unit vector from the plate toward the light, in plate space (x = u, y = v, z = w, which is also
 * three.js space for the viewport).
 */
export function lightDirection(azimuth: number, elevation: number): Vec3 {
  const c = Math.cos(elevation);
  return [c * Math.cos(azimuth), c * Math.sin(azimuth), Math.sin(elevation)];
}

/** Wraps an angle in degrees into [0, 360). */
export function wrapDeg(deg: number): number {
  const r = deg % 360;
  return r < 0 ? r + 360 : r;
}
