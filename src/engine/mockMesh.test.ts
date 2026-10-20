import { describe, expect, it } from 'vitest';
import { DEFAULT_PARAMS } from '../state/store';
import { buildMockSquama, buildSyntheticMesh, type RawMesh } from './mockMesh';
import { findOutOfRangeIndex } from './payload';

function signedVolume(m: RawMesh): number {
  const p = m.positions;
  let v = 0;
  for (let t = 0; t < m.indexCount; t += 3) {
    const a = m.indices[t] * 3, b = m.indices[t + 1] * 3, c = m.indices[t + 2] * 3;
    v +=
      p[a] * (p[b + 1] * p[c + 2] - p[b + 2] * p[c + 1]) -
      p[a + 1] * (p[b] * p[c + 2] - p[b + 2] * p[c]) +
      p[a + 2] * (p[b] * p[c + 1] - p[b + 1] * p[c]);
  }
  return v / 6;
}

function degenerateTriangles(m: RawMesh): number {
  const p = m.positions;
  let n = 0;
  for (let t = 0; t < m.indexCount; t += 3) {
    const a = m.indices[t] * 3, b = m.indices[t + 1] * 3, c = m.indices[t + 2] * 3;
    const ux = p[b] - p[a], uy = p[b + 1] - p[a + 1], uz = p[b + 2] - p[a + 2];
    const vx = p[c] - p[a], vy = p[c + 1] - p[a + 1], vz = p[c + 2] - p[a + 2];
    const cx = uy * vz - uz * vy, cy = uz * vx - ux * vz, cz = ux * vy - uy * vx;
    if (Math.hypot(cx, cy, cz) < 1e-9) n++;
  }
  return n;
}

describe('mock Squama plate', () => {
  it('builds the 50×63 default plate with sane bounds', () => {
    const m = buildMockSquama(DEFAULT_PARAMS);
    expect(m.nx).toBe(50);
    expect(m.ny).toBe(63);
    expect(m.bboxMin).toEqual([0, 0, 0]);
    expect(m.bboxMax[0]).toBe(300);
    expect(m.bboxMax[1]).toBe(378);
    expect(m.bboxMax[2]).toBeGreaterThan(DEFAULT_PARAMS.baseMm);
    expect(m.bboxMax[2]).toBeLessThan(DEFAULT_PARAMS.baseMm + 2);
    expect(m.indexCount % 3).toBe(0);
    expect(findOutOfRangeIndex(m.indices.subarray(0, m.indexCount), m.vertexCount)).toBe(-1);
  });

  it('keeps every facet corner at or above the base thickness', () => {
    const m = buildMockSquama({ ...DEFAULT_PARAMS, maxTiltDeg: 20, lightAzimuthDeg: 37 });
    const facetVerts = m.nx * m.ny * 4;
    for (let v = 0; v < facetVerts; v++) {
      expect(m.positions[v * 3 + 2]).toBeGreaterThanOrEqual(DEFAULT_PARAMS.baseMm - 1e-4);
    }
  });

  it('winds outward (positive volume close to the plate volume) and has no degenerate triangles', () => {
    const m = buildMockSquama(DEFAULT_PARAMS);
    const vol = signedVolume(m);
    expect(vol).toBeGreaterThan(300 * 378 * DEFAULT_PARAMS.baseMm);
    expect(vol).toBeLessThan(300 * 378 * m.bboxMax[2]);
    expect(degenerateTriangles(m)).toBe(0);
  });

  it('follows the width and cell size', () => {
    const m = buildMockSquama({ ...DEFAULT_PARAMS, widthMm: 600, cellMm: 6 });
    expect([m.nx, m.ny]).toEqual([100, 126]);
    expect(m.bboxMax[0]).toBe(600);
    expect(m.bboxMax[1]).toBe(756);
  });

  it('is deterministic', () => {
    const a = buildMockSquama(DEFAULT_PARAMS);
    const b = buildMockSquama(DEFAULT_PARAMS);
    expect(a.vertexCount).toBe(b.vertexCount);
    expect(a.indexCount).toBe(b.indexCount);
    expect(a.positions.subarray(0, a.vertexCount * 3)).toEqual(b.positions.subarray(0, b.vertexCount * 3));
  });
});

describe('synthetic mesh', () => {
  it.each([1, 2, 3, 999, 1000, 12345])('has exactly %i triangles', (n) => {
    const m = buildSyntheticMesh(n);
    expect(m.indexCount).toBe(n * 3);
    expect(m.indices.length).toBe(n * 3);
    expect(findOutOfRangeIndex(m.indices, m.vertexCount)).toBe(-1);
    expect(degenerateTriangles(m)).toBe(0);
  });

  it('builds 1M triangles over the plate footprint', () => {
    const m = buildSyntheticMesh(1_000_000);
    expect(m.indexCount).toBe(3_000_000);
    expect(m.vertexCount).toBeGreaterThan(490_000);
    expect(m.vertexCount).toBeLessThan(520_000);
    expect(m.bboxMax[0]).toBe(300);
    expect(m.bboxMax[1]).toBe(378);
    expect(findOutOfRangeIndex(m.indices, m.vertexCount)).toBe(-1);
  });

  it('handles zero', () => {
    expect(buildSyntheticMesh(0).vertexCount).toBe(0);
  });
});
