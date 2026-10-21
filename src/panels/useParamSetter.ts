// Parameter setters that carry the preview/final quality (§13): drag → Quality::Preview,
// release or typed value → Quality::Final.

import { useCallback } from 'react';
import type { ImageOpts, SquamaParams } from '../engine/types';
import { useAppStore } from '../state/store';
import { followLamp } from './lamp';
import { type ChangePhase, imageOptsPatch, paramPatch, releasePatch } from './squamaDto';
import { useSquamaUi } from './squamaUi';

export interface ParamSetter {
  set: <K extends keyof SquamaParams>(key: K, value: SquamaParams[K], phase?: ChangePhase) => void;
  patch: (patch: Partial<SquamaParams>, phase?: ChangePhase) => void;
  image: (patch: Partial<ImageOpts>, phase?: ChangePhase) => void;
  /** Slider released: regenerate at final quality if the last request was a drag preview. */
  commit: () => void;
}

/**
 * Applies a user change to the params. A point lamp moves with the plate (size, crop, layout,
 * azimuth) in the same update, so the change and the lamp move are one undo step.
 */
export function applyUserPatch(p: Partial<SquamaParams>): void {
  const prev = useAppStore.getState().params;
  const next = { ...prev, ...p };
  const lamp = followLamp(prev, next, useSquamaUi.getState().image);
  useAppStore.getState().setParams(lamp ? { ...p, ...lamp } : p);
}

export function useParamSetter(): ParamSetter {
  const set = useCallback(<K extends keyof SquamaParams>(key: K, value: SquamaParams[K], phase: ChangePhase = 'commit') => {
    const s = useAppStore.getState();
    if (s.params[key] === value && (phase === 'drag' || s.params.quality !== 'preview')) return;
    applyUserPatch(paramPatch(key, value, phase));
  }, []);
  const patch = useCallback((p: Partial<SquamaParams>, phase: ChangePhase = 'commit') => {
    applyUserPatch({ ...p, quality: phase === 'drag' ? 'preview' : 'final' });
  }, []);
  const image = useCallback((p: Partial<ImageOpts>, phase: ChangePhase = 'commit') => {
    const s = useAppStore.getState();
    applyUserPatch(imageOptsPatch(s.params.imageOpts, p, phase));
  }, []);
  const commit = useCallback(() => {
    const s = useAppStore.getState();
    const p = releasePatch(s.params);
    if (p) s.setParams(p);
  }, []);
  return { set, patch, image, commit };
}
