// Key light (directional, self-shadowing) or point lamp (a spot light aimed at the plate, with a
// perspective shadow map), room fill (hemisphere) and the light marker at the light's position.
// Also drives the light uniforms of the analytic plate material. Everything is placed
// imperatively before each frame from the stores and the model bounds, so moving the light never
// re-renders React.

import { useFrame, useThree } from '@react-three/fiber';
import { useEffect, useMemo } from 'react';
import * as THREE from 'three';
import { degToRad } from '../engine/squamaMath';
import type { Vec3 } from '../engine/types';
import { useAppStore } from '../state/store';
import { autoExposure, fitShadowCamera, fitSpotLight } from './lightMath';
import { PLATE_ALBEDO, plateMaterials } from './PlateMesh';
import { lightKey, resolvePreviewLight } from './previewLight';
import { viewportBus } from './viewportBus';
import { useViewportStore } from './viewportStore';

export const SHADOW_MAP_SIZE = 4096;
export const SPOT_SHADOW_MAP_SIZE = 2048;
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
    const lamp = new THREE.SpotLight(0xffffff, 1, 0, Math.PI / 4, 0, 2);
    lamp.name = 'point-lamp';
    lamp.castShadow = true;
    lamp.shadow.mapSize.set(SPOT_SHADOW_MAP_SIZE, SPOT_SHADOW_MAP_SIZE);
    lamp.shadow.bias = SHADOW_BIAS;
    lamp.visible = false;
    const hemi = new THREE.HemisphereLight(SKY, GROUND, 0);
    hemi.name = 'room-fill';
    return { light, lamp, hemi, marker: buildMarker(), key: '' };
  }, []);

  useEffect(() => {
    viewportBus.light = rig.light;
    viewportBus.activeLight = rig.light;
    // Moving the light or changing the exposure inputs only needs a new frame.
    const unsubscribe = useAppStore.subscribe((s, prev) => {
      if (s.view !== prev.view || s.params !== prev.params) invalidate();
    });
    const unsubscribeVp = useViewportStore.subscribe(() => invalidate());
    return () => {
      unsubscribe();
      unsubscribeVp();
      if (viewportBus.light === rig.light) viewportBus.light = null;
      viewportBus.activeLight = null;
    };
  }, [rig, invalidate]);

  useEffect(
    () => () => {
      rig.light.shadow.map?.dispose();
      rig.lamp.shadow.map?.dispose();
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
    const { light, lamp, hemi, marker } = rig;
    const hf = plateMaterials().heightfield.uniforms;
    // The analytic shadows follow the active light's shadow intensity (the spike's C1 toggles it).
    hf.uShadowStrength.value = (lamp.visible ? lamp : light).shadow.intensity;

    const state = useAppStore.getState();
    const vp = useViewportStore.getState();
    const box = viewportBus.modelBox;
    const centre: Vec3 = [(box.min.x + box.max.x) / 2, (box.min.y + box.max.y) / 2, box.max.z];
    const resolved = resolvePreviewLight(state, vp, centre);
    const { ambient, lightDragging } = state.view;
    const maxTiltDeg = state.params.maxTiltDeg;
    const key = `${lightKey(resolved)}|${ambient}|${maxTiltDeg}|${lightDragging}|${viewportBus.modelBoxVersion}`;
    if (key === rig.key) return;
    rig.key = key;

    const el = degToRad(resolved.elevationDeg);
    const dir = new THREE.Vector3(...resolved.dir);
    const exposure = autoExposure(PLATE_ALBEDO, el, degToRad(maxTiltDeg), ambient);
    hemi.intensity = exposure.ambient / HEMI_VERTICAL_FACTOR;
    hf.uAmbient.value = exposure.ambient;
    hf.uDirect.value = exposure.direct;
    const diag = Math.max(box.getSize(new THREE.Vector3()).length(), 10);
    const target = new THREE.Vector3(...centre);
    let sunPos: THREE.Vector3;
    let reach: number;

    if (resolved.kind === 'directional') {
      const fit = fitShadowCamera(box, dir);
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
      light.intensity = exposure.direct;
      light.visible = true;
      light.castShadow = true;
      lamp.visible = false;
      lamp.castShadow = false;
      viewportBus.activeLight = light;
      hf.uLightKind.value = 0;
      hf.uLightDir.value.copy(dir);
      reach = 0.62 * diag;
      sunPos = target.clone().addScaledVector(dir, reach);
    } else {
      const pos = new THREE.Vector3(...resolved.pos);
      const spot = fitSpotLight(box, pos, target);
      lamp.position.copy(pos);
      lamp.target.position.copy(target);
      lamp.target.updateMatrixWorld();
      lamp.angle = spot.angle;
      lamp.shadow.camera.near = spot.near;
      lamp.shadow.camera.far = spot.far;
      lamp.shadow.camera.updateProjectionMatrix();
      // Irradiance at the plate centre = intensity / d² (decay 2) = the exposure's direct term.
      lamp.intensity = exposure.direct * resolved.distanceMm * resolved.distanceMm;
      lamp.visible = true;
      lamp.castShadow = true;
      light.visible = false;
      light.castShadow = false;
      viewportBus.activeLight = lamp;
      hf.uLightKind.value = 1;
      hf.uLightPos.value.copy(pos);
      hf.uRefDist2.value = resolved.distanceMm * resolved.distanceMm;
      reach = resolved.distanceMm;
      sunPos = pos;
    }

    // Marker: the sun (or lamp) on a ray from the plate centre; the azimuth ring shows while dragging.
    marker.sun.position.copy(sunPos);
    marker.sun.scale.setScalar(0.014 * diag);
    const ray = marker.ray.geometry.getAttribute('position') as THREE.BufferAttribute;
    ray.setXYZ(0, target.x, target.y, target.z);
    ray.setXYZ(1, sunPos.x, sunPos.y, sunPos.z);
    ray.needsUpdate = true;
    marker.ray.geometry.computeBoundingSphere();
    marker.ring.visible = lightDragging;
    marker.ring.position.set(target.x, target.y, target.z + reach * Math.sin(el));
    marker.ring.scale.setScalar(Math.max(reach * Math.cos(el), 1e-3));
  }, 0);

  return (
    <>
      <primitive object={rig.light} />
      <primitive object={rig.light.target} />
      <primitive object={rig.lamp} />
      <primitive object={rig.lamp.target} />
      <primitive object={rig.hemi} />
      <primitive object={rig.marker.group} />
    </>
  );
}
