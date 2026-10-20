// Small binary formats from src-tauri/src/squama_cmds.rs (little-endian):
// - thumbnail: [w u32][h u32] then w·h sRGB grey bytes, top row first;
// - AD3T target grid: [magic u32 "AD3T"][nx u32][ny u32][flags u32] then nx·ny f32 g
//   (row 0 at the bottom of the plate); flags bit 0 = a simulated grid of the same size follows.

import type { GrayImage, TargetGrid } from './types';

export const TARGET_MAGIC = 0x54334441;
export const TARGET_HAS_SIM = 1;
/** Payload flags bit 0 (§12.2): the mesh is final quality. */
export const PAYLOAD_FLAG_FINAL = 1;

function view(buf: ArrayBuffer | ArrayBufferView): DataView {
  return buf instanceof ArrayBuffer ? new DataView(buf) : new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
}

export function parseThumbnail(buf: ArrayBuffer | ArrayBufferView): GrayImage {
  const dv = view(buf);
  if (dv.byteLength < 8) throw new Error('thumbnail too short');
  const width = dv.getUint32(0, true);
  const height = dv.getUint32(4, true);
  if (dv.byteLength !== 8 + width * height) throw new Error(`thumbnail size mismatch (${width}×${height}, ${dv.byteLength} B)`);
  const data = new Uint8Array(dv.buffer, dv.byteOffset + 8, width * height).slice();
  return { width, height, data };
}

export function encodeThumbnail(img: GrayImage): ArrayBuffer {
  const out = new ArrayBuffer(8 + img.data.length);
  const dv = new DataView(out);
  dv.setUint32(0, img.width, true);
  dv.setUint32(4, img.height, true);
  new Uint8Array(out, 8).set(img.data);
  return out;
}

export function parseTarget(buf: ArrayBuffer | ArrayBufferView): TargetGrid {
  const dv = view(buf);
  if (dv.byteLength < 16 || dv.getUint32(0, true) !== TARGET_MAGIC) throw new Error('not an AD3T target grid');
  const nx = dv.getUint32(4, true);
  const ny = dv.getUint32(8, true);
  const flags = dv.getUint32(12, true);
  const n = nx * ny;
  const grids = flags & TARGET_HAS_SIM ? 2 : 1;
  if (dv.byteLength !== 16 + 4 * n * grids) throw new Error(`target size mismatch (${nx}×${ny}, ${dv.byteLength} B)`);
  const read = (offset: number) => {
    const g = new Float32Array(n);
    for (let k = 0; k < n; k++) g[k] = dv.getFloat32(offset + 4 * k, true);
    return g;
  };
  const grid: TargetGrid = { nx, ny, g: read(16) };
  if (grids === 2) grid.sim = read(16 + 4 * n);
  return grid;
}

export function encodeTarget(grid: TargetGrid): ArrayBuffer {
  const n = grid.nx * grid.ny;
  const grids = grid.sim ? 2 : 1;
  const out = new ArrayBuffer(16 + 4 * n * grids);
  const dv = new DataView(out);
  dv.setUint32(0, TARGET_MAGIC, true);
  dv.setUint32(4, grid.nx, true);
  dv.setUint32(8, grid.ny, true);
  dv.setUint32(12, grid.sim ? TARGET_HAS_SIM : 0, true);
  for (let k = 0; k < n; k++) dv.setFloat32(16 + 4 * k, grid.g[k], true);
  if (grid.sim) for (let k = 0; k < n; k++) dv.setFloat32(16 + 4 * (n + k), grid.sim[k], true);
  return out;
}
