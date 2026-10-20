import { describe, expect, it } from 'vitest';
import { buildMockSquama } from '../engine/mockMesh';
import { degToRad, lightDirection } from '../engine/squamaMath';
import { DEFAULT_PARAMS } from '../state/store';
import {
  buildCellPlanes,
  type CellLayout,
  cellCenter,
  cellExit,
  type CellPlanes,
  gridSize,
  lookupCell,
  planeAt,
  planesFromFunction,
  shadeRadiance,
  traceShadow,
} from './heightfield';

/** Deterministic PRNG (mulberry32). */
function rng(seed: number) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const LAYOUTS: CellLayout[] = [
  { kind: 'square', cellMm: 6, widthMm: 60, heightMm: 72 },
  { kind: 'hex', orientation: 'pointy', cellMm: 6, widthMm: 60, heightMm: 6 * (Math.sqrt(3) / 2) * 12 },
  { kind: 'hex', orientation: 'flat', cellMm: 5, widthMm: 5 * (Math.sqrt(3) / 2) * 12, heightMm: 50 },
];

function bruteNearest(l: CellLayout, u: number, v: number): [number, number] {
  const { cols, rows } = gridSize(l);
  let best = Infinity;
  let out: [number, number] = [0, 0];
  for (let r = 0; r < rows; r++) {
    for (let c = 0; c < cols; c++) {
      const [cu, cv] = cellCenter(l, c, r);
      const d = (u - cu) ** 2 + (v - cv) ** 2;
      if (d < best) {
        best = d;
        out = [c, r];
      }
    }
  }
  return out;
}

describe('cell lookup', () => {
  it('square cells are floor(u / cell)', () => {
    const l = LAYOUTS[0];
    expect(lookupCell(l, 0.1, 0.1)).toEqual([0, 0]);
    expect(lookupCell(l, 6.01, 11.99)).toEqual([1, 1]);
    expect(lookupCell(l, 59.9, 71.9)).toEqual([9, 11]);
    expect(cellCenter(l, 2, 3)).toEqual([15, 21]);
  });

  for (const l of LAYOUTS.slice(1)) {
    it(`${l.kind === 'hex' ? l.orientation : ''} hex lookup is the nearest centre`, () => {
      const r = rng(7);
      for (let n = 0; n < 4000; n++) {
        const u = r() * l.widthMm;
        const v = r() * l.heightMm;
        const got = lookupCell(l, u, v);
        const want = bruteNearest(l, u, v);
        const [gu, gv] = cellCenter(l, got[0], got[1]);
        const [wu, wv] = cellCenter(l, want[0], want[1]);
        // Equal, or a tie on a Voronoi edge.
        expect(Math.hypot(u - gu, v - gv)).toBeCloseTo(Math.hypot(u - wu, v - wv), 9);
      }
    });
  }

  it('hex centres follow §6.2 (centre at plate origin, odd rows shifted half a cell)', () => {
    const l = LAYOUTS[1];
    expect(lookupCell(l, 0.01, 0.01)).toEqual([1, 0]);
    const [u0, v0] = cellCenter(l, 1, 0);
    expect(u0).toBe(0);
    expect(v0).toBe(0);
    const [u1, v1] = cellCenter(l, 1, 1);
    expect(u1).toBeCloseTo(3, 12);
    expect(v1).toBeCloseTo(6 * Math.sqrt(3) / 2, 12);
  });
});

describe('cell exit', () => {
  for (const l of LAYOUTS) {
    it(`${l.kind}${l.kind === 'hex' ? '-' + l.orientation : ''}: the exit point is on the cell boundary`, () => {
      const r = rng(11);
      const grid = gridSize(l);
      let checked = 0;
      for (let n = 0; n < 3000; n++) {
        const u = l.widthMm * (0.1 + 0.8 * r());
        const v = l.heightMm * (0.1 + 0.8 * r());
        const a = r() * Math.PI * 2;
        const du = Math.cos(a);
        const dv = Math.sin(a);
        const [c, w] = lookupCell(l, u, v, grid);
        const t = cellExit(l, c, w, u, v, du, dv);
        expect(t).toBeGreaterThanOrEqual(0);
        expect(t).toBeLessThanOrEqual(l.cellMm * Math.SQRT2 + 1e-9);
        const before = lookupCell(l, u + du * (t - 1e-4), v + dv * (t - 1e-4), grid);
        const after = lookupCell(l, u + du * (t + 1e-4), v + dv * (t + 1e-4), grid);
        if (t > 2e-4) expect(before).toEqual([c, w]);
        const ue = u + du * (t + 1e-4);
        const ve = v + dv * (t + 1e-4);
        if (ue > 0 && ve > 0 && ue < l.widthMm && ve < l.heightMm) {
          expect(after).not.toEqual([c, w]);
          checked++;
        }
      }
      expect(checked).toBeGreaterThan(2000);
    });
  }
});

/** Plate of square cells with a raised first column: a 1 mm step at u = 6. */
function stepPlate(): CellPlanes {
  const l: CellLayout = { kind: 'square', cellMm: 6, widthMm: 120, heightMm: 36 };
  return planesFromFunction(l, (col) => ({ h: col === 0 ? 2 : 1, gu: 0, gv: 0 }));
}

describe('cast shadow trace', () => {
  it('a 1 mm step casts a shadow 1/tan(α) long under a directional light', () => {
    const pl = stepPlate();
    const el = degToRad(5);
    const dir = lightDirection(Math.PI, el); // light on the left
    const len = 1 / Math.tan(el); // 11.43 mm
    const at = (x: number) => traceShadow(pl, 6 + x, 18, 1, { kind: 'directional', dir }, { onFacet: true }).shadowed;
    expect(at(0.5)).toBe(true);
    expect(at(len - 0.1)).toBe(true);
    expect(at(len + 0.1)).toBe(false);
    expect(at(30)).toBe(false);
    // Light from the right: nothing occludes.
    const right = lightDirection(0, el);
    expect(traceShadow(pl, 6.5, 18, 1, { kind: 'directional', dir: right }, { onFacet: true }).shadowed).toBe(false);
  });

  it('the top of the step and the lit wall are not self-shadowed', () => {
    const pl = stepPlate();
    const dir = lightDirection(Math.PI, degToRad(5));
    for (const x of [0.01, 1, 3, 5.99]) {
      expect(traceShadow(pl, x, 18, 2, { kind: 'directional', dir }, { onFacet: true }).shadowed).toBe(false);
    }
    // The wall at u = 6 faces +u (away from a left light) – but light from the right lights it.
    const right = lightDirection(0, degToRad(5));
    for (const z of [1.05, 1.5, 1.95]) {
      expect(traceShadow(pl, 6, 18, z, { kind: 'directional', dir: right }).shadowed).toBe(false);
    }
  });

  it('point light: shadow length follows the lamp position', () => {
    const pl = stepPlate();
    // Lamp 100 mm left of the step edge at w = 1 + 10 mm: the ray grazes the step corner (6, ·, 2)
    // and reaches w = 1 at u = 6 + 100/9.
    const lamp = [6 - 100, 18, 11] as const;
    const edge = 6 + 100 / 9;
    const at = (u: number) => traceShadow(pl, u, 18, 1, { kind: 'point', pos: lamp }, { onFacet: true }).shadowed;
    expect(at(edge - 0.1)).toBe(true);
    expect(at(edge + 0.1)).toBe(false);
  });

  it('a point light below the occluder top does not see past it, one above does', () => {
    const pl = stepPlate();
    expect(traceShadow(pl, 30, 18, 1, { kind: 'point', pos: [-50, 18, 1.5] }, { onFacet: true }).shadowed).toBe(true);
    expect(traceShadow(pl, 30, 18, 1, { kind: 'point', pos: [3, 18, 30] }, { onFacet: true }).shadowed).toBe(false);
  });

  for (const l of LAYOUTS) {
    it(`${l.kind}${l.kind === 'hex' ? '-' + l.orientation : ''}: agrees with a brute-force march over random planes`, () => {
      const r = rng(1234);
      const tilt = degToRad(12);
      const pl = planesFromFunction(l, () => {
        const a = r() * Math.PI * 2;
        const g = Math.tan(tilt) * r();
        return { h: 3 + r() * 1.5, gu: g * Math.cos(a), gv: g * Math.sin(a) };
      });
      const grid = { cols: pl.cols, rows: pl.rows };
      const surface = (u: number, v: number) => {
        const [c, w] = lookupCell(l, u, v, grid);
        return planeAt(pl, c, w, u, v);
      };
      let agree = 0;
      let decided = 0;
      for (let n = 0; n < 250; n++) {
        const u = l.widthMm * (0.15 + 0.7 * r());
        const v = l.heightMm * (0.15 + 0.7 * r());
        const w = surface(u, v);
        const az = r() * Math.PI * 2;
        const el = degToRad(2 + r() * 25);
        const dir = lightDirection(az, el);
        const hl = Math.hypot(dir[0], dir[1]);
        const [c0, r0] = lookupCell(l, u, v, grid);
        // Brute force: largest (surface − ray) outside the start cell, sampled every 5 µm until the ray clears the top.
        let worst = -Infinity;
        let margin = Infinity;
        for (let t = 0.0; t < 400; t += 0.005) {
          if (w + (dir[2] / hl) * t > pl.zMax + 0.01) break;
          const x = u + (dir[0] / hl) * t;
          const y = v + (dir[1] / hl) * t;
          if (x < 0 || y < 0 || x > l.widthMm || y > l.heightMm) break;
          const cell = lookupCell(l, x, y, grid);
          if (cell[0] === c0 && cell[1] === r0) continue;
          const d = surface(x, y) - (w + (dir[2] / hl) * t);
          worst = Math.max(worst, d);
          margin = Math.min(margin, Math.abs(d - 0.003));
        }
        const got = traceShadow(pl, u, v, w, { kind: 'directional', dir }, { onFacet: true }).shadowed;
        if (margin < 0.01) continue; // grazing a corner within sampling error
        decided++;
        if (got === worst > 0.003) agree++;
      }
      expect(decided).toBeGreaterThan(170);
      expect(agree).toBe(decided);
    }, 60_000);
  }
});

describe('cell planes from a mesh', () => {
  it('recovers the mock plate facets exactly', () => {
    const p = { ...DEFAULT_PARAMS, widthMm: 60, lightElevationDeg: 8, maxTiltDeg: 14, lightAzimuthDeg: 200 };
    const m = buildMockSquama(p);
    const layout: CellLayout = { kind: 'square', cellMm: p.cellMm, widthMm: m.nx * p.cellMm, heightMm: m.ny * p.cellMm };
    const pl = buildCellPlanes(layout, m.positions, m.indices, m.indexCount);
    expect(pl.cols).toBe(m.nx);
    expect(pl.rows).toBe(m.ny);
    expect(pl.validCells).toBe(m.nx * m.ny);
    // Every facet corner (the first 4 vertices of each cell) lies on its cell plane.
    for (let k = 0; k < m.nx * m.ny; k++) {
      const col = k % m.nx;
      const row = Math.floor(k / m.nx);
      for (let c = 0; c < 4; c++) {
        const v = (k * 4 + c) * 3;
        expect(planeAt(pl, col, row, m.positions[v], m.positions[v + 1])).toBeCloseTo(m.positions[v + 2], 4);
      }
    }
    expect(pl.zMax).toBeCloseTo(m.bboxMax[2], 5);
  });

  it('uses payload face kinds when given and never rises above any facet vertex', () => {
    // Two triangles of one cell, the second corner raised by τ (§5.1 clustering).
    const positions = new Float32Array([0, 0, 1, 6, 0, 1, 6, 6, 1.05, 0, 6, 1, 0, 0, 0, 6, 0, 0]);
    const indices = new Uint32Array([0, 1, 2, 0, 2, 3, 4, 1, 0]);
    const layout: CellLayout = { kind: 'square', cellMm: 6, widthMm: 6, heightMm: 6 };
    const kinds = new Uint16Array([0, 0, 1]);
    const pl = buildCellPlanes(layout, positions, indices, 9, kinds);
    expect(pl.validCells).toBe(1);
    for (const [u, v, w] of [[0, 0, 1], [6, 0, 1], [6, 6, 1.05], [0, 6, 1]]) {
      expect(planeAt(pl, 0, 0, u, v)).toBeLessThanOrEqual(w + 1e-6);
    }
  });
});

describe('shading', () => {
  it('Lambert with ambient: shadowed facets keep only room light', () => {
    expect(shadeRadiance(0.7, 0.5, true, 2, 0.1)).toBeCloseTo((0.7 / Math.PI) * 1.1, 12);
    expect(shadeRadiance(0.7, 0.5, false, 2, 0.1)).toBeCloseTo((0.7 / Math.PI) * 0.1, 12);
    expect(shadeRadiance(0.7, -0.3, true, 2, 0)).toBe(0);
  });
});
