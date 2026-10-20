// Which light the viewport renders with: the design light (directional §12.1 v1 fields, or the
// optional point light) when "lock preview light to design light" is on, otherwise the free
// preview light (angles from the app store, lamp distance and light model from the viewport store).

import { degToRad, lightDirection, radToDeg, wrapDeg } from '../engine/squamaMath';
import type { SquamaParams, Vec3 } from '../engine/types';
import type { AppState } from '../state/store';
import { selectPreviewLight } from '../state/store';
import type { ViewportSettings } from './viewportStore';

export type ResolvedLight =
  | { kind: 'directional'; dir: Vec3; azimuthDeg: number; elevationDeg: number }
  | { kind: 'point'; pos: Vec3; dir: Vec3; distanceMm: number; azimuthDeg: number; elevationDeg: number };

/** The design's point light, if any (§12.1 `light: { kind: 'point'; posMm }`, plate space). */
export function designPointLight(params: SquamaParams): Vec3 | null {
  const light = params.light;
  if (!light || light.kind !== 'point' || !Array.isArray(light.posMm) || light.posMm.length !== 3) return null;
  const p = light.posMm;
  return p.every(Number.isFinite) ? [p[0], p[1], p[2]] : null;
}

/** Azimuth, elevation (degrees) and distance of a lamp seen from `centre`. */
export function lampAngles(pos: Vec3, centre: Vec3): { azimuthDeg: number; elevationDeg: number; distanceMm: number } {
  const dx = pos[0] - centre[0];
  const dy = pos[1] - centre[1];
  const dz = pos[2] - centre[2];
  const distanceMm = Math.hypot(dx, dy, dz);
  return {
    azimuthDeg: wrapDeg(radToDeg(Math.atan2(dy, dx))),
    elevationDeg: radToDeg(Math.atan2(dz, Math.hypot(dx, dy))),
    distanceMm,
  };
}

/** Lamp position at the given angles and distance from `centre`. */
export function lampPosition(centre: Vec3, azimuthDeg: number, elevationDeg: number, distanceMm: number): Vec3 {
  const d = lightDirection(degToRad(azimuthDeg), degToRad(elevationDeg));
  return [centre[0] + d[0] * distanceMm, centre[1] + d[1] * distanceMm, centre[2] + d[2] * distanceMm];
}

/** `centre` = the middle of the plate's front face (plate space). */
export function resolvePreviewLight(app: AppState, vp: ViewportSettings, centre: Vec3): ResolvedLight {
  const designLamp = designPointLight(app.params);
  if (app.view.followDesignLight && designLamp) {
    const a = lampAngles(designLamp, centre);
    const dir = lightDirection(degToRad(a.azimuthDeg), degToRad(a.elevationDeg));
    return { kind: 'point', pos: designLamp, dir, ...a };
  }
  const { azimuthDeg, elevationDeg } = selectPreviewLight(app);
  const dir = lightDirection(degToRad(azimuthDeg), degToRad(elevationDeg));
  if (vp.lightKind === 'point') {
    const distanceMm = vp.lampDistanceMm;
    return { kind: 'point', pos: lampPosition(centre, azimuthDeg, elevationDeg, distanceMm), dir, distanceMm, azimuthDeg, elevationDeg };
  }
  return { kind: 'directional', dir, azimuthDeg, elevationDeg };
}

/** A string that changes whenever the resolved light changes (for frame-skip keys). */
export function lightKey(l: ResolvedLight): string {
  return l.kind === 'point' ? `p|${l.pos.join(',')}` : `d|${l.azimuthDeg}|${l.elevationDeg}`;
}
