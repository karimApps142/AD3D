// The generated model. Mesh updates bypass React: a store subscription applies each payload to the
// GPU buffers (see geometry.ts) and requests one frame.

import { useThree } from '@react-three/fiber';
import { useEffect, useMemo, useRef } from 'react';
import * as THREE from 'three';
import { perf } from '../state/perf';
import { type DisplayMesh, useAppStore } from '../state/store';
import { applyPayload, type GeometrySlot } from './geometry';
import { viewportBus } from './viewportBus';

/** Light grey PLA. */
export const PLATE_COLOR = new THREE.Color('#d8d8d4');
/** Linear-space luminance of PLATE_COLOR, used by the auto exposure. */
export const PLATE_ALBEDO = 0.2126 * PLATE_COLOR.r + 0.7152 * PLATE_COLOR.g + 0.0722 * PLATE_COLOR.b;

export function PlateMesh() {
  const invalidate = useThree((s) => s.invalidate);
  const slotRef = useRef<GeometrySlot | null>(null);

  const mesh = useMemo(() => {
    // No normals: flat shading derives each face normal from screen-space derivatives.
    const material = new THREE.MeshStandardMaterial({
      color: PLATE_COLOR,
      roughness: 0.8,
      metalness: 0,
      flatShading: true,
    });
    const m = new THREE.Mesh(new THREE.BufferGeometry(), material);
    m.castShadow = true;
    m.receiveShadow = true;
    m.name = 'plate';
    return m;
  }, []);

  useEffect(() => {
    const apply = (display: DisplayMesh | null) => {
      if (!display) return;
      const t0 = performance.now();
      const result = applyPayload(slotRef.current, display.payload);
      if (mesh.geometry !== result.slot.geometry) {
        const old = mesh.geometry;
        mesh.geometry = result.slot.geometry;
        if (old !== result.replaced) old.dispose(); // the initial placeholder
      }
      result.replaced?.dispose();
      slotRef.current = result.slot;
      const geometryMs = performance.now() - t0;
      perf.last.geometryMs = geometryMs;
      perf.last.geometryMode = result.mode;
      if (result.slot.geometry.boundingBox) viewportBus.setModelBox(result.slot.geometry.boundingBox);
      viewportBus.applied = {
        version: display.version,
        inputAt: display.inputAt,
        geometryMs,
        mode: result.mode,
      };
      invalidate();
    };
    apply(useAppStore.getState().mesh);
    return useAppStore.subscribe((state, prev) => {
      if (state.mesh !== prev.mesh) apply(state.mesh);
    });
  }, [mesh, invalidate]);

  useEffect(
    () => () => {
      mesh.geometry.dispose();
      (mesh.material as THREE.Material).dispose();
    },
    [mesh],
  );

  return <primitive object={mesh} />;
}
