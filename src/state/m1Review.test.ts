// Verify round 1 (M1 correctness review): regression tests for confirmed app-state bugs.

import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import type { Engine, MeshPayload, SquamaParams, SquamaParamsDto } from '../engine/types';
import { paramPatch, releasePatch } from '../panels/squamaDto';
import { paramsFromDoc } from './document';
import { paramsHistory, startParamsHistory } from './paramsHistory';
import { startPreview } from './preview';
import { DEFAULT_PARAMS, useAppStore } from './store';

describe('undo after a slider drag', () => {
  let stop: () => void = () => {};
  beforeEach(() => {
    useAppStore.setState({ params: { ...DEFAULT_PARAMS, quality: 'final' } });
    stop = startParamsHistory({ now: () => 0, coalesceMs: 1000 });
  });
  afterEach(() => stop());

  it('is one step and comes back at final quality', () => {
    const before = useAppStore.getState().params;
    // Real DOM order: useAppShell's window *capture* pointerdown/pointerup listeners run
    // before the Slider's own React handlers, so the gesture closes before onCommit fires.
    paramsHistory.beginGesture(); // window capture pointerdown
    for (let w = 301; w <= 320; w++) useAppStore.getState().setParams(paramPatch('widthMm', w, 'drag'));
    paramsHistory.endGesture(); // window capture pointerup
    const p = releasePatch(useAppStore.getState().params); // Slider onPointerUp → onCommit
    if (p) useAppStore.getState().setParams(p);
    expect(useAppStore.getState().params.quality).toBe('final');

    paramsHistory.undo();
    const after = useAppStore.getState().params;
    // One undo must restore the pre-drag plate, not the last drag preview at 'preview' quality
    // (which the release patch never upgrades again, so the plate stays low-res).
    expect(after.widthMm).toBe(before.widthMm);
    expect(after.quality).not.toBe('preview');
  });
});

describe('quality of generations that no slider started', () => {
  let dispose: () => void = () => {};
  afterEach(() => dispose());

  function captureEngine(sent: SquamaParamsDto[]): Engine {
    return {
      generateSquama: (p: SquamaParamsDto) => {
        sent.push(p);
        return new Promise<MeshPayload>(() => {}); // never resolves; we only inspect the request
      },
    } as unknown as Engine;
  }

  it('the first plate after launch is generated at final quality', () => {
    useAppStore.setState({ params: DEFAULT_PARAMS });
    const sent: SquamaParamsDto[] = [];
    dispose = startPreview(captureEngine(sent)).dispose;
    expect(sent).toHaveLength(1);
    // Missing quality is Quality::Preview in the core (no optimiser, 1024 px target, no score),
    // and releasePatch() only upgrades an explicit 'preview', so it would stay that way.
    expect(sent[0].quality).toBe('final');
  });

  it('an opened project is generated at final quality', () => {
    // project_write strips `quality` (src-tauri project.rs to_core), so opened docs lack it.
    const params: SquamaParams = paramsFromDoc({
      generator: 'shadow_plate',
      generatorVersion: 1,
      params: { ...DEFAULT_PARAMS, optimiser: { kind: 'shadow', maxSweeps: 8 } } as SquamaParams & Record<string, unknown>,
    });
    useAppStore.setState({ params });
    const sent: SquamaParamsDto[] = [];
    dispose = startPreview(captureEngine(sent)).dispose;
    expect(sent[0].quality).toBe('final');
  });
});

describe('HEIC photos', () => {
  it('are images for File → Open and drag-and-drop, as the host and core accept them', async () => {
    const { pathKind } = await import('./document');
    // src-tauri project.rs IMAGE_EXTS and ad3d_core::image::heic accept HEIC/HEIF (iPhone photos),
    // but openPath()/handleDrop() reject them as "not a project or an image".
    expect(pathKind('/Users/me/Pictures/IMG_0001.HEIC')).toBe('image');
    expect(pathKind('/Users/me/Pictures/IMG_0002.heif')).toBe('image');
  });
});
