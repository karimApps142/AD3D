// Takes over rendering (useFrame priority > 0) so every frame can be timed, counted and followed
// by after-render hooks. The canvas runs frameloop="demand": nothing renders while idle.

import { useFrame, useThree } from '@react-three/fiber';
import { useEffect, useRef } from 'react';
import * as THREE from 'three';
import { recordFrame } from '../state/perf';
import { viewportBus } from './viewportBus';

const _center = new THREE.Vector3();
const _size = new THREE.Vector3();

/** Keeps near/far tight around the model so close-ups of 0.3 mm steps keep depth precision. */
function updateClipPlanes(camera: THREE.Camera): void {
  if (!(camera instanceof THREE.PerspectiveCamera)) return;
  const box = viewportBus.modelBox;
  box.getCenter(_center);
  const radius = Math.max(box.getSize(_size).length() / 2, 1);
  const d = camera.position.distanceTo(_center);
  const near = Math.min(100, Math.max(0.5, (d - radius) * 0.5));
  const far = d + radius * 8 + 2000;
  if (Math.abs(camera.near - near) > near * 0.01 || Math.abs(camera.far - far) > far * 0.01) {
    camera.near = near;
    camera.far = far;
    camera.updateProjectionMatrix();
  }
}

export function RenderDriver() {
  const gl = useThree((s) => s.gl);
  const invalidate = useThree((s) => s.invalidate);
  const get = useThree((s) => s.get);
  const lastVersion = useRef(-1);

  useEffect(() => {
    viewportBus.attach({
      gl,
      invalidate,
      getView: () => {
        const s = get();
        return { camera: s.camera, controls: s.controls };
      },
    });
    return () => viewportBus.detach(gl);
  }, [gl, invalidate, get]);

  useFrame((state) => {
    updateClipPlanes(state.camera);
    const applied = viewportBus.applied;
    const version = applied?.version ?? 0;
    const t0 = performance.now();
    state.gl.render(state.scene, state.camera);
    const t1 = performance.now();
    const meshChanged = version !== lastVersion.current;
    lastVersion.current = version;
    const frame = {
      t: t1,
      cpuMs: t1 - t0,
      meshVersion: version,
      meshChanged,
      latencyMs: meshChanged && applied?.inputAt !== undefined ? t1 - applied.inputAt : undefined,
    };
    recordFrame(frame);
    viewportBus.runAfterRender(state.gl, frame);
  }, 1);

  return null;
}
