// Turns mesh payloads into a three.js BufferGeometry while reusing GPU buffers where possible.
//
//   swap  same vertex and index counts as the current buffers: point the attributes at the new
//         payload arrays (no JS copy) and re-upload in place with bufferSubData.
//   copy  fits in buffers we allocated ourselves: copy into them and upload only the used range.
//   new   otherwise: a fresh geometry. The first mesh and very large meshes attach the payload
//         views directly (zero-copy); later ones get 25% headroom so small size changes while a
//         slider is dragged land in "copy". The caller disposes the replaced geometry.

import * as THREE from 'three';
import type { MeshPayload } from '../engine/types';

export type GeometryMode = 'new' | 'swap' | 'copy';

export interface GeometrySlot {
  geometry: THREE.BufferGeometry;
  position: THREE.BufferAttribute;
  index: THREE.BufferAttribute;
  /** True when the arrays were allocated here and may be overwritten. */
  owned: boolean;
}

export interface ApplyResult {
  slot: GeometrySlot;
  mode: GeometryMode;
  /** The previous geometry when a new one was created; dispose it after swapping it out. */
  replaced: THREE.BufferGeometry | null;
}

export const HEADROOM = 1.25;
/** At or above this many indices (1M triangles and up) we never copy into headroom buffers. */
export const ZERO_COPY_INDEX_THRESHOLD = 3_000_000;
/** Reallocate when a mesh would use less than this fraction of the owned index buffer. */
const MIN_FILL = 0.25;

function finalize(geometry: THREE.BufferGeometry, payload: MeshPayload): void {
  geometry.setDrawRange(0, payload.indexCount);
  const box = geometry.boundingBox ?? new THREE.Box3();
  box.min.fromArray(payload.bboxMin);
  box.max.fromArray(payload.bboxMax);
  geometry.boundingBox = box;
  const sphere = geometry.boundingSphere ?? new THREE.Sphere();
  box.getBoundingSphere(sphere);
  geometry.boundingSphere = sphere;
}

function roundUpToMultiple(n: number, m: number): number {
  return Math.ceil(n / m) * m;
}

export function applyPayload(slot: GeometrySlot | null, payload: MeshPayload): ApplyResult {
  const nPos = payload.vertexCount * 3;
  const nIdx = payload.indexCount;

  if (slot) {
    const posArr = slot.position.array;
    const idxArr = slot.index.array;
    if (posArr.length === nPos && idxArr.length === nIdx) {
      slot.position.array = payload.positions;
      slot.position.clearUpdateRanges();
      slot.position.needsUpdate = true;
      slot.index.array = payload.indices;
      slot.index.clearUpdateRanges();
      slot.index.needsUpdate = true;
      slot.owned = false;
      finalize(slot.geometry, payload);
      return { slot, mode: 'swap', replaced: null };
    }
    if (
      slot.owned &&
      nPos <= posArr.length &&
      nIdx <= idxArr.length &&
      nIdx >= idxArr.length * MIN_FILL
    ) {
      (posArr as Float32Array).set(payload.positions);
      slot.position.clearUpdateRanges();
      slot.position.addUpdateRange(0, nPos);
      slot.position.needsUpdate = true;
      (idxArr as Uint32Array).set(payload.indices);
      slot.index.clearUpdateRanges();
      slot.index.addUpdateRange(0, nIdx);
      slot.index.needsUpdate = true;
      finalize(slot.geometry, payload);
      return { slot, mode: 'copy', replaced: null };
    }
  }

  const geometry = new THREE.BufferGeometry();
  let position: THREE.BufferAttribute;
  let index: THREE.BufferAttribute;
  let owned: boolean;
  if (!slot || nIdx >= ZERO_COPY_INDEX_THRESHOLD) {
    position = new THREE.BufferAttribute(payload.positions, 3);
    index = new THREE.BufferAttribute(payload.indices, 1);
    owned = false;
  } else {
    const posCap = roundUpToMultiple(Math.max(3, Math.ceil(nPos * HEADROOM)), 3);
    const idxCap = roundUpToMultiple(Math.max(3, Math.ceil(nIdx * HEADROOM)), 3);
    const posArr = new Float32Array(posCap);
    posArr.set(payload.positions);
    const idxArr = new Uint32Array(idxCap);
    idxArr.set(payload.indices);
    position = new THREE.BufferAttribute(posArr, 3).setUsage(THREE.DynamicDrawUsage);
    index = new THREE.BufferAttribute(idxArr, 1).setUsage(THREE.DynamicDrawUsage);
    owned = true;
  }
  geometry.setAttribute('position', position);
  geometry.setIndex(index);
  finalize(geometry, payload);
  return {
    slot: { geometry, position, index, owned },
    mode: 'new',
    replaced: slot ? slot.geometry : null,
  };
}
