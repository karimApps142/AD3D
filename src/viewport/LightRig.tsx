// Key light (directional, self-shadowing), room fill (hemisphere) and the light marker.
// Everything is placed imperatively before each frame from the store and the model bounds, so
// moving the light never re-renders React.

import { useFrame, useThree } from '@react-three/fiber';
import { useEffect, useMemo } from 'react';
import * as THREE from 'three';
import { degToRad, lightDirection } from '../engine/squamaMath';
import { selectPreviewLight, useAppStore } from '../state/store';
import { autoExposure, fitShadowCamera } from './lightMath';
import { PLATE_ALBEDO } from './PlateMesh';
import { viewportBus } from './viewportBus';

export const SHADOW_MAP_SIZE = 4096;
/**
 * The plate is a closed mesh, so the shadow pass renders back faces (three.js default for
 * FrontSide materials) and lit faces never compare against themselves. A small negative depth
 * bias (normalised depth over the tight frustum, ~0.03 mm here) cleans up contact edges without
 * visibly detaching the ~0.3 mm step shadows. normalBias has no effect: the mesh has no normals.
 */
export const SHADOW_BIAS = -0.0001;
const MARKER_COLOR = '#ffc95c';
/** Hemisphere irradiance on a vertical face is the mean of sky and ground. */
const SKY = new THREE.Color('#ffffff');
const GROUND = new THREE.Color('#d9d9d9');
const HEMI_VERTICAL_FACTOR = (SKY.r + GROUND.r) / 2;

function buildMarker() {
  const group = new THREE.Group();
  group.name = 'light-marker';
  const sun = new THREE.Mesh(
    new THREE.SphereGeometry(1, 20, 14),
    new THREE.MeshBasicMaterial({ color: MARKER_COLOR, toneMapped: false }),
  );
  const rayGeometry = new THREE.BufferGeometry();
  rayGeometry.setAttribute('position', new THREE.BufferAttribute(new Float32Array(6), 3));
  const ray = new THREE.Line(
    rayGeometry,
    new THREE.LineBasicMaterial({ color: MARKER_COLOR, transparent: true, opacity: 0.35, toneMapped: false }),
  );
  const ringPoints: THREE.Vector3[] = [];
  for (let i = 0; i < 128; i++) {
    const a = (i / 128) * Math.PI * 2;
    ringPoints.push(new THREE.Vector3(Math.cos(a), Math.sin(a), 0));
  }
  const ring = new THREE.LineLoop(
    new THREE.BufferGeometry().setFromPoints(ringPoints),
    new THREE.LineBasicMaterial({ color: MARKER_COLOR, transparent: true, opacity: 0.22, toneMapped: false }),
  );
  ring.visible = false;
  group.add(sun, ray, ring);
  group.traverse((o) => {
    o.castShadow = false;
    o.receiveShadow = false;
    o.raycast = () => {};
  });
  return { group, sun, ray, ring };
}

export function LightRig() {
  const invalidate = useThree((s) => s.invalidate);

  const rig = useMemo(() => {
    const light = new THREE.DirectionalLight(0xffffff, 1);
    light.name = 'key-light';
    light.castShadow = true;
    light.shadow.mapSize.set(SHADOW_MAP_SIZE, SHADOW_MAP_SIZE);
    light.shadow.bias = SHADOW_BIAS;
    light.shadow.normalBias = 0;
    light.shadow.radius = 1;
    const hemi = new THREE.HemisphereLight(SKY, GROUND, 0);
    hemi.name = 'room-fill';
    return { light, hemi, marker: buildMarker(), key: '' };
  }, []);

  useEffect(() => {
    viewportBus.light = rig.light;
    // Moving the light or changing the exposure inputs only needs a new frame.
    const unsubscribe = useAppStore.subscribe((s, prev) => {
      if (s.view !== prev.view || s.params.maxTiltDeg !== prev.params.maxTiltDeg ||
        s.params.lightAzimuthDeg !== prev.params.lightAzimuthDeg ||
        s.params.lightElevationDeg !== prev.params.lightElevationDeg) {
        invalidate();
      }
    });
    return () => {
      unsubscribe();
      if (viewportBus.light === rig.light) viewportBus.light = null;
    };
  }, [rig, invalidate]);

  useEffect(
    () => () => {
      rig.light.shadow.map?.dispose();
      rig.marker.group.traverse((o) => {
        if (o instanceof THREE.Mesh || o instanceof THREE.Line) {
          o.geometry.dispose();
          (o.material as THREE.Material).dispose();
        }
      });
    },
    [rig],
  );

  // Runs before the render driver (priority 1); recomputes only when an input changed.
  useFrame(() => {
    const state = useAppStore.getState();
    const { azimuthDeg, elevationDeg } = selectPreviewLight(state);
    const { ambient, lightDragging } = state.view;
    const maxTiltDeg = state.params.maxTiltDeg;
    const key = `${azimuthDeg}|${elevationDeg}|${ambient}|${maxTiltDeg}|${lightDragging}|${viewportBus.modelBoxVersion}`;
    if (key === rig.key) return;
    rig.key = key;

    const az = degToRad(azimuthDeg);
    const el = degToRad(elevationDeg);
    const dir = new THREE.Vector3(...lightDirection(az, el));
    const box = viewportBus.modelBox;
    const fit = fitShadowCamera(box, dir);

    const { light, hemi, marker } = rig;
    light.position.copy(fit.position);
    light.target.position.copy(fit.target);
    light.target.updateMatrixWorld();
    const cam = light.shadow.camera;
    cam.up.copy(fit.up);
    cam.left = fit.left;
    cam.right = fit.right;
    cam.top = fit.top;
    cam.bottom = fit.bottom;
    cam.near = fit.near;
    cam.far = fit.far;
    cam.updateProjectionMatrix();

    const exposure = autoExposure(PLATE_ALBEDO, el, degToRad(maxTiltDeg), ambient);
    light.intensity = exposure.direct;
    hemi.intensity = exposure.ambient / HEMI_VERTICAL_FACTOR;

    // Marker: a small sun on a ray from the model centre; the azimuth ring shows while dragging.
    const diag = Math.max(box.getSize(new THREE.Vector3()).length(), 10);
    const reach = 0.62 * diag;
    const sunPos = fit.target.clone().addScaledVector(dir, reach);
    marker.sun.position.copy(sunPos);
    marker.sun.scale.setScalar(0.014 * diag);
    const ray = marker.ray.geometry.getAttribute('position') as THREE.BufferAttribute;
    ray.setXYZ(0, fit.target.x, fit.target.y, fit.target.z);
    ray.setXYZ(1, sunPos.x, sunPos.y, sunPos.z);
    ray.needsUpdate = true;
    marker.ray.geometry.computeBoundingSphere();
    marker.ring.visible = lightDragging;
    marker.ring.position.set(fit.target.x, fit.target.y, fit.target.z + reach * Math.sin(el));
    marker.ring.scale.setScalar(Math.max(reach * Math.cos(el), 1e-3));
  }, 0);

  return (
    <>
      <primitive object={rig.light} />
      <primitive object={rig.light.target} />
      <primitive object={rig.hemi} />
      <primitive object={rig.marker.group} />
    </>
  );
}
