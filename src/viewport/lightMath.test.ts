import * as THREE from 'three';
import { describe, expect, it } from 'vitest';
import { degToRad, lightDirection } from '../engine/squamaMath';
import { autoExposure, EXPOSURE_TARGET, fitShadowCamera, shadowUp } from './lightMath';

const plate = new THREE.Box3(new THREE.Vector3(0, 0, 0), new THREE.Vector3(300, 378, 4.2));

function corners(box: THREE.Box3): THREE.Vector3[] {
  const out: THREE.Vector3[] = [];
  for (let i = 0; i < 8; i++) {
    out.push(new THREE.Vector3(i & 1 ? box.max.x : box.min.x, i & 2 ? box.max.y : box.min.y, i & 4 ? box.max.z : box.min.z));
  }
  return out;
}

describe('shadow camera fit', () => {
  for (const [az, el] of [[180, 5], [0, 5], [90, 12], [270, 0], [45, 20], [123, 89.9]]) {
    it(`contains the model for azimuth ${az}°, elevation ${el}°`, () => {
      const dir = new THREE.Vector3(...lightDirection(degToRad(az), degToRad(el)));
      const fit = fitShadowCamera(plate, dir);
      const cam = new THREE.OrthographicCamera(fit.left, fit.right, fit.top, fit.bottom, fit.near, fit.far);
      cam.up.copy(fit.up);
      cam.position.copy(fit.position);
      cam.lookAt(fit.target);
      cam.updateMatrixWorld();
      cam.updateProjectionMatrix();
      for (const c of corners(plate)) {
        const ndc = c.clone().project(cam);
        expect(Math.abs(ndc.x)).toBeLessThanOrEqual(1);
        expect(Math.abs(ndc.y)).toBeLessThanOrEqual(1);
        expect(Math.abs(ndc.z)).toBeLessThanOrEqual(1);
      }
      expect(fit.near).toBeGreaterThan(0);
    });
  }

  it('is tight for grazing light: the plate foreshortens along the light', () => {
    const dir = new THREE.Vector3(...lightDirection(Math.PI, degToRad(5)));
    const fit = fitShadowCamera(plate, dir);
    const w = fit.right - fit.left;
    const h = fit.top - fit.bottom;
    // One side spans the plate height (378 mm), the other only ~300·sin 5° + relief.
    expect(Math.max(w, h)).toBeLessThan(400);
    expect(Math.min(w, h)).toBeLessThan(40);
  });

  it('picks a stable up vector, including for overhead light', () => {
    const up = shadowUp(new THREE.Vector3(-1, 0, 0));
    expect(up.z).toBeCloseTo(1, 9);
    const overhead = shadowUp(new THREE.Vector3(0, 0, 1));
    expect(overhead.y).toBe(1);
  });
});

describe('auto exposure', () => {
  const albedo = 0.7;
  it('puts the brightest possible facet at the target', () => {
    const el = degToRad(5);
    const S = degToRad(10);
    const e = autoExposure(albedo, el, S, 0);
    const brightest = (albedo / Math.PI) * e.direct * Math.sin(el + S);
    expect(brightest).toBeCloseTo(EXPOSURE_TARGET, 9);
    expect(e.ambient).toBe(0);
  });

  it('gives the contrast (sin(α+S) + a) / a with ambient a', () => {
    const el = degToRad(5);
    const S = degToRad(10);
    const a = 0.1;
    const e = autoExposure(albedo, el, S, a);
    const bright = (albedo / Math.PI) * (e.direct * Math.sin(el + S) + e.ambient);
    const dark = (albedo / Math.PI) * e.ambient;
    expect(bright).toBeCloseTo(EXPOSURE_TARGET, 9);
    expect(bright / dark).toBeCloseTo((Math.sin(el + S) + a) / a, 9);
  });
});
