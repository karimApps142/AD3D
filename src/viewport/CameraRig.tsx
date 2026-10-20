// Frames the model: once when the first mesh arrives, then whenever a fit is requested (F key,
// Fit button). It never refits on its own while sliders move, so the view stays put.

import { useThree } from '@react-three/fiber';
import { useEffect, useRef } from 'react';
import * as THREE from 'three';
import { useAppStore } from '../state/store';
import { viewportBus } from './viewportBus';

interface OrbitLike {
  target: THREE.Vector3;
  update: () => void;
}

/** Slightly right of and above the plate's axis, so its thickness reads. */
const VIEW_DIR = new THREE.Vector3(0.2, 0.12, 1).normalize();

export function fitCamera(camera: THREE.PerspectiveCamera, controls: OrbitLike | null, box: THREE.Box3, aspect: number): void {
  const center = box.getCenter(new THREE.Vector3());
  const size = box.getSize(new THREE.Vector3());
  const halfV = Math.tan(THREE.MathUtils.degToRad(camera.fov) / 2);
  const margin = 1.3;
  const distV = ((size.y / 2) * margin) / halfV;
  const distH = ((size.x / 2) * margin) / (halfV * Math.max(aspect, 0.1));
  const dist = Math.max(distV, distH, 50) + size.z;
  camera.position.copy(center).addScaledVector(VIEW_DIR, dist);
  camera.lookAt(center);
  camera.updateProjectionMatrix();
  if (controls) {
    controls.target.copy(center);
    controls.update();
  }
}

export function CameraRig() {
  const camera = useThree((s) => s.camera);
  const controls = useThree((s) => s.controls) as unknown as OrbitLike | null;
  const width = useThree((s) => s.size.width);
  const height = useThree((s) => s.size.height);
  const invalidate = useThree((s) => s.invalidate);
  const fitRequest = useAppStore((s) => s.fitRequest);
  const hasMesh = useAppStore((s) => s.mesh !== null);
  const done = useRef<number | null>(null);

  useEffect(() => {
    if (!hasMesh || !controls || !(camera instanceof THREE.PerspectiveCamera)) return;
    if (done.current === fitRequest) return;
    done.current = fitRequest;
    fitCamera(camera, controls, viewportBus.modelBox, width / Math.max(height, 1));
    invalidate();
  }, [hasMesh, controls, camera, fitRequest, width, height, invalidate]);

  return null;
}
