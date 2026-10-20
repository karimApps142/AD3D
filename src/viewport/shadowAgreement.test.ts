// The viewport's analytic cast-shadow tracer (heightfield.ts traceShadow, ported 1:1 to the
// preview shader) and the core's forward model (ad3d_core::sim, exact horizon sweep: what the
// match score, the simulated appearance and the shadow optimiser see) must agree on how much of
// every cell is in a cast shadow. Fixture: the CR-6 Mona Lisa plate (160 mm, LuBan defaults) at
// final quality, optimiser off and on, written by `e2e_m1_dump_shadow_fixture`
// (src-tauri/src/e2e_m1.rs).
import { describe, expect, it } from 'vitest';
import { appearanceFromCore } from '../engine/squamaChecks';
import fixtureText from '../../tests/fixtures/mona-lisa-160-shadow.json?raw';
import { type CellLayout, planeAt, planesFromFunction, shadeRadiance, traceShadow } from './heightfield';

interface Case {
  tag: string;
  nx: number;
  ny: number;
  cellMm: number;
  wMm: number;
  hMm: number;
  lightAzimuthDeg: number;
  lightElevationDeg: number;
  planes: [number, number, number][];
  litFraction: number[];
  /** The core's simulated appearance per facet (what the Appearance view gets). */
  sim: number[];
}

const fixture = JSON.parse(fixtureText) as {
  cases: Case[];
};

/** Lit share and mean shader radiance (preview ambient `ambient`) of each cell, from an n × n
 *  grid of samples traced like the shader does. */
function viewportCells(c: Case, n: number, ambient = 0.02): { lit: number[]; radiance: number[] } {
  const layout: CellLayout = { kind: 'square', cellMm: c.cellMm, widthMm: c.wMm, heightMm: c.hMm };
  const pl = planesFromFunction(layout, (col, row) => {
    const p = c.planes[row * c.nx + col];
    return p ? { h: p[0], gu: p[1], gv: p[2] } : null;
  });
  const az = (c.lightAzimuthDeg * Math.PI) / 180;
  const el = (c.lightElevationDeg * Math.PI) / 180;
  const dir = [Math.cos(el) * Math.cos(az), Math.cos(el) * Math.sin(az), Math.sin(el)] as const;
  const lit: number[] = [];
  const radiance: number[] = [];
  for (let row = 0; row < c.ny; row++) {
    for (let col = 0; col < c.nx; col++) {
      const [, gu, gv] = c.planes[row * c.nx + col];
      const nl = Math.hypot(gu, gv, 1);
      const nDotL = (-gu * dir[0] - gv * dir[1] + dir[2]) / nl;
      let litN = 0;
      let rad = 0;
      for (let j = 0; j < n; j++) {
        for (let i = 0; i < n; i++) {
          const u = (col + (i + 0.5) / n) * c.cellMm;
          const v = (row + (j + 0.5) / n) * c.cellMm;
          const w = planeAt(pl, col, row, u, v);
          const visible = !traceShadow(pl, u, v, w, { kind: 'directional', dir }, { onFacet: true }).shadowed;
          if (visible) litN++;
          rad += shadeRadiance(Math.PI, nDotL, visible, 1, ambient);
        }
      }
      lit.push(litN / (n * n));
      radiance.push(rad / (n * n));
    }
  }
  return { lit, radiance };
}

function pearson(a: ArrayLike<number>, b: ArrayLike<number>): number {
  const n = a.length;
  let ma = 0;
  let mb = 0;
  for (let k = 0; k < n; k++) {
    ma += a[k] / n;
    mb += b[k] / n;
  }
  let sab = 0;
  let saa = 0;
  let sbb = 0;
  for (let k = 0; k < n; k++) {
    sab += (a[k] - ma) * (b[k] - mb);
    saa += (a[k] - ma) ** 2;
    sbb += (b[k] - mb) ** 2;
  }
  return sab / Math.sqrt(saa * sbb);
}

describe('viewport cast shadows agree with the core forward model', () => {
  for (const c of fixture.cases) {
    const n = 24;
    const vp = viewportCells(c, n);
    it(`Mona Lisa 160 mm (${c.tag}): lit fraction per cell`, () => {
      let worst = 0;
      let sum = 0;
      for (let k = 0; k < vp.lit.length; k++) {
        const d = Math.abs(vp.lit[k] - c.litFraction[k]);
        worst = Math.max(worst, d);
        sum += d;
      }
      // Sampling an n × n grid quantises the viewport's fraction to 1/n per column of samples.
      expect(sum / vp.lit.length).toBeLessThan(0.01);
      expect(worst).toBeLessThan(1.5 / n);
    });
    it(`Mona Lisa 160 mm (${c.tag}): the Appearance view matches the shader's per-facet brightness`, () => {
      const shown = appearanceFromCore(c.sim, 0.02).values;
      expect(pearson(shown, vp.radiance)).toBeGreaterThanOrEqual(0.98);
      // Same normalisation: brightest facet = 1 in the view, and the ratio to the shader is constant.
      const top = Math.max(...vp.radiance);
      let worst = 0;
      for (let k = 0; k < shown.length; k++) worst = Math.max(worst, Math.abs(shown[k] - vp.radiance[k] / top));
      expect(worst).toBeLessThan(0.05);
    });
  }
});
