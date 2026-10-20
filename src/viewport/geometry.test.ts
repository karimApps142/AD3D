import { describe, expect, it } from 'vitest';
import { encodeMeshPayload, parseMeshPayload } from '../engine/payload';
import { applyPayload, ZERO_COPY_INDEX_THRESHOLD } from './geometry';

function grid(n: number) {
  // n triangles in a strip, enough to vary sizes.
  const verts = n + 2;
  const positions = new Float32Array(verts * 3);
  for (let i = 0; i < verts; i++) positions.set([i >> 1, i & 1, 0], i * 3);
  const indices = new Uint32Array(n * 3);
  for (let t = 0; t < n; t++) indices.set(t & 1 ? [t, t + 2, t + 1] : [t, t + 1, t + 2], t * 3);
  return parseMeshPayload(encodeMeshPayload({ positions, indices }));
}

describe('applyPayload', () => {
  it('attaches the first payload zero-copy', () => {
    const p = grid(10);
    const r = applyPayload(null, p);
    expect(r.mode).toBe('new');
    expect(r.replaced).toBeNull();
    expect(r.slot.owned).toBe(false);
    expect(r.slot.position.array).toBe(p.positions);
    expect(r.slot.index.array).toBe(p.indices);
    expect(r.slot.geometry.drawRange.count).toBe(30);
    expect(r.slot.geometry.boundingBox?.max.x).toBe(5);
    expect(r.slot.geometry.boundingSphere?.radius).toBeGreaterThan(0);
  });

  it('swaps arrays when sizes match exactly', () => {
    const first = applyPayload(null, grid(10));
    const p2 = grid(10);
    const r = applyPayload(first.slot, p2);
    expect(r.mode).toBe('swap');
    expect(r.slot.geometry).toBe(first.slot.geometry);
    expect(r.slot.position.array).toBe(p2.positions);
    expect(r.slot.position.version).toBeGreaterThan(0);
  });

  it('reallocates with headroom, then copies smaller and slightly larger meshes in place', () => {
    const first = applyPayload(null, grid(100));
    const grown = applyPayload(first.slot, grid(110));
    expect(grown.mode).toBe('new');
    expect(grown.replaced).toBe(first.slot.geometry);
    expect(grown.slot.owned).toBe(true);
    expect(grown.slot.index.array.length).toBeGreaterThanOrEqual(110 * 3 * 1.25);

    const p = grid(120);
    const copied = applyPayload(grown.slot, p);
    expect(copied.mode).toBe('copy');
    expect(copied.slot.geometry).toBe(grown.slot.geometry);
    expect(copied.slot.geometry.drawRange.count).toBe(360);
    expect(Array.from(copied.slot.index.array.subarray(0, 360))).toEqual(Array.from(p.indices));
    expect(copied.slot.index.updateRanges).toEqual([{ start: 0, count: 360 }]);
    expect(copied.slot.position.updateRanges).toEqual([{ start: 0, count: p.vertexCount * 3 }]);

    const smaller = applyPayload(copied.slot, grid(90));
    expect(smaller.mode).toBe('copy');
    expect(smaller.slot.geometry.drawRange.count).toBe(270);
  });

  it('shrinks when a mesh would use under a quarter of the buffer', () => {
    const a = applyPayload(null, grid(1000));
    const b = applyPayload(a.slot, grid(1100)); // owned, headroom
    const c = applyPayload(b.slot, grid(100));
    expect(c.mode).toBe('new');
    expect(c.slot.index.array.length).toBeLessThan(1000);
  });

  it('never copies into borrowed payload arrays', () => {
    const a = applyPayload(null, grid(100));
    const b = applyPayload(a.slot, grid(90));
    expect(b.mode).toBe('new');
    expect(b.slot.owned).toBe(true);
  });

  it('attaches very large meshes zero-copy even after a smaller one', () => {
    const a = applyPayload(null, grid(10));
    const big = grid(ZERO_COPY_INDEX_THRESHOLD / 3 + 1);
    const r = applyPayload(a.slot, big);
    expect(r.mode).toBe('new');
    expect(r.slot.owned).toBe(false);
    expect(r.slot.index.array).toBe(big.indices);
  });

  it('treats exactly 1M triangles (3M indices) as very large', () => {
    const a = applyPayload(null, grid(10));
    const big = grid(ZERO_COPY_INDEX_THRESHOLD / 3);
    expect(big.indexCount).toBe(ZERO_COPY_INDEX_THRESHOLD);
    const r = applyPayload(a.slot, big);
    expect(r.mode).toBe('new');
    expect(r.slot.owned).toBe(false);
    // The same size again reuses the buffers in place ('swap'), not a copy.
    expect(applyPayload(r.slot, grid(ZERO_COPY_INDEX_THRESHOLD / 3)).mode).toBe('swap');
  });
});
