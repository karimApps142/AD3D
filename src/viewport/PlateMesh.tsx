// The generated model. Mesh updates bypass React: a store subscription applies each payload to the
// GPU buffers (see geometry.ts) and requests one frame.
//
// Render modes (viewportStore.renderMode):
//   analytic   Lambert + exact ray-marched cast shadows over the per-cell planes (heightfieldShader)
//   shadowMap  the spike's MeshStandardMaterial lit by the shadow-mapped key light (fallback)
//   relief     height colour map with face kinds (reliefShader)
// All of them use flat per-triangle normals from screen-space derivatives (§12.2), never
// computeVertexNormals.

import { useThree } from '@react-three/fiber';
import { useEffect, useMemo, useRef } from 'react';
import * as THREE from 'three';
import { perf } from '../state/perf';
import { type DisplayMesh, useAppStore } from '../state/store';
import { payloadFaceKinds } from './faceKinds';
import { applyPayload, type GeometrySlot } from './geometry';
import { buildCellPlanes, type CellPlanes } from './heightfield';
import { createHeightfieldMaterial, type HeightfieldMaterial, setPlanes } from './heightfieldShader';
import { layoutFromDisplay } from './plateLayout';
import { createReliefMaterial, deindexWithKinds, type ReliefMaterial } from './reliefShader';
import { viewportBus } from './viewportBus';
import { type RenderMode, useViewportStore } from './viewportStore';

/** Light grey PLA. */
export const PLATE_COLOR = new THREE.Color('#d8d8d4');
/** Linear-space luminance of PLATE_COLOR, used by the auto exposure. */
export const PLATE_ALBEDO = 0.2126 * PLATE_COLOR.r + 0.7152 * PLATE_COLOR.g + 0.0722 * PLATE_COLOR.b;

export interface PlateMaterials {
  standard: THREE.MeshStandardMaterial;
  heightfield: HeightfieldMaterial;
  relief: ReliefMaterial;
  reliefKinds: ReliefMaterial;
}

let shared: PlateMaterials | null = null;

/** The plate's materials, shared with LightRig (which drives their light uniforms). */
export function plateMaterials(): PlateMaterials {
  shared ??= {
    // No normals: flat shading derives each face normal from screen-space derivatives.
    standard: new THREE.MeshStandardMaterial({ color: PLATE_COLOR, roughness: 0.8, metalness: 0, flatShading: true }),
    heightfield: createHeightfieldMaterial(PLATE_COLOR),
    relief: createReliefMaterial(false),
    reliefKinds: createReliefMaterial(true),
  };
  return shared;
}

export function PlateMesh() {
  const invalidate = useThree((s) => s.invalidate);
  const slotRef = useRef<GeometrySlot | null>(null);

  const rig = useMemo(() => {
    const mats = plateMaterials();
    const m = new THREE.Mesh<THREE.BufferGeometry, THREE.Material>(new THREE.BufferGeometry(), mats.heightfield);
    m.castShadow = true;
    m.receiveShadow = true;
    m.name = 'plate';
    const relief = new THREE.Mesh<THREE.BufferGeometry, THREE.Material>(new THREE.BufferGeometry(), mats.reliefKinds);
    relief.name = 'plate-relief';
    relief.visible = false;
    relief.castShadow = true;
    return {
      mesh: m,
      relief,
      planes: null as CellPlanes | null,
      planesVersion: -1,
      reliefVersion: -1,
      display: null as DisplayMesh | null,
    };
  }, []);

  useEffect(() => {
    const mats = plateMaterials();

    /** Brings the derived data (planes, de-indexed relief) up to date for the current mode. */
    const refresh = (mode: RenderMode) => {
      const display = rig.display;
      const needPlanes = mode === 'analytic' || mode === 'relief';
      if (display && needPlanes && rig.planesVersion !== display.version) {
        const layout = layoutFromDisplay(display);
        const t0 = performance.now();
        const p = display.payload;
        rig.planes = layout ? buildCellPlanes(layout, p.positions, p.indices, p.indexCount, payloadFaceKinds(p)) : null;
        rig.planesVersion = display.version;
        viewportBus.planes = rig.planes;
        viewportBus.planesMs = performance.now() - t0;
        setPlanes(mats.heightfield, rig.planes);
      }
      const kinds = display ? payloadFaceKinds(display.payload) : null;
      if (mode === 'relief') {
        const range = rig.planes ? [rig.planes.zMinFacet, rig.planes.zMax] : display ? [display.payload.bboxMin[2], display.payload.bboxMax[2]] : [0, 1];
        mats.relief.uniforms.uRange.value.set(range[0], range[1]);
        mats.reliefKinds.uniforms.uRange.value.set(range[0], range[1]);
        if (display && kinds && rig.reliefVersion !== display.version) {
          const p = display.payload;
          const d = deindexWithKinds(p.positions, p.indices, p.indexCount, kinds);
          const g = new THREE.BufferGeometry();
          g.setAttribute('position', new THREE.BufferAttribute(d.position, 3));
          g.setAttribute('aKind', new THREE.BufferAttribute(d.kind, 1));
          g.setAttribute('aPart', new THREE.BufferAttribute(d.part, 1));
          g.boundingBox = rig.mesh.geometry.boundingBox?.clone() ?? null;
          g.boundingSphere = rig.mesh.geometry.boundingSphere?.clone() ?? null;
          rig.relief.geometry.dispose();
          rig.relief.geometry = g;
          rig.reliefVersion = display.version;
        }
      }
      const useKinds = mode === 'relief' && kinds !== null;
      rig.relief.visible = useKinds;
      rig.mesh.visible = !useKinds;
      rig.mesh.material = mode === 'analytic' ? mats.heightfield : mode === 'relief' ? mats.relief : mats.standard;
      rig.mesh.receiveShadow = mode === 'shadowMap';
      viewportBus.renderMode = mode;
    };

    const apply = (display: DisplayMesh | null) => {
      if (!display) return;
      const t0 = performance.now();
      const result = applyPayload(slotRef.current, display.payload);
      const mesh = rig.mesh;
      if (mesh.geometry !== result.slot.geometry) {
        const old = mesh.geometry;
        mesh.geometry = result.slot.geometry;
        if (old !== result.replaced) old.dispose(); // the initial placeholder
      }
      result.replaced?.dispose();
      slotRef.current = result.slot;
      rig.display = display;
      refresh(useViewportStore.getState().renderMode);
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
    refresh(useViewportStore.getState().renderMode);
    const unsubMesh = useAppStore.subscribe((state, prev) => {
      if (state.mesh !== prev.mesh) apply(state.mesh);
    });
    const unsubMode = useViewportStore.subscribe((state, prev) => {
      if (state.renderMode !== prev.renderMode) {
        refresh(state.renderMode);
        invalidate();
      }
    });
    return () => {
      unsubMesh();
      unsubMode();
    };
  }, [rig, invalidate]);

  useEffect(
    () => () => {
      rig.mesh.geometry.dispose();
      rig.relief.geometry.dispose();
    },
    [rig],
  );

  return (
    <>
      <primitive object={rig.mesh} />
      <primitive object={rig.relief} />
    </>
  );
}
