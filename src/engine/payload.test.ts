import { describe, expect, it } from 'vitest';
import {
  computeBBox,
  encodeMeshPayload,
  expectedPayloadBytes,
  findOutOfRangeIndex,
  HEADER_BYTES,
  HEADER_OFFSETS,
  MESH_MAGIC,
  parseMeshPayload,
  PayloadError,
  setPayloadTimes,
} from './payload';

function tetrahedron() {
  const positions = new Float32Array([0, 0, 0, 10, 0, 0, 0, 20, 0, 0, 0, 30]);
  const indices = new Uint32Array([0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3]);
  return { positions, indices };
}

describe('mesh payload', () => {
  it('magic spells "AD3M" in little-endian byte order', () => {
    const buf = encodeMeshPayload(tetrahedron());
    expect(new TextDecoder().decode(new Uint8Array(buf, 0, 4))).toBe('AD3M');
    expect(new DataView(buf).getUint32(0, true)).toBe(MESH_MAGIC);
  });

  it('round-trips header fields and arrays', () => {
    const { positions, indices } = tetrahedron();
    const buf = encodeMeshPayload({ positions, indices, genMs: 1.5, encodeMs: 0.25, jobId: 42, flags: 0 });
    expect(buf.byteLength).toBe(HEADER_BYTES + 4 * 12 + 12 * 4);
    const p = parseMeshPayload(buf, { checkIndices: true });
    expect(p.version).toBe(1);
    expect(p.vertexCount).toBe(4);
    expect(p.indexCount).toBe(12);
    expect(p.triangleCount).toBe(4);
    expect(p.bboxMin).toEqual([0, 0, 0]);
    expect(p.bboxMax).toEqual([10, 20, 30]);
    expect(p.genMs).toBeCloseTo(1.5, 5);
    expect(p.encodeMs).toBeCloseTo(0.25, 5);
    expect(p.jobId).toBe(42);
    expect(p.flags).toBe(0);
    expect(Array.from(p.positions)).toEqual(Array.from(positions));
    expect(Array.from(p.indices)).toEqual(Array.from(indices));
    expect(p.byteLength).toBe(buf.byteLength);
  });

  it('header offsets match the shared contract', () => {
    const buf = encodeMeshPayload({ ...tetrahedron(), jobId: 7, genMs: 3, encodeMs: 4 });
    const dv = new DataView(buf);
    expect(dv.getUint32(4, true)).toBe(1);
    expect(dv.getUint32(8, true)).toBe(4);
    expect(dv.getUint32(12, true)).toBe(12);
    expect(dv.getFloat32(28 + 4, true)).toBe(20);
    expect(dv.getFloat32(40, true)).toBe(3);
    expect(dv.getFloat32(44, true)).toBe(4);
    expect(dv.getUint32(48, true)).toBe(7);
    expect(dv.getUint32(52, true)).toBe(0);
    expect(dv.getUint32(56, true)).toBe(0);
    expect(dv.getUint32(60, true)).toBe(0);
    expect(dv.getFloat32(64, true)).toBe(0);
    expect(dv.getFloat32(64 + 3 * 4, true)).toBe(10);
    expect(dv.getUint32(64 + 4 * 12 + 4, true)).toBe(2);
    expect(HEADER_OFFSETS.jobId).toBe(48);
  });

  it('returns zero-copy views into the received buffer', () => {
    const buf = encodeMeshPayload(tetrahedron());
    const p = parseMeshPayload(buf);
    expect(p.payloadZeroCopy).toBe(true);
    expect(p.positions.buffer).toBe(buf);
    expect(p.indices.buffer).toBe(buf);
    expect(p.positions.byteOffset).toBe(64);
    expect(p.indices.byteOffset).toBe(64 + 4 * 12);
  });

  it('accepts an aligned Uint8Array view without copying', () => {
    const inner = encodeMeshPayload(tetrahedron());
    const outer = new Uint8Array(inner.byteLength + 8);
    outer.set(new Uint8Array(inner), 8);
    const p = parseMeshPayload(outer.subarray(8));
    expect(p.payloadZeroCopy).toBe(true);
    expect(p.positions.buffer).toBe(outer.buffer);
    expect(p.vertexCount).toBe(4);
  });

  it('copies an unaligned view so typed arrays can be built', () => {
    const inner = encodeMeshPayload(tetrahedron());
    const outer = new Uint8Array(inner.byteLength + 3);
    outer.set(new Uint8Array(inner), 3);
    const p = parseMeshPayload(outer.subarray(3));
    expect(p.payloadZeroCopy).toBe(false);
    expect(Array.from(p.indices)).toEqual(Array.from(tetrahedron().indices));
  });

  it('rejects a bad magic', () => {
    const buf = encodeMeshPayload(tetrahedron());
    new DataView(buf).setUint32(0, 0x12345678, true);
    expect(() => parseMeshPayload(buf)).toThrow(PayloadError);
    expect(() => parseMeshPayload(buf)).toThrow(/bad magic 0x12345678/);
  });

  it('rejects an unknown version', () => {
    const buf = encodeMeshPayload(tetrahedron());
    new DataView(buf).setUint32(4, 2, true);
    expect(() => parseMeshPayload(buf)).toThrow(/version 2/);
  });

  it('rejects a truncated header', () => {
    expect(() => parseMeshPayload(new ArrayBuffer(63))).toThrow(/too short/);
  });

  it('rejects truncated and oversized bodies', () => {
    const buf = encodeMeshPayload(tetrahedron());
    expect(() => parseMeshPayload(buf.slice(0, buf.byteLength - 4))).toThrow(/size mismatch/);
    const bigger = new Uint8Array(buf.byteLength + 4);
    bigger.set(new Uint8Array(buf));
    expect(() => parseMeshPayload(bigger.buffer)).toThrow(/size mismatch/);
  });

  it('rejects an index count that is not a multiple of 3', () => {
    const buf = new ArrayBuffer(expectedPayloadBytes(0, 4));
    const dv = new DataView(buf);
    dv.setUint32(0, MESH_MAGIC, true);
    dv.setUint32(4, 1, true);
    dv.setUint32(12, 4, true);
    expect(() => parseMeshPayload(buf)).toThrow(/multiple of 3/);
  });

  it('rejects non-finite or inverted bounds', () => {
    const buf = encodeMeshPayload(tetrahedron());
    new DataView(buf).setFloat32(16, Number.NaN, true);
    expect(() => parseMeshPayload(buf)).toThrow(/bounding box/);
    const buf2 = encodeMeshPayload({ ...tetrahedron(), bboxMin: [5, 0, 0], bboxMax: [1, 1, 1] });
    expect(() => parseMeshPayload(buf2)).toThrow(/bounding box/);
  });

  it('finds out-of-range indices only when asked', () => {
    const { positions } = tetrahedron();
    const buf = encodeMeshPayload({ positions, indices: new Uint32Array([0, 1, 4]) });
    expect(() => parseMeshPayload(buf)).not.toThrow();
    expect(() => parseMeshPayload(buf, { checkIndices: true })).toThrow(/vertex 4/);
    expect(findOutOfRangeIndex(new Uint32Array([0, 1, 2]), 3)).toBe(-1);
    expect(findOutOfRangeIndex(new Uint32Array([0, 3, 2]), 3)).toBe(1);
  });

  it('handles an empty mesh', () => {
    const p = parseMeshPayload(encodeMeshPayload({ positions: new Float32Array(0), indices: new Uint32Array(0) }));
    expect(p.vertexCount).toBe(0);
    expect(p.triangleCount).toBe(0);
    expect(p.byteLength).toBe(64);
  });

  it('encodes only the requested prefix of oversized arrays', () => {
    const positions = new Float32Array(30);
    positions.set([1, 2, 3, 4, 5, 6, 7, 8, 9]);
    const indices = new Uint32Array(30);
    indices.set([0, 1, 2]);
    const p = parseMeshPayload(encodeMeshPayload({ positions, indices, vertexCount: 3, indexCount: 3 }));
    expect(p.vertexCount).toBe(3);
    expect(p.indexCount).toBe(3);
    expect(p.bboxMin).toEqual([1, 2, 3]);
    expect(p.bboxMax).toEqual([7, 8, 9]);
  });

  it('patches gen and encode times in place', () => {
    const buf = encodeMeshPayload(tetrahedron());
    setPayloadTimes(buf, 12.5, 0.5);
    const p = parseMeshPayload(buf);
    expect(p.genMs).toBe(12.5);
    expect(p.encodeMs).toBe(0.5);
  });

  it('rejects non-binary input', () => {
    expect(() => parseMeshPayload([1, 2, 3] as unknown as ArrayBuffer)).toThrow(/ArrayBuffer/);
  });

  it('computes bounds over the vertex prefix', () => {
    const b = computeBBox(new Float32Array([1, 2, 3, -1, 5, 0, 100, 100, 100]), 2);
    expect(b.min).toEqual([-1, 2, 0]);
    expect(b.max).toEqual([1, 5, 3]);
  });
});
