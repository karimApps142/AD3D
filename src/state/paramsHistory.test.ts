import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { changeKey, paramsEqual, paramsHistory, startParamsHistory, useHistoryStatus } from './paramsHistory';
import { DEFAULT_PARAMS, useAppStore } from './store';

let t = 0;
let stop: () => void = () => {};

beforeEach(() => {
  t = 0;
  useAppStore.setState({ params: DEFAULT_PARAMS });
  stop = startParamsHistory({ now: () => t, coalesceMs: 1000 });
});

afterEach(() => stop());

const setParam = useAppStore.getState().setParam;

describe('params history', () => {
  it('records param changes and undoes them', () => {
    setParam('cellMm', 4);
    t += 5000;
    setParam('baseMm', 2);
    expect(useHistoryStatus.getState().canUndo).toBe(true);
    paramsHistory.undo();
    expect(useAppStore.getState().params.baseMm).toBe(DEFAULT_PARAMS.baseMm);
    expect(useAppStore.getState().params.cellMm).toBe(4);
    paramsHistory.undo();
    expect(useAppStore.getState().params).toEqual(DEFAULT_PARAMS);
    expect(useHistoryStatus.getState().canUndo).toBe(false);
    paramsHistory.redo();
    paramsHistory.redo();
    expect(useAppStore.getState().params.baseMm).toBe(2);
  });

  it('a slider drag is one undo step', () => {
    paramsHistory.beginGesture();
    for (let w = 301; w <= 400; w++) {
      t += 16;
      setParam('widthMm', w);
    }
    paramsHistory.endGesture();
    expect(paramsHistory.undoDepth).toBe(1);
    paramsHistory.undo();
    expect(useAppStore.getState().params.widthMm).toBe(300);
  });

  it('undo itself is not recorded', () => {
    setParam('cellMm', 4);
    paramsHistory.undo();
    expect(paramsHistory.canRedo).toBe(true);
    expect(paramsHistory.undoDepth).toBe(0);
  });

  it('load replaces params without an undo step', () => {
    setParam('cellMm', 4);
    paramsHistory.load({ ...DEFAULT_PARAMS, widthMm: 196 });
    expect(useAppStore.getState().params.widthMm).toBe(196);
    expect(paramsHistory.canUndo).toBe(false);
    expect(paramsHistory.canRedo).toBe(false);
  });

  it('change keys name the changed fields', () => {
    expect(changeKey(DEFAULT_PARAMS, { ...DEFAULT_PARAMS, widthMm: 1 })).toBe('widthMm');
    expect(changeKey(DEFAULT_PARAMS, { ...DEFAULT_PARAMS, widthMm: 1, cellMm: 2 })).toBe('cellMm,widthMm');
    expect(paramsEqual(DEFAULT_PARAMS, { ...DEFAULT_PARAMS })).toBe(true);
  });
});
