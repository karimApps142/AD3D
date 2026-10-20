// Picks the engine once at startup: the Rust core inside Tauri, the JS mock in a plain browser.

import type { Engine } from './types';

export type { Engine, MeshPayload, SquamaParams, SquamaParamsDto } from './types';

/** Tauri 2 injects `window.__TAURI_INTERNALS__` before any page script runs. */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

let enginePromise: Promise<Engine> | null = null;

export function getEngine(): Promise<Engine> {
  enginePromise ??= isTauri()
    ? import('./tauriEngine').then((m) => m.createTauriEngine())
    : import('./mockEngine').then((m) => m.createMockEngine());
  return enginePromise;
}
