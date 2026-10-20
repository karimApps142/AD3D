// UX review (verify round 1): reproduces problems found in the M1 shell. Uncommitted; for the fixer.
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { paramPatch, releasePatch } from '../panels/squamaDto';
import { isDirty, pathKind } from './document';
import { paramsHistory, startParamsHistory } from './paramsHistory';
import { DEFAULT_PARAMS, useAppStore } from './store';

let t = 0;
let stop: () => void = () => {};

beforeEach(() => {
  t = 0;
  useAppStore.setState({ params: DEFAULT_PARAMS });
  stop = startParamsHistory({ now: () => t, coalesceMs: 1000 });
});
afterEach(() => stop());

describe('UX review', () => {
  it('one slider drag + release should be ONE undo step and undo should not leave a preview-quality plate', () => {
    // What Slider + useParamSetter do: window capture pointerup ends the gesture BEFORE the
    // element's onPointerUp fires onCommit → releasePatch({quality:'final'}).
    paramsHistory.beginGesture();
    for (let w = 301; w <= 320; w++) {
      t += 16;
      useAppStore.getState().setParams(paramPatch('widthMm', w, 'drag'));
    }
    paramsHistory.endGesture();
    t += 1;
    const rel = releasePatch(useAppStore.getState().params);
    if (rel) useAppStore.getState().setParams(rel);

    expect(paramsHistory.undoDepth).toBe(1); // FAILS today: 2 (the quality flip is its own step)
    paramsHistory.undo();
    expect(useAppStore.getState().params.widthMm).toBe(300);
  });

  it('saving after a release and then changing nothing is clean; quality is not document state', () => {
    const saved = { ...DEFAULT_PARAMS, quality: 'final' as const };
    expect(isDirty({ ...saved, quality: 'preview' }, { path: null, savedParams: saved })).toBe(false); // FAILS today
  });

  it('HEIC is accepted by the open dialog, so drop should accept it too', () => {
    expect(pathKind('/Users/me/IMG_0001.HEIC')).toBe('image'); // FAILS today: 'unknown'
  });
});
