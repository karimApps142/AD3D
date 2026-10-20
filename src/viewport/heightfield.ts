// Analytic heightfield of a Squama plate for the preview shader: one plane per cell (square or hex
// layout, docs/design/squama-mesher.md §4, §6.2), the cell lookup, the distance to a cell's exit
// along a ray, and the cast-shadow ray march. The GLSL in heightfieldShader.ts is a line-by-line
// port of lookupCell / cellExit / planeAt / traceShadow below; keep the two in step (the unit
// tests exercise this TS version).
//
// Plate space: u right, v up, w out of the front face (three.js x, y, z). Millimetres.

import { FACE_KIND, faceKindOf } from './faceKinds';

export type CellLayout =
  | { kind: 'square'; cellMm: number; widthMm: number; heightMm: number }
  | { kind: 'hex'; orientation: 'pointy' | 'flat'; cellMm: number; widthMm: number; heightMm: number };

/** Shader/texture code of a layout: 0 square, 1 pointy-top hex, 2 flat-top hex. */
export function layoutCode(l: CellLayout): 0 | 1 | 2 {
  if (l.kind === 'square') return 0;
  return l.orientation === 'flat' ? 2 : 1;
}

const SQRT3_2 = Math.sqrt(3) / 2;

/** Texture grid size (columns, rows) that holds every cell touching the plate. */
export function gridSize(l: CellLayout): { cols: number; rows: number } {
  const c = Math.max(l.cellMm, 1e-6);
  if (l.kind === 'square') {
    return { cols: Math.max(1, Math.round(l.widthMm / c)), rows: Math.max(1, Math.round(l.heightMm / c)) };
  }
  // Local frame: x along the rows, y across them (flat-top swaps u and v, §6.2).
  const [lx, ly] = l.orientation === 'flat' ? [l.heightMm, l.widthMm] : [l.widthMm, l.heightMm];
  const p = SQRT3_2 * c;
  // Column = i + 1 so the half-cell at x < cell/2 on odd rows never needs i = -1.
  return { cols: Math.floor(lx / c) + 3, rows: Math.floor(ly / p) + 2 };
}

/** Centre (pivot) of cell (col, row) in plate space. */
export function cellCenter(l: CellLayout, col: number, row: number): [number, number] {
  const c = l.cellMm;
  if (l.kind === 'square') return [(col + 0.5) * c, (row + 0.5) * c];
  const i = col - 1;
  const x = (i + 0.5 * (row & 1)) * c;
  const y = row * SQRT3_2 * c;
  return l.orientation === 'flat' ? [y, x] : [x, y];
}

/**
 * Cell (col, row) containing plate point (u, v), clamped to the texture grid. For hex cells this
 * is the nearest centre (the Voronoi cell), found among the two rows that bracket v.
 */
export function lookupCell(l: CellLayout, u: number, v: number, grid = gridSize(l)): [number, number] {
  const c = l.cellMm;
  let col: number;
  let row: number;
  if (l.kind === 'square') {
    col = Math.floor(u / c);
    row = Math.floor(v / c);
  } else {
    const [x, y] = l.orientation === 'flat' ? [v, u] : [u, v];
    const p = SQRT3_2 * c;
    const j0 = Math.floor(y / p);
    let best = Infinity;
    col = 0;
    row = 0;
    for (let k = 0; k < 2; k++) {
      const j = j0 + k;
      const off = 0.5 * (j & 1);
      const i = Math.round(x / c - off);
      const dx = x - (i + off) * c;
      const dy = y - j * p;
      const d2 = dx * dx + dy * dy;
      if (d2 < best) {
        best = d2;
        col = i + 1;
        row = j;
      }
    }
  }
  return [Math.min(grid.cols - 1, Math.max(0, col)), Math.min(grid.rows - 1, Math.max(0, row))];
}

/**
 * Horizontal distance from (u, v) along the unit 2D direction (du, dv) to where the ray leaves
 * cell (col, row). The cell is the intersection of half-planes n·(p − centre) ≤ apothem: 4 for a
 * square (apothem cell/2), 6 for a hex (apothem cell/2, the width across flats).
 */
export function cellExit(l: CellLayout, col: number, row: number, u: number, v: number, du: number, dv: number): number {
  const [cu, cv] = cellCenter(l, col, row);
  const ru = u - cu;
  const rv = v - cv;
  const a = 0.5 * l.cellMm;
  let t = Infinity;
  const n = l.kind === 'square' ? 4 : 6;
  // Square normals at 0°, 90°…; pointy hex at 0°, 60°…; flat hex at 30°, 90°… (u, v swapped).
  const step = (2 * Math.PI) / n;
  const phase = l.kind === 'hex' && l.orientation === 'flat' ? Math.PI / 6 : 0;
  for (let k = 0; k < n; k++) {
    const nu = Math.cos(phase + k * step);
    const nv = Math.sin(phase + k * step);
    const dn = du * nu + dv * nv;
    if (dn > 1e-9) t = Math.min(t, (a - (ru * nu + rv * nv)) / dn);
  }
  return Math.max(0, t);
}

/** Per-cell planes, packed for a float RGBA texture: (w at centre, ∂w/∂u, ∂w/∂v, valid). */
export interface CellPlanes {
  layout: CellLayout;
  cols: number;
  rows: number;
  /** cols × rows × 4 floats, row-major (row = texture y). */
  data: Float32Array<ArrayBuffer>;
  /** Highest point of the plate's front (facet) surface. */
  zMax: number;
  /** Lowest facet vertex (for the relief colour map). */
  zMinFacet: number;
  /** Number of cells that received at least one facet triangle. */
  validCells: number;
}

/** Height of cell (col, row)'s plane at (u, v); −Infinity when the cell has no facet. */
export function planeAt(pl: CellPlanes, col: number, row: number, u: number, v: number): number {
  const o = (row * pl.cols + col) * 4;
  if (pl.data[o + 3] < 0.5) return -Infinity;
  const [cu, cv] = cellCenter(pl.layout, col, row);
  return pl.data[o] + pl.data[o + 1] * (u - cu) + pl.data[o + 2] * (v - cv);
}

/** Planes from analytic facets (tests, and a future facet payload from the core). */
export function planesFromFunction(
  layout: CellLayout,
  facet: (col: number, row: number) => { h: number; gu: number; gv: number } | null,
): CellPlanes {
  const { cols, rows } = gridSize(layout);
  const data = new Float32Array(cols * rows * 4);
  let zMax = -Infinity;
  let zMin = Infinity;
  let valid = 0;
  const half = layout.kind === 'square' ? layout.cellMm * Math.SQRT1_2 : layout.cellMm / Math.sqrt(3);
  for (let row = 0; row < rows; row++) {
    for (let col = 0; col < cols; col++) {
      const f = facet(col, row);
      if (!f) continue;
      const o = (row * cols + col) * 4;
      data[o] = f.h;
      data[o + 1] = f.gu;
      data[o + 2] = f.gv;
      data[o + 3] = 1;
      const spread = Math.hypot(f.gu, f.gv) * half;
      zMax = Math.max(zMax, f.h + spread);
      zMin = Math.min(zMin, f.h - spread);
      valid++;
    }
  }
  return { layout, cols, rows, data, zMax: valid ? zMax : 0, zMinFacet: valid ? zMin : 0, validCells: valid };
}

/**
 * Fits one plane per cell to the mesh's facet triangles. The slope is the area-weighted mean
 * facet normal; the offset is the lowest one that keeps every facet vertex of the cell on or above
 * the plane. §5.1 clustering only ever raises corners (by ≤ τ), so for a real plate this recovers
 * the design plane, and the rendered facets never dip below their own plane (no shadow acne).
 *
 * `kinds` (§12.2 per-triangle u16, low byte = FaceKind) is optional; without it facets are the
 * triangles whose normal points out of the front (n_w > 0.5).
 */
export function buildCellPlanes(
  layout: CellLayout,
  positions: ArrayLike<number>,
  indices: ArrayLike<number>,
  indexCount: number,
  kinds: ArrayLike<number> | null = null,
): CellPlanes {
  const grid = gridSize(layout);
  const { cols, rows } = grid;
  const n = cols * rows;
  const acc = new Float64Array(n * 3); // area-weighted normal
  const triOf = new Int32Array(indexCount / 3);
  let zMax = -Infinity;
  let zMin = Infinity;

  for (let t = 0, tri = 0; t + 2 < indexCount; t += 3, tri++) {
    triOf[tri] = -1;
    const a = indices[t] * 3, b = indices[t + 1] * 3, c = indices[t + 2] * 3;
    const ax = positions[a], ay = positions[a + 1], az = positions[a + 2];
    const e1x = positions[b] - ax, e1y = positions[b + 1] - ay, e1z = positions[b + 2] - az;
    const e2x = positions[c] - ax, e2y = positions[c + 1] - ay, e2z = positions[c + 2] - az;
    const nx = e1y * e2z - e1z * e2y;
    const ny = e1z * e2x - e1x * e2z;
    const nz = e1x * e2y - e1y * e2x;
    const len = Math.hypot(nx, ny, nz);
    if (len === 0) continue;
    const kind = faceKindOf(kinds, tri, nz / len);
    if (kind !== FACE_KIND.facet || nz <= 0) continue;
    const cu = (ax + positions[b] + positions[c]) / 3;
    const cv = (ay + positions[b + 1] + positions[c + 1]) / 3;
    const [col, row] = lookupCell(layout, cu, cv, grid);
    const k = row * cols + col;
    triOf[tri] = k;
    acc[k * 3] += nx; // |n| = 2·area, so this is area weighting
    acc[k * 3 + 1] += ny;
    acc[k * 3 + 2] += nz;
  }

  const data = new Float32Array(n * 4);
  const offset = new Float64Array(n).fill(Infinity);
  for (let k = 0; k < n; k++) {
    const nz = acc[k * 3 + 2];
    if (nz <= 0) continue;
    data[k * 4 + 1] = -acc[k * 3] / nz;
    data[k * 4 + 2] = -acc[k * 3 + 1] / nz;
  }
  // Lowest offset over every vertex of the cell's facet triangles.
  for (let t = 0, tri = 0; t + 2 < indexCount; t += 3, tri++) {
    const k = triOf[tri];
    if (k < 0) continue;
    const col = k % cols;
    const row = (k - col) / cols;
    const [pu, pv] = cellCenter(layout, col, row);
    const gu = data[k * 4 + 1];
    const gv = data[k * 4 + 2];
    for (let s = 0; s < 3; s++) {
      const p = indices[t + s] * 3;
      const z = positions[p + 2];
      const h = z - gu * (positions[p] - pu) - gv * (positions[p + 1] - pv);
      if (h < offset[k]) offset[k] = h;
      if (z > zMax) zMax = z;
      if (z < zMin) zMin = z;
    }
  }
  let valid = 0;
  for (let k = 0; k < n; k++) {
    if (offset[k] === Infinity) continue;
    data[k * 4] = offset[k];
    data[k * 4 + 3] = 1;
    valid++;
  }
  return { layout, cols, rows, data, zMax: valid ? zMax : 0, zMinFacet: valid ? zMin : 0, validCells: valid };
}

export type TraceLight = { kind: 'directional'; dir: readonly [number, number, number] } | { kind: 'point'; pos: readonly [number, number, number] };

export interface TraceOptions {
  /** A surface must rise this far above the ray to count as an occluder (mm). */
  bias?: number;
  /** Cells visited before giving up (counted as lit). */
  maxSteps?: number;
  /** True for a fragment on a facet: its own cell cannot shadow it and is skipped. */
  onFacet?: boolean;
}

export const TRACE_BIAS = 0.003;
export const TRACE_MAX_STEPS = 192;
/** Step past a cell boundary so the next lookup lands in the next cell. */
export const TRACE_EPS = 1e-3;
/** Start offset for wall fragments, so the first lookup is on the light side of the wall. */
export const TRACE_WALL_START = 0.02;

export interface TraceResult {
  shadowed: boolean;
  steps: number;
}

/**
 * Is plate point P = (pu, pv, pw) in a cast shadow? Walks the cells the ray toward the light
 * crosses. Inside one cell both the ray and the cell's plane are linear in the distance, so the
 * surface rises above the ray in that cell iff it does at the entry or the exit: the test is exact
 * for the analytic heightfield, with no shadow map and no acne.
 */
export function traceShadow(pl: CellPlanes, pu: number, pv: number, pw: number, light: TraceLight, opt: TraceOptions = {}): TraceResult {
  const bias = opt.bias ?? TRACE_BIAS;
  const maxSteps = opt.maxSteps ?? TRACE_MAX_STEPS;
  const l = pl.layout;
  const grid = { cols: pl.cols, rows: pl.rows };
  let dx: number, dy: number, dz: number;
  let tMax = Infinity;
  if (light.kind === 'directional') {
    [dx, dy, dz] = light.dir;
  } else {
    dx = light.pos[0] - pu;
    dy = light.pos[1] - pv;
    dz = light.pos[2] - pw;
  }
  const hl = Math.hypot(dx, dy);
  if (hl < 1e-6) return { shadowed: false, steps: 0 };
  if (light.kind === 'point') tMax = hl;
  const du = dx / hl;
  const dv = dy / hl;
  const slope = dz / hl;

  let t = 0;
  if (opt.onFacet) {
    const [c0, r0] = lookupCell(l, pu, pv, grid);
    t = cellExit(l, c0, r0, pu, pv, du, dv) + TRACE_EPS;
  } else {
    t = TRACE_WALL_START;
  }
  for (let step = 0; step < maxSteps; step++) {
    if (t >= tMax) return { shadowed: false, steps: step };
    const u = pu + du * t;
    const v = pv + dv * t;
    if (u < 0 || v < 0 || u > l.widthMm || v > l.heightMm) return { shadowed: false, steps: step };
    const zr = pw + slope * t;
    if (slope >= 0 && zr > pl.zMax + bias) return { shadowed: false, steps: step };
    const [col, row] = lookupCell(l, u, v, grid);
    const te = Math.min(t + cellExit(l, col, row, u, v, du, dv), tMax);
    const s0 = planeAt(pl, col, row, u, v) - zr;
    const s1 = planeAt(pl, col, row, pu + du * te, pv + dv * te) - (pw + slope * te);
    if (Math.max(s0, s1) > bias) return { shadowed: true, steps: step + 1 };
    t = te + TRACE_EPS;
  }
  return { shadowed: false, steps: maxSteps };
}

/** Lambert term the shader uses: radiance = albedo/π · (I · max(0, N·L) · vis + A). */
export function shadeRadiance(albedo: number, nDotL: number, visible: boolean, direct: number, ambient: number): number {
  return (albedo / Math.PI) * (direct * Math.max(0, nDotL) * (visible ? 1 : 0) + ambient);
}
