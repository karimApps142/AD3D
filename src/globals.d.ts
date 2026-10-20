// Globals the app reads or publishes on `window`.

declare global {
  interface Window {
    /** Injected by Tauri 2; its presence selects the native engine. */
    __TAURI_INTERNALS__?: unknown;
    /** The finished spike report (both engines), for headless runs to pick up. */
    __SPIKE_REPORT__?: unknown;
    /** Mock engine only: artifacts saved by the spike harness, by name. */
    __SPIKE_ARTIFACTS__?: Record<string, Uint8Array>;
    /** Live spike progress, for headless runs. */
    __SPIKE_STATUS__?: string;
  }
}

export {};
