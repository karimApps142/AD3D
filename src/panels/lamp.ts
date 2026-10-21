// The point lamp (§15.4) in the panels. `params.light.posMm` is the one source of truth: it is
// what the core uses, what a project saves and what undo restores. The lamp controls (distance
// from the plate centre, height out of the face) are derived from it, and a user change to the
// plate size or the azimuth moves the lamp with the plate in the same params update, so it is
// one undo step and never happens on load, undo or redo.

import type { ImageInfo, SquamaParams, Vec3 } from '../engine/types';
import { predictPlate } from './plateLayout';
import { lampPosition } from './squamaDto';

export interface LampPlacement {
  distanceMm: number;
  heightMm: number;
}

/** Lamp distance (in the plate plane, from the plate centre) and height of a lamp position. */
export function lampFromPos(wMm: number, hMm: number, posMm: Vec3): LampPlacement {
  return { distanceMm: Math.hypot(posMm[0] - wMm / 2, posMm[1] - hMm / 2), heightMm: posMm[2] };
}

const round3 = (v: number) => Math.round(v * 1000) / 1000;

/**
 * The lamp patch that keeps a point lamp in the same place relative to the plate when a user
 * change moves the plate centre or the azimuth (`prev` → `next`). Null when nothing to do: no
 * point lamp, the change sets the light itself, or the plate and the azimuth did not move.
 */
export function followLamp(
  prev: SquamaParams,
  next: SquamaParams,
  prevImage: ImageInfo | null,
  nextImage: ImageInfo | null = prevImage,
): Pick<SquamaParams, 'light'> | null {
  const light = next.light;
  if (light?.kind !== 'point' || prev.light?.kind !== 'point' || light !== prev.light) return null;
  const a = predictPlate(prev, prevImage);
  const b = predictPlate(next, nextImage);
  const turn = ((next.lightAzimuthDeg - prev.lightAzimuthDeg) * Math.PI) / 180;
  if (a.wMm === b.wMm && a.hMm === b.hMm && turn === 0) return null;
  const [x, y, z] = light.posMm;
  const dx = x - a.wMm / 2;
  const dy = y - a.hMm / 2;
  const c = Math.cos(turn);
  const s = Math.sin(turn);
  const posMm: Vec3 = [round3(b.wMm / 2 + c * dx - s * dy), round3(b.hMm / 2 + s * dx + c * dy), z];
  return { light: { kind: 'point', posMm } };
}

/** A lamp at `placement` on the azimuth ray of the plate (the lamp controls and the
 *  "Point lamp" button). */
export function placeLamp(wMm: number, hMm: number, azimuthDeg: number, placement: LampPlacement): Pick<SquamaParams, 'light'> {
  return { light: { kind: 'point', posMm: lampPosition(wMm, hMm, azimuthDeg, placement.distanceMm, placement.heightMm) } };
}
