// M1 verify round 1: scripted capture of the live preview in the real desktop app (dev only).
//
// Enabled by `VITE_AD3D_E2E_PHOTO=/abs/photo.jpg pnpm tauri dev` plus the one-line hook in
// main.tsx (see docs/m1/E2E.md). It does what ImagePanel's "Open photo…" does after the dialog
// (engine.imageInfo, then the params patch), waits for the plate, and saves the canvas through the
// app's own "Save preview image" path (save_preview_image → ~/Pictures/AD3D) three times: at the
// design light, with the free preview light swung, and in the front view.

import { useImage } from '../app/actions';
import { getEngine } from '../engine';
import { useSquamaUi } from '../panels/squamaUi';
import { useAppStore } from '../state/store';
import { savePreviewImage } from '../viewport/savePreview';
import { useViewportStore } from '../viewport/viewportStore';

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function waitFor(what: string, pred: () => boolean, ms = 90_000): Promise<void> {
  const t0 = performance.now();
  while (!pred()) {
    if (performance.now() - t0 > ms) throw new Error(`timed out waiting for ${what}`);
    await sleep(100);
  }
}

export async function runE2eM1(path: string): Promise<void> {
  const log = (m: string) => console.warn(`[e2e-m1] ${m}`);
  try {
    await waitFor('the native engine', () => !!useAppStore.getState().engine?.isNative);
    const e = await getEngine();
    // Round 2: the app's one photo entry point (File → Open / Import / drop / Image panel).
    await useImage(path);
    const ui = useSquamaUi.getState().image;
    log(`ui.image after useImage: ${ui ? `${ui.widthPx}x${ui.heightPx}` : 'null'}; params.quality ${useAppStore.getState().params.quality}`);
    useAppStore.getState().setParams({ widthMm: 160, ...(import.meta.env.VITE_AD3D_E2E_OPT ? { optimiser: { kind: 'shadow', maxSweeps: 8 } } : {}) });
    const t0 = performance.now();
    await waitFor('the photo plate', () => {
      const s = useAppStore.getState();
      return s.mesh?.params?.image === path && !s.previewBusy;
    });
    log(`first plate after ${(performance.now() - t0).toFixed(0)} ms`);
    await waitFor('a final plate', () => ((useAppStore.getState().mesh?.payload.flags ?? 0) & 1) === 1);
    const m = useAppStore.getState().mesh!;
    log(`final plate after ${(performance.now() - t0).toFixed(0)} ms: ${m.payload.triangleCount} tris, flags ${m.payload.flags}, gen ${m.payload.genMs.toFixed(1)} ms, optimiser ${JSON.stringify(m.params?.optimiser ?? null)}`);
    const ji = await e.squamaJobInfo(m.payload.jobId).catch(() => null);
    log(`job info: ${JSON.stringify(ji)}`);
    await sleep(1000);
    useAppStore.getState().requestFit();
    await sleep(1500);
    log(`design light: ${await savePreviewImage()}`);
    useAppStore.getState().setView({ followDesignLight: false, previewAzimuthDeg: 160, previewElevationDeg: 9 });
    await sleep(1500);
    log(`moved light: ${await savePreviewImage()}`);
    useViewportStore.getState().set({ frontView: true });
    await sleep(1500);
    log(`front view, moved light: ${await savePreviewImage()}`);
    useAppStore.getState().setView({ followDesignLight: true });
    await sleep(1500);
    log(`front view, design light: ${await savePreviewImage()}`);
  } catch (err) {
    console.error('[e2e-m1] failed', err);
  }
}
