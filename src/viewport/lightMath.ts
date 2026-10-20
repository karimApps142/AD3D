// Light placement maths for the viewport: a tight orthographic shadow frustum around the model and
// an automatic exposure that keeps the brightest possible facet near white.

import * as THREE from 'three';

const Z_AXIS = new THREE.Vector3(0, 0, 1);

/**
 * Up vector for the shadow camera: the plate normal (+z) with the light direction projected out,
 * which lines the shadow map up with the plate. Falls back to +y when the light is overhead.
 */
export function shadowUp(dir: THREE.Vector3, out = new THREE.Vector3()): THREE.Vector3 {
  const d = dir.dot(Z_AXIS);
  if (Math.abs(d) > 0.995) return out.set(0, 1, 0);
  return out.copy(Z_AXIS).addScaledVector(dir, -d).normalize();
}

export interface ShadowFit {
  /** Light position (world); the light shines from here toward `target`. */
  position: THREE.Vector3;
  target: THREE.Vector3;
  up: THREE.Vector3;
  left: number;
  right: number;
  top: number;
  bottom: number;
  near: number;
  far: number;
}

/**
 * Orthographic shadow camera that just contains `box` (plus a margin) seen along `dir` (unit
 * vector from the model toward the light). At grazing angles the plate is foreshortened along the
 * light, so a tight fit spends the shadow-map texels where the tiny facet shadows are.
 */
export function fitShadowCamera(box: THREE.Box3, dir: THREE.Vector3): ShadowFit {
  const target = box.getCenter(new THREE.Vector3());
  const size = box.getSize(new THREE.Vector3());
  const radius = Math.max(size.length() / 2, 1);
  const margin = 0.01 * radius + 0.5;
  const position = target.clone().addScaledVector(dir, radius * 2 + 10);
  const up = shadowUp(dir);

  // Same view matrix Object3D.lookAt gives a camera at `position` looking at `target`.
  const world = new THREE.Matrix4().lookAt(position, target, up).setPosition(position);
  const view = world.clone().invert();

  const p = new THREE.Vector3();
  let x0 = Infinity, x1 = -Infinity, y0 = Infinity, y1 = -Infinity, z0 = Infinity, z1 = -Infinity;
  for (let i = 0; i < 8; i++) {
    p.set(i & 1 ? box.max.x : box.min.x, i & 2 ? box.max.y : box.min.y, i & 4 ? box.max.z : box.min.z);
    p.applyMatrix4(view);
    x0 = Math.min(x0, p.x);
    x1 = Math.max(x1, p.x);
    y0 = Math.min(y0, p.y);
    y1 = Math.max(y1, p.y);
    z0 = Math.min(z0, p.z);
    z1 = Math.max(z1, p.z);
  }
  return {
    position,
    target,
    up,
    left: x0 - margin,
    right: x1 + margin,
    bottom: y0 - margin,
    top: y1 + margin,
    // The camera looks down −z: the nearest corner has the largest z.
    near: Math.max(0.01, -z1 - margin),
    far: -z0 + margin,
  };
}

/** Brightest linear value the exposure aims for (≈ 0.93 after sRGB encoding). */
export const EXPOSURE_TARGET = 0.85;

export interface Exposure {
  /** DirectionalLight intensity. */
  direct: number;
  /** Ambient irradiance (to be split over the hemisphere light). */
  ambient: number;
}

/**
 * Eyes adapt to a dark room, so scale the lamp until the brightest facet the design allows,
 * N·L = sin(elevation + maxTilt), renders at EXPOSURE_TARGET. `ambient` is room light as a
 * fraction of the lamp's irradiance at normal incidence; more of it lowers contrast exactly as
 * (sin(α + S) + a) / a, which is what the Ambient slider is meant to show.
 *
 * three.js radiance = albedo / π · (I · N·L + A).
 */
export function autoExposure(
  albedo: number,
  elevationRad: number,
  maxTiltRad: number,
  ambient: number,
): Exposure {
  const nlMax = Math.max(0.02, Math.sin(Math.min(Math.PI / 2, Math.max(0, elevationRad) + maxTiltRad)));
  const a = Math.max(0, ambient);
  const direct = (EXPOSURE_TARGET * Math.PI) / (Math.max(albedo, 0.05) * (nlMax + a));
  return { direct, ambient: a * direct };
}

export interface SpotFit {
  /** Half-angle of the cone (radians) that covers the whole model. */
  angle: number;
  near: number;
  far: number;
}

/**
 * Cone and shadow-camera depth range for a lamp at `pos` aimed at `target` that just contains
 * `box`. Used for the point light (a spot light whose cone covers the plate behaves like a point
 * light there, with a far better shadow map than a cube map).
 */
export function fitSpotLight(box: THREE.Box3, pos: THREE.Vector3, target: THREE.Vector3): SpotFit {
  const axis = target.clone().sub(pos).normalize();
  const p = new THREE.Vector3();
  let maxAngle = 0;
  let dMin = Infinity;
  let dMax = 0;
  for (let i = 0; i < 8; i++) {
    p.set(i & 1 ? box.max.x : box.min.x, i & 2 ? box.max.y : box.min.y, i & 4 ? box.max.z : box.min.z).sub(pos);
    const d = p.length();
    dMin = Math.min(dMin, d);
    dMax = Math.max(dMax, d);
    if (d > 1e-9) maxAngle = Math.max(maxAngle, Math.acos(Math.min(1, Math.max(-1, p.dot(axis) / d))));
  }
  const angle = Math.min(Math.PI / 2 - 0.01, maxAngle * 1.05 + 0.02);
  return { angle, near: Math.max(0.5, dMin * 0.5), far: dMax * 1.5 + 10 };
}
