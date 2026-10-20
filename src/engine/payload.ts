// Binary mesh payload: the one wire format for meshes going from a generator to the viewport.
//
// Little-endian, 64-byte header, then the vertex and index arrays:
//
//   offset  type     field
//   0       u32      magic 0x4D334441 (the bytes "AD3M")
//   4       u32      version = 1
//   8       u32      vertex_count
//   12      u32      index_count (u32 indices; triangles = index_count / 3)
//   16      f32[3]   bbox_min
//   28      f32[3]   bbox_max
//   40      f32      gen_ms     (producer's generation time)
//   44      f32      encode_ms  (producer's encode time)
//   48      u32      job_id     (echo of the request's jobId)
//   52      u32      flags: bit 0 = final quality, bit 1 = face kinds follow (§12.2)
//   56      u32[2]   reserved
//   64      f32      positions[3 * vertex_count]
//   ...     u32      indices[index_count]
//   ...     u16      face kinds[triangles] (flags bit 1 only; low byte = kind, high byte = part),
//                    padded to 4 bytes
//
// Every section starts on a 4-byte boundary, so on a little-endian host (x86_64, arm64) the arrays
// are exposed as zero-copy Float32Array/Uint32Array views into the received buffer.

import type { MeshPayload, Vec3 } from './types';

export const MESH_MAGIC = 0x4d334441;
export const MESH_VERSION = 1;
export const HEADER_BYTES = 64;

export const HEADER_OFFSETS = {
  magic: 0,
  version: 4,
  vertexCount: 8,
  indexCount: 12,
  bboxMin: 16,
  bboxMax: 28,
  genMs: 40,
  encodeMs: 44,
  jobId: 48,
  flags: 52,
  reserved: 56,
} as const;

export class PayloadError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'PayloadError';
  }
}

export const HOST_LITTLE_ENDIAN =
  new Uint8Array(new Uint32Array([0x01020304]).buffer)[0] === 0x04;

/** §12.2 flags bit 1: a u16 face kind per triangle follows the indices. */
export const PAYLOAD_FLAG_FACE_KINDS = 1 << 1;

/** Bytes of the face-kind section for `triangles` triangles (u16 each, padded to 4). */
export function faceKindBytes(triangles: number): number {
  return Math.ceil((2 * triangles) / 4) * 4;
}

export function expectedPayloadBytes(vertexCount: number, indexCount: number, flags = 0): number {
  const kinds = flags & PAYLOAD_FLAG_FACE_KINDS ? faceKindBytes(indexCount / 3) : 0;
  return HEADER_BYTES + vertexCount * 12 + indexCount * 4 + kinds;
}

function isArrayBuffer(x: unknown): x is ArrayBuffer {
  return x instanceof ArrayBuffer || Object.prototype.toString.call(x) === '[object ArrayBuffer]';
}

function describe(x: unknown): string {
  if (x === null) return 'null';
  if (Array.isArray(x)) return `array(${x.length})`;
  return Object.prototype.toString.call(x);
}

function hex32(n: number): string {
  return '0x' + (n >>> 0).toString(16).toUpperCase().padStart(8, '0');
}

export interface ParseOptions {
  /** Scan all indices and reject any >= vertex_count. O(n); off by default on the hot path. */
  checkIndices?: boolean;
}

/**
 * Validates the header and returns typed-array views over the payload. Throws PayloadError on any
 * mismatch: wrong magic or version, truncated or oversized buffer, index count not a multiple of 3,
 * non-finite bounds, or (with checkIndices) an out-of-range index.
 */
export function parseMeshPayload(
  input: ArrayBuffer | ArrayBufferView,
  options: ParseOptions = {},
): MeshPayload {
  let buffer: ArrayBuffer;
  let base = 0;
  let length: number;
  let payloadZeroCopy = true;

  if (isArrayBuffer(input)) {
    buffer = input;
    length = input.byteLength;
  } else if (ArrayBuffer.isView(input)) {
    length = input.byteLength;
    if (input.byteOffset % 4 === 0 && isArrayBuffer(input.buffer)) {
      buffer = input.buffer;
      base = input.byteOffset;
    } else {
      // Unaligned (or shared) memory cannot back Float32Array/Uint32Array views: copy once.
      const copy = new Uint8Array(length);
      copy.set(new Uint8Array(input.buffer, input.byteOffset, length));
      buffer = copy.buffer;
      payloadZeroCopy = false;
    }
  } else {
    throw new PayloadError(`expected an ArrayBuffer or typed array, got ${describe(input)}`);
  }

  if (length < HEADER_BYTES) {
    throw new PayloadError(`payload too short: ${length} bytes, the header alone is ${HEADER_BYTES}`);
  }

  const dv = new DataView(buffer, base, length);
  const magic = dv.getUint32(HEADER_OFFSETS.magic, true);
  if (magic !== MESH_MAGIC) {
    throw new PayloadError(`bad magic ${hex32(magic)}, expected ${hex32(MESH_MAGIC)} ("AD3M")`);
  }
  const version = dv.getUint32(HEADER_OFFSETS.version, true);
  if (version !== MESH_VERSION) {
    throw new PayloadError(`unsupported payload version ${version}, expected ${MESH_VERSION}`);
  }
  const vertexCount = dv.getUint32(HEADER_OFFSETS.vertexCount, true);
  const indexCount = dv.getUint32(HEADER_OFFSETS.indexCount, true);
  if (indexCount % 3 !== 0) {
    throw new PayloadError(`index_count ${indexCount} is not a multiple of 3`);
  }
  const flags = dv.getUint32(HEADER_OFFSETS.flags, true);
  const expected = expectedPayloadBytes(vertexCount, indexCount, flags);
  if (length !== expected) {
    throw new PayloadError(
      `size mismatch: header says ${expected} bytes (vertex_count ${vertexCount}, ` +
        `index_count ${indexCount}), buffer has ${length}`,
    );
  }

  const bboxMin: Vec3 = [
    dv.getFloat32(HEADER_OFFSETS.bboxMin, true),
    dv.getFloat32(HEADER_OFFSETS.bboxMin + 4, true),
    dv.getFloat32(HEADER_OFFSETS.bboxMin + 8, true),
  ];
  const bboxMax: Vec3 = [
    dv.getFloat32(HEADER_OFFSETS.bboxMax, true),
    dv.getFloat32(HEADER_OFFSETS.bboxMax + 4, true),
    dv.getFloat32(HEADER_OFFSETS.bboxMax + 8, true),
  ];
  if (vertexCount > 0) {
    for (let i = 0; i < 3; i++) {
      if (!Number.isFinite(bboxMin[i]) || !Number.isFinite(bboxMax[i]) || bboxMin[i] > bboxMax[i]) {
        throw new PayloadError(
          `invalid bounding box min=${JSON.stringify(bboxMin)} max=${JSON.stringify(bboxMax)}`,
        );
      }
    }
  }

  const posOffset = base + HEADER_BYTES;
  const idxOffset = posOffset + vertexCount * 12;
  const kindOffset = idxOffset + indexCount * 4;
  const triangleCount = indexCount / 3;
  let positions: Float32Array<ArrayBuffer>;
  let indices: Uint32Array<ArrayBuffer>;
  let faceKinds: Uint16Array<ArrayBuffer> | null = null;
  if (HOST_LITTLE_ENDIAN) {
    positions = new Float32Array(buffer, posOffset, vertexCount * 3);
    indices = new Uint32Array(buffer, idxOffset, indexCount);
    if (flags & PAYLOAD_FLAG_FACE_KINDS) faceKinds = new Uint16Array(buffer, kindOffset, triangleCount);
  } else {
    // Big-endian hosts (none we ship to) get a decoded copy.
    payloadZeroCopy = false;
    positions = new Float32Array(vertexCount * 3);
    indices = new Uint32Array(indexCount);
    const all = new DataView(buffer);
    for (let i = 0; i < positions.length; i++) positions[i] = all.getFloat32(posOffset + i * 4, true);
    for (let i = 0; i < indices.length; i++) indices[i] = all.getUint32(idxOffset + i * 4, true);
    if (flags & PAYLOAD_FLAG_FACE_KINDS) {
      faceKinds = new Uint16Array(triangleCount);
      for (let i = 0; i < triangleCount; i++) faceKinds[i] = all.getUint16(kindOffset + i * 2, true);
    }
  }

  if (options.checkIndices) {
    const bad = findOutOfRangeIndex(indices, vertexCount);
    if (bad >= 0) {
      throw new PayloadError(
        `index ${bad} refers to vertex ${indices[bad]}, but vertex_count is ${vertexCount}`,
      );
    }
  }

  return {
    version,
    vertexCount,
    indexCount,
    triangleCount,
    bboxMin,
    bboxMax,
    genMs: dv.getFloat32(HEADER_OFFSETS.genMs, true),
    encodeMs: dv.getFloat32(HEADER_OFFSETS.encodeMs, true),
    jobId: dv.getUint32(HEADER_OFFSETS.jobId, true),
    flags,
    positions,
    indices,
    faceKinds,
    byteLength: length,
    payloadZeroCopy,
  };
}

/** Position of the first index >= vertexCount, or -1 when every index is in range. */
export function findOutOfRangeIndex(indices: Uint32Array, vertexCount: number): number {
  for (let i = 0; i < indices.length; i++) {
    if (indices[i] >= vertexCount) return i;
  }
  return -1;
}

export function computeBBox(
  positions: Float32Array,
  vertexCount = Math.floor(positions.length / 3),
): { min: Vec3; max: Vec3 } {
  if (vertexCount === 0) return { min: [0, 0, 0], max: [0, 0, 0] };
  let x0 = Infinity, y0 = Infinity, z0 = Infinity;
  let x1 = -Infinity, y1 = -Infinity, z1 = -Infinity;
  for (let i = 0, n = vertexCount * 3; i < n; i += 3) {
    const x = positions[i], y = positions[i + 1], z = positions[i + 2];
    if (x < x0) x0 = x;
    if (x > x1) x1 = x;
    if (y < y0) y0 = y;
    if (y > y1) y1 = y;
    if (z < z0) z0 = z;
    if (z > z1) z1 = z;
  }
  return { min: [x0, y0, z0], max: [x1, y1, z1] };
}

export interface EncodeInput {
  /** xyz per vertex; may be longer than 3 * vertexCount (only the prefix is written). */
  positions: Float32Array;
  /** Triangle indices; may be longer than indexCount. */
  indices: Uint32Array;
  vertexCount?: number;
  indexCount?: number;
  bboxMin?: Vec3;
  bboxMax?: Vec3;
  genMs?: number;
  encodeMs?: number;
  jobId?: number;
  flags?: number;
}

/** Writes a payload in the wire format above. Used by the mock engine and by tests. */
export function encodeMeshPayload(input: EncodeInput): ArrayBuffer {
  const vertexCount = input.vertexCount ?? Math.floor(input.positions.length / 3);
  const indexCount = input.indexCount ?? input.indices.length;
  if (!Number.isInteger(vertexCount) || vertexCount < 0 || vertexCount * 3 > input.positions.length) {
    throw new RangeError(`vertexCount ${vertexCount} exceeds positions (${input.positions.length / 3})`);
  }
  if (!Number.isInteger(indexCount) || indexCount < 0 || indexCount > input.indices.length) {
    throw new RangeError(`indexCount ${indexCount} exceeds indices (${input.indices.length})`);
  }
  if (indexCount % 3 !== 0) throw new RangeError(`indexCount ${indexCount} is not a multiple of 3`);

  const bbox =
    input.bboxMin && input.bboxMax
      ? { min: input.bboxMin, max: input.bboxMax }
      : computeBBox(input.positions, vertexCount);

  const buffer = new ArrayBuffer(expectedPayloadBytes(vertexCount, indexCount));
  const dv = new DataView(buffer);
  const O = HEADER_OFFSETS;
  dv.setUint32(O.magic, MESH_MAGIC, true);
  dv.setUint32(O.version, MESH_VERSION, true);
  dv.setUint32(O.vertexCount, vertexCount, true);
  dv.setUint32(O.indexCount, indexCount, true);
  for (let i = 0; i < 3; i++) {
    dv.setFloat32(O.bboxMin + i * 4, bbox.min[i], true);
    dv.setFloat32(O.bboxMax + i * 4, bbox.max[i], true);
  }
  dv.setFloat32(O.genMs, input.genMs ?? 0, true);
  dv.setFloat32(O.encodeMs, input.encodeMs ?? 0, true);
  dv.setUint32(O.jobId, (input.jobId ?? 0) >>> 0, true);
  dv.setUint32(O.flags, (input.flags ?? 0) >>> 0, true);

  const posOffset = HEADER_BYTES;
  const idxOffset = HEADER_BYTES + vertexCount * 12;
  if (HOST_LITTLE_ENDIAN) {
    new Float32Array(buffer, posOffset, vertexCount * 3).set(
      input.positions.subarray(0, vertexCount * 3),
    );
    new Uint32Array(buffer, idxOffset, indexCount).set(input.indices.subarray(0, indexCount));
  } else {
    for (let i = 0; i < vertexCount * 3; i++) dv.setFloat32(posOffset + i * 4, input.positions[i], true);
    for (let i = 0; i < indexCount; i++) dv.setUint32(idxOffset + i * 4, input.indices[i], true);
  }
  return buffer;
}

/** Overwrites gen_ms / encode_ms in an encoded payload (the encoder cannot time itself). */
export function setPayloadTimes(buffer: ArrayBuffer, genMs: number, encodeMs: number): void {
  const dv = new DataView(buffer);
  dv.setFloat32(HEADER_OFFSETS.genMs, genMs, true);
  dv.setFloat32(HEADER_OFFSETS.encodeMs, encodeMs, true);
}
