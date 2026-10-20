// Frames the model: once when the first mesh arrives, then whenever a fit is requested (F key,
// Fit button) or the front viewer is switched on or off. It never refits on its own while sliders
// move, so the view stays put.

import { useThree } from '@react-three/fiber';
import { useEffect, useRef } from 'react';
import * as THREE from 'three';
import { useAppStore } from '../state/store';
import { viewportBus } from './viewportBus';
import { useViewportStore } from './viewportStore';

interface OrbitLike {
  target: THREE.Vector3;
  update: () => void;
  /** The camera the controls drive (drei recreates the controls when the default camera changes). */
  object?: THREE.Camera;
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

/** Fraction of the view the plate fills in the front viewer. */
export const FRONT_FILL = 0.88;

/**
 * Front viewer: an orthographic camera looking straight down −w at the plate's centre with v up,
 * zoomed so the whole plate fills FRONT_FILL of the view (as someone standing in front of it).
 */
export function fitFrontCamera(camera: THREE.OrthographicCamera, controls: OrbitLike | null, box: THREE.Box3, width: number, height: number): void {
  const center = box.getCenter(new THREE.Vector3());
  const size = box.getSize(new THREE.Vector3());
  camera.left = -width / 2;
  camera.right = width / 2;
  camera.top = height / 2;
  camera.bottom = -height / 2;
  camera.zoom = Math.max(1e-4, FRONT_FILL * Math.min(width / Math.max(size.x, 1), height / Math.max(size.y, 1)));
  camera.up.set(0, 1, 0);
  camera.position.set(center.x, center.y, box.max.z + Math.max(1000, size.length()));
  camera.near = 0.1;
  camera.far = camera.position.z - box.min.z + 1000;
  camera.lookAt(center.x, center.y, center.z);
  camera.updateProjectionMatrix();
  if (controls) {
    controls.target.set(center.x, center.y, center.z);
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
  const done = useRef<string | null>(null);

  useEffect(() => {
    if (!hasMesh || !controls) return;
    // After a camera switch, wait for the controls bound to the new camera: the old ones would
    // re-aim the new camera at their own target on their next update.
    if (controls.object && controls.object !== camera) return;
    // Refit on a request or a camera switch (front viewer on/off), not on resize.
    const key = `${fitRequest}|${camera.uuid}`;
    if (done.current === key) return;
    if (camera instanceof THREE.PerspectiveCamera) {
      fitCamera(camera, controls, viewportBus.modelBox, width / Math.max(height, 1));
    } else if (camera instanceof THREE.OrthographicCamera) {
      fitFrontCamera(camera, controls, viewportBus.modelBox, width, height);
    } else return;
    done.current = key;
    invalidate();
  }, [hasMesh, controls, camera, fitRequest, width, height, invalidate]);

  return null;
}

/**
 * Swaps in an orthographic camera while the front viewer is on and restores the perspective one
 * afterwards (r3f's default camera is replaced through the store, like drei's makeDefault).
 */
export function FrontCamera() {
  const set = useThree((s) => s.set);
  const width = useThree((s) => s.size.width);
  const height = useThree((s) => s.size.height);
  const invalidate = useThree((s) => s.invalidate);
  const front = useViewportStore((s) => s.frontView);
  const ortho = useRef<THREE.OrthographicCamera | null>(null);
  ortho.current ??= new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 10000);

  useEffect(() => {
    if (!front) return;
    const cam = ortho.current!;
    let old: THREE.Camera | null = null;
    set((s) => {
      old = s.camera;
      return { camera: cam as unknown as typeof s.camera };
    });
    invalidate();
    return () => {
      if (old) set({ camera: old as never });
      invalidate();
    };
  }, [front, set, invalidate]);

  useEffect(() => {
    const cam = ortho.current!;
    cam.left = -width / 2;
    cam.right = width / 2;
    cam.top = height / 2;
    cam.bottom = -height / 2;
    cam.updateProjectionMatrix();
    invalidate();
  }, [width, height, invalidate]);

  return null;
}
