// Per-triangle face kinds (docs/design/squama-mesher.md §12 FaceKind, §12.2 payload flags bit 1):
// a u16 per triangle, low byte = kind, high byte = part index, parsed by engine/payload.ts. For
// payloads without them (the mock engine, the spike), the viewport classifies by the normal.

import type { MeshPayload } from '../engine/types';

/** Order of §12 `FaceKind`: Facet, Wall, Back, Key, Stand. */
export const FACE_KIND = { facet: 0, wall: 1, back: 2, key: 3, stand: 4 } as const;
export type FaceKind = (typeof FACE_KIND)[keyof typeof FACE_KIND];

/** §12.2: flags bit 1 = face kinds follow the indices. */
export const FLAG_FACE_KINDS = 1 << 1;
/** §12.2: flags bit 0 = final quality. */
export const FLAG_FINAL = 1 << 0;

/** The payload's per-triangle face kinds, or null when it carries none. */
export function payloadFaceKinds(payload: MeshPayload): Uint16Array | null {
  const kinds = payload.faceKinds;
  if (!kinds || (payload.flags & FLAG_FACE_KINDS) === 0) return null;
  return kinds.length >= payload.triangleCount ? kinds : null;
}

/** Kind from a triangle's unit normal w component, for payloads without face kinds. */
export function classifyByNormal(nw: number): FaceKind {
  if (nw > 0.5) return FACE_KIND.facet;
  if (nw < -0.5) return FACE_KIND.back;
  return FACE_KIND.wall;
}

/** Kind of triangle `tri`: from the payload's kinds when given, else from its normal. */
export function faceKindOf(kinds: ArrayLike<number> | null, tri: number, nw: number): number {
  return kinds ? kinds[tri] & 0xff : classifyByNormal(nw);
}

/** Part index (high byte) of triangle `tri`; 0 without kinds. */
export function partOf(kinds: ArrayLike<number> | null, tri: number): number {
  return kinds ? (kinds[tri] >> 8) & 0xff : 0;
}
