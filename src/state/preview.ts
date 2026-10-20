// Live preview: every parameter change goes through one latest-wins loop to the engine, and each
// accepted mesh is put on screen. One controller per app.

import type { Engine, MeshPayload, SquamaParams } from '../engine/types';
import { LatestWinsLoop } from './latestWins';
import { perf } from './perf';
import { useAppStore } from './store';

export interface PreviewController {
  readonly loop: LatestWinsLoop<SquamaParams, MeshPayload>;
  /** Regenerates the plate from the current params. */
  refresh(): void;
  dispose(): void;
}

let controller: PreviewController | null = null;

export function getPreviewController(): PreviewController | null {
  return controller;
}

/** Copies the per-mesh numbers into the stats overlay's counters. */
export function notePayload(payload: MeshPayload): void {
  const t = payload.timing;
  if (t) {
    perf.last.ipcMs = t.receivedAt - t.requestedAt;
    perf.last.parseMs = t.parsedAt - t.receivedAt;
  }
  perf.last.genMs = payload.genMs;
  perf.last.encodeMs = payload.encodeMs;
  perf.last.triangles = payload.triangleCount;
  perf.last.vertices = payload.vertexCount;
  perf.last.bytes = payload.byteLength;
  perf.last.jobId = payload.jobId;
}

export function startPreview(engine: Engine): PreviewController {
  if (controller) return controller;

  const loop = new LatestWinsLoop<SquamaParams, MeshPayload>({
    // A request no slider tagged as a drag is a final plate (launch, open, undo, typed values).
    run: (params, jobId) => engine.generateSquama({ ...params, quality: params.quality ?? 'final', jobId }),
    getJobId: (payload) => payload.jobId,
    onResult: (payload, info) => {
      notePayload(payload);
      const store = useAppStore.getState();
      if (store.previewError) useAppStore.setState({ previewError: null });
      store.showMesh({ payload, kind: 'squama', params: info.params, inputAt: info.requestedAt });
    },
    onError: (error) => {
      console.error('[preview] generation failed', error);
      useAppStore.setState({ previewError: error instanceof Error ? error.message : String(error) });
    },
    onBusyChange: (busy) => useAppStore.setState({ previewBusy: busy }),
    // A final plate (optimiser: up to its time budget) must not hold back the next drag tick:
    // abort it in the core and send the drag at once.
    shouldCancel: (inFlight, next) => (inFlight.quality ?? 'final') === 'final' && next.quality === 'preview',
    cancel: engine.cancelGenerate ? () => void engine.cancelGenerate?.() : undefined,
  });

  const unsubscribe = useAppStore.subscribe((state, prev) => {
    if (state.params !== prev.params) loop.request(state.params);
  });
  loop.request(useAppStore.getState().params);

  controller = {
    loop,
    refresh: () => loop.request(useAppStore.getState().params),
    dispose: () => {
      unsubscribe();
      controller = null;
    },
  };
  return controller;
}
