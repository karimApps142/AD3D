// Parameter setters that carry the preview/final quality (§13): drag → Quality::Preview,
// release or typed value → Quality::Final.

import { useCallback } from 'react';
import type { ImageOpts, SquamaParams } from '../engine/types';
import { useAppStore } from '../state/store';
import { type ChangePhase, imageOptsPatch, paramPatch, releasePatch } from './squamaDto';

export interface ParamSetter {
  set: <K extends keyof SquamaParams>(key: K, value: SquamaParams[K], phase?: ChangePhase) => void;
  patch: (patch: Partial<SquamaParams>, phase?: ChangePhase) => void;
  image: (patch: Partial<ImageOpts>, phase?: ChangePhase) => void;
  /** Slider released: regenerate at final quality if the last request was a drag preview. */
  commit: () => void;
}

export function useParamSetter(): ParamSetter {
  const set = useCallback(<K extends keyof SquamaParams>(key: K, value: SquamaParams[K], phase: ChangePhase = 'commit') => {
    const s = useAppStore.getState();
    if (s.params[key] === value && (phase === 'drag' || s.params.quality !== 'preview')) return;
    s.setParams(paramPatch(key, value, phase));
  }, []);
  const patch = useCallback((p: Partial<SquamaParams>, phase: ChangePhase = 'commit') => {
    useAppStore.getState().setParams({ ...p, quality: phase === 'drag' ? 'preview' : 'final' });
  }, []);
  const image = useCallback((p: Partial<ImageOpts>, phase: ChangePhase = 'commit') => {
    const s = useAppStore.getState();
    s.setParams(imageOptsPatch(s.params.imageOpts, p, phase));
  }, []);
  const commit = useCallback(() => {
    const s = useAppStore.getState();
    const p = releasePatch(s.params);
    if (p) s.setParams(p);
  }, []);
  return { set, patch, image, commit };
}
