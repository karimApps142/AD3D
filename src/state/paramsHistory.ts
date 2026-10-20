// Undo/redo of generator parameters. Watches the app store: every params change is recorded
// (coalesced, see history.ts) unless it came from undo/redo itself. Undo simply restores the
// parameters; the live preview regenerates from them like after any other change.

import { create } from 'zustand';
import type { SquamaParams } from '../engine/types';
import { History } from './history';
import { useAppStore } from './store';

export interface HistoryStatus {
  canUndo: boolean;
  canRedo: boolean;
  /** Bumped on every history change (for subscribers that need to re-render). */
  version: number;
}

export const useHistoryStatus = create<HistoryStatus>()(() => ({ canUndo: false, canRedo: false, version: 0 }));

/** Shallow equality for flat param objects (nested values compared by JSON). */
export function paramsEqual(a: SquamaParams, b: SquamaParams): boolean {
  if (a === b) return true;
  const ka = Object.keys(a) as (keyof SquamaParams)[];
  const kb = Object.keys(b);
  if (ka.length !== kb.length) return false;
  for (const k of ka) {
    const va = a[k];
    const vb = b[k];
    if (va === vb) continue;
    if (typeof va === 'object' && typeof vb === 'object' && JSON.stringify(va) === JSON.stringify(vb)) continue;
    return false;
  }
  return true;
}

/** The coalescing key of a change: the sorted names of the fields that differ. */
export function changeKey(prev: SquamaParams, next: SquamaParams): string {
  const keys = new Set([...Object.keys(prev), ...Object.keys(next)] as (keyof SquamaParams)[]);
  const changed: string[] = [];
  for (const k of keys) {
    if (!paramsEqual({ [k]: prev[k] } as SquamaParams, { [k]: next[k] } as SquamaParams)) changed.push(k);
  }
  return changed.sort().join(',');
}

let history = new History<SquamaParams>(useAppStore.getState().params, { equals: paramsEqual });
let applying = false;
let unsubscribe: (() => void) | null = null;

function publish(): void {
  useHistoryStatus.setState((s) => ({ canUndo: history.canUndo, canRedo: history.canRedo, version: s.version + 1 }));
}

/** Starts recording. Idempotent; returns a stop function. */
export function startParamsHistory(opts: { limit?: number; coalesceMs?: number; now?: () => number } = {}): () => void {
  if (unsubscribe) return unsubscribe;
  history = new History<SquamaParams>(useAppStore.getState().params, { equals: paramsEqual, ...opts });
  const off = useAppStore.subscribe((state, prev) => {
    if (applying || state.params === prev.params) return;
    history.record(state.params, changeKey(prev.params, state.params));
    publish();
  });
  publish();
  unsubscribe = () => {
    off();
    unsubscribe = null;
  };
  return unsubscribe;
}

function apply(params: SquamaParams | null): boolean {
  if (!params) return false;
  applying = true;
  try {
    useAppStore.setState({ params });
  } finally {
    applying = false;
  }
  publish();
  return true;
}

export const paramsHistory = {
  undo: (): boolean => apply(history.undo()),
  redo: (): boolean => apply(history.redo()),
  /** Forget all steps; the current params become the baseline (new/open project). */
  reset: (): void => {
    history.reset(useAppStore.getState().params);
    publish();
  },
  /** Replace params without recording a step, then reset (opening a project). */
  load: (params: SquamaParams): void => {
    apply(params);
    history.reset(params);
    publish();
  },
  beginGesture: (): void => history.beginGesture(),
  endGesture: (): void => history.endGesture(),
  setLimit: (n: number): void => {
    history.setLimit(n);
    publish();
  },
  get canUndo(): boolean {
    return history.canUndo;
  },
  get canRedo(): boolean {
    return history.canRedo;
  },
  get undoDepth(): number {
    return history.undoDepth;
  },
};
