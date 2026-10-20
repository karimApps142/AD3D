// Pure JS mesh generators for the browser mock engine. They exist so the UI, the binary payload
// path and the spike harness can run without Rust. The plate is a plausible tilted-facet Squama
// plate (facets, step walls split where neighbouring facets cross, boundary walls, back face) with
// consistent outward winding, but it is NOT the real mesher: it keeps T-junctions and saddle
// pinches, so it is not watertight. The real one lives in crates/ad3d-core.

import { brightnessToTilt, degToRad, pivotHeight, snapGrid, SYNTHETIC_ASPECT } from './squamaMath';
import { cellBrightness } from './syntheticImage';
import type { SquamaParams, Vec3 } from './types';

export interface RawMesh {
  /** xyz per vertex; may be longer than 3 * vertexCount. */
  positions: Float32Array<ArrayBuffer>;
  /** Triangle list; may be longer than indexCount. */
  indices: Uint32Array<ArrayBuffer>;
  vertexCount: number;
  indexCount: number;
  bboxMin: Vec3;
  bboxMax: Vec3;
}

export interface MockSquamaMesh extends RawMesh {
  nx: number;
  ny: number;
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

const EPS = 1e-9;

export function buildMockSquama(p: SquamaParams): MockSquamaMesh {
  const s = clamp(p.cellMm, 0.1, 1000);
  const { nx, ny } = snapGrid(p.widthMm, s, SYNTHETIC_ASPECT);
  const W = nx * s;
  const H = ny * s;
  const alpha = degToRad(clamp(p.lightElevationDeg, 0, 89));
  const maxTilt = degToRad(clamp(p.maxTiltDeg, 0.01, 60));
  const phi = degToRad(p.lightAzimuthDeg);
  const ax = Math.cos(phi);
  const ay = Math.sin(phi);
  const pivot = pivotHeight(Math.max(0, p.baseMm), s, phi, maxTilt);
  const g = cellBrightness(nx, ny);

  const nCells = nx * ny;
  const nInterior = (nx - 1) * ny + nx * (ny - 1);
  const ring = 2 * (nx + ny);
  const maxVerts = 4 * nCells + nInterior + ring + 1;
  const maxTris = 2 * nCells + 2 * nInterior + 3 * ring;
  const pos = new Float32Array(maxVerts * 3);
  const idx = new Uint32Array(maxTris * 3);
  let nv = 0;
  let ni = 0;

  const h = new Float64Array(nCells * 4); // corner heights: BL, BR, TR, TL
  const half = s / 2;
  const cornerDx = [-half, half, half, -half];
  const cornerDy = [-half, -half, half, half];

  // Facet corners: 4 own vertices per cell, so the step walls can join them.
  for (let j = 0; j < ny; j++) {
    for (let i = 0; i < nx; i++) {
      const k = j * nx + i;
      const tanT = Math.tan(brightnessToTilt(g[k], alpha, maxTilt));
      const cx = (i + 0.5) * s;
      const cy = (j + 0.5) * s;
      for (let c = 0; c < 4; c++) {
        // w = pivot + ∇w · d with ∇w = −tan θ · a (design §3–§4).
        const w = pivot - tanT * (ax * cornerDx[c] + ay * cornerDy[c]);
        h[k * 4 + c] = w;
        pos[nv * 3] = cx + cornerDx[c];
        pos[nv * 3 + 1] = cy + cornerDy[c];
        pos[nv * 3 + 2] = w;
        nv++;
      }
      const v = k * 4;
      idx[ni++] = v; idx[ni++] = v + 1; idx[ni++] = v + 2;
      idx[ni++] = v; idx[ni++] = v + 2; idx[ni++] = v + 3;
    }
  }

  const tri = (a: number, b: number, c: number) => {
    idx[ni++] = a;
    idx[ni++] = b;
    idx[ni++] = c;
  };
  const addVertex = (x: number, y: number, z: number) => {
    pos[nv * 3] = x;
    pos[nv * 3 + 1] = y;
    pos[nv * 3 + 2] = z;
    return nv++;
  };

  // Step wall between facet A and facet B along a shared edge p0 → p1. A0/A1 and B0/B1 are the
  // facets' own vertices at p0/p1. With the vertex orders below the wall normal follows the sign
  // of the height difference, so it always faces the lower facet; where the facet edges cross the
  // wall is split at the crossing X into two triangles.
  const wall = (
    A0: number, A1: number, B0: number, B1: number,
    hA0: number, hA1: number, hB0: number, hB1: number,
    x0: number, y0: number, x1: number, y1: number,
    vertical: boolean,
  ) => {
    let d0 = hA0 - hB0;
    let d1 = hA1 - hB1;
    if (Math.abs(d0) < EPS) d0 = 0;
    if (Math.abs(d1) < EPS) d1 = 0;
    if (d0 === 0 && d1 === 0) return;
    if (d0 * d1 < 0) {
      const t = d0 / (d0 - d1);
      const X = addVertex(x0 + t * (x1 - x0), y0 + t * (y1 - y0), hA0 + t * (hA1 - hA0));
      if (vertical) {
        tri(B0, X, A0);
        tri(X, B1, A1);
      } else {
        tri(B0, A0, X);
        tri(X, A1, B1);
      }
      return;
    }
    if (vertical) {
      if (d1 !== 0) tri(B0, B1, A1);
      if (d0 !== 0) tri(B0, A1, A0);
    } else {
      if (d0 !== 0) tri(B0, A0, A1);
      if (d1 !== 0) tri(B0, A1, B1);
    }
  };

  // Vertical edges: A = (i, j) on the left, B = (i + 1, j) on the right.
  for (let j = 0; j < ny; j++) {
    for (let i = 0; i < nx - 1; i++) {
      const a = (j * nx + i) * 4;
      const b = (j * nx + i + 1) * 4;
      const x = (i + 1) * s;
      wall(a + 1, a + 2, b, b + 3, h[a + 1], h[a + 2], h[b], h[b + 3], x, j * s, x, (j + 1) * s, true);
    }
  }
  // Horizontal edges: A = (i, j) below, B = (i, j + 1) above.
  for (let j = 0; j < ny - 1; j++) {
    for (let i = 0; i < nx; i++) {
      const a = (j * nx + i) * 4;
      const b = ((j + 1) * nx + i) * 4;
      const y = (j + 1) * s;
      wall(a + 3, a + 2, b, b + 1, h[a + 3], h[a + 2], h[b], h[b + 1], i * s, y, (i + 1) * s, y, false);
    }
  }

  // Base ring at w = 0, counter-clockwise seen from +w: bottom, right, top, left.
  const ringBase = nv;
  for (let i = 0; i < nx; i++) addVertex(i * s, 0, 0);
  for (let j = 0; j < ny; j++) addVertex(W, j * s, 0);
  for (let i = nx; i > 0; i--) addVertex(i * s, H, 0);
  for (let j = ny; j > 0; j--) addVertex(0, j * s, 0);
  const rb = (i: number) => ringBase + i;
  const rr = (j: number) => ringBase + nx + j;
  const rt = (i: number) => ringBase + nx + ny + (nx - i);
  const rl = (j: number) => ringBase + ((2 * nx + ny + (ny - j)) % ring);

  for (let i = 0; i < nx; i++) {
    const bot = i * 4; // cell (i, 0)
    tri(rb(i), bot + 1, bot);
    tri(rb(i), rb(i + 1), bot + 1);
    const top = ((ny - 1) * nx + i) * 4; // cell (i, ny - 1)
    tri(rt(i), top + 3, top + 2);
    tri(rt(i), top + 2, rt(i + 1));
  }
  for (let j = 0; j < ny; j++) {
    const left = (j * nx) * 4; // cell (0, j)
    tri(rl(j), left, left + 3);
    tri(rl(j), left + 3, rl(j + 1));
    const right = (j * nx + nx - 1) * 4; // cell (nx - 1, j)
    tri(rr(j), right + 2, right + 1);
    tri(rr(j), rr(j + 1), right + 2);
  }

  // Back face: fan from the centre, wound so its normal points to −w.
  const centre = addVertex(W / 2, H / 2, 0);
  for (let k = 0; k < ring; k++) tri(centre, ringBase + ((k + 1) % ring), ringBase + k);

  let zMax = 0;
  for (let k = 0; k < h.length; k++) if (h[k] > zMax) zMax = h[k];
  return {
    positions: pos,
    indices: idx,
    vertexCount: nv,
    indexCount: ni,
    bboxMin: [0, 0, 0],
    bboxMax: [W, H, zMax],
    nx,
    ny,
  };
}

/**
 * A wavy height-field sheet with exactly `nTriangles` triangles, wound counter-clockwise seen from
 * +z, covering widthMm × heightMm. Used for the 1M-triangle transfer benchmark (C4).
 */
export function buildSyntheticMesh(
  nTriangles: number,
  widthMm = 300,
  heightMm = 378,
  baseMm = 3,
): RawMesh {
  const n = Math.max(0, Math.floor(nTriangles));
  if (n > 50_000_000) throw new RangeError(`nTriangles ${n} is above the 50M limit`);
  if (n === 0) {
    return {
      positions: new Float32Array(0),
      indices: new Uint32Array(0),
      vertexCount: 0,
      indexCount: 0,
      bboxMin: [0, 0, 0],
      bboxMax: [0, 0, 0],
    };
  }
  const quads = Math.ceil(n / 2); // the last quad holds one triangle when n is odd
  const cols = Math.max(1, Math.ceil(Math.sqrt(quads * (widthMm / heightMm))));
  const rows = Math.ceil(quads / cols);
  const dx = widthMm / cols;
  const dy = heightMm / rows;
  const vx = cols + 1;
  const vertexCount = vx * (rows + 1);
  const positions = new Float32Array(vertexCount * 3);
  let zMin = Infinity;
  let zMax = -Infinity;
  for (let j = 0, o = 0; j <= rows; j++) {
    const y = j * dy;
    for (let i = 0; i <= cols; i++, o += 3) {
      const x = i * dx;
      const z = baseMm + 1.2 * Math.sin(x / 17) * Math.cos(y / 23) + 0.5 * Math.sin((x + y) / 7) + 1.7;
      positions[o] = x;
      positions[o + 1] = y;
      positions[o + 2] = z;
      if (z < zMin) zMin = z;
      if (z > zMax) zMax = z;
    }
  }
  const indices = new Uint32Array(n * 3);
  let t = 0;
  for (let q = 0; q < quads; q++) {
    const i = q % cols;
    const j = (q - i) / cols;
    const v00 = j * vx + i;
    const v10 = v00 + 1;
    const v01 = v00 + vx;
    const v11 = v01 + 1;
    indices[t++] = v00; indices[t++] = v10; indices[t++] = v11;
    if (t < indices.length) {
      indices[t++] = v00; indices[t++] = v11; indices[t++] = v01;
    }
  }
  return {
    positions,
    indices,
    vertexCount,
    indexCount: n * 3,
    bboxMin: [0, 0, zMin],
    bboxMax: [widthMm, heightMm, zMax],
  };
}
