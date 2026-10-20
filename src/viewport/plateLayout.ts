// The cell layout of the displayed plate, recovered from the params that produced it and the
// mesh bounds (the plate spans [0, W] × [0, H] in plate space, §1).

import type { SquamaParams } from '../engine/types';
import type { DisplayMesh } from '../state/store';
import type { CellLayout } from './heightfield';

/** §12.1 `layout` (square default, or hex with an orientation). */
export function paramsLayoutKind(params: SquamaParams): { kind: 'square' } | { kind: 'hex'; orientation: 'pointy' | 'flat' } {
  const l = params.layout;
  if (l && l.kind === 'hex') return { kind: 'hex', orientation: l.orientation === 'flat' ? 'flat' : 'pointy' };
  return { kind: 'square' };
}

export function layoutFromDisplay(display: DisplayMesh | null): CellLayout | null {
  if (!display || display.kind !== 'squama' || !display.params) return null;
  const p = display.payload;
  if (p.vertexCount === 0) return null;
  const cellMm = display.params.cellMm;
  if (!(cellMm > 0)) return null;
  const widthMm = p.bboxMax[0];
  const heightMm = p.bboxMax[1];
  if (!(widthMm > 0 && heightMm > 0) || Math.abs(p.bboxMin[0]) > 1e-3 || Math.abs(p.bboxMin[1]) > 1e-3) return null;
  const k = paramsLayoutKind(display.params);
  return k.kind === 'hex'
    ? { kind: 'hex', orientation: k.orientation, cellMm, widthMm, heightMm }
    : { kind: 'square', cellMm, widthMm, heightMm };
}
