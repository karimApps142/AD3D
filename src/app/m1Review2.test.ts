// Verify round 2 (M1 correctness review): regression tests for the app-state bugs it found.
// The lamp controls are derived from params.light.posMm (panels/lamp.ts); the old "keep the lamp
// in step" render effect is gone, so these drive the same functions the panel calls.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useSquamaInfo } from '../engine/squamaInfo';
import type { SquamaParams } from '../engine/types';
import { plateLayout } from '../panels/plateLayout';
import { followLamp, lampFromPos, placeLamp } from '../panels/lamp';
import { applyUserPatch } from '../panels/useParamSetter';
import { useSquamaUi } from '../panels/squamaUi';
import { buildProjectDoc, exportFromDoc, isDirty, paramsFromDoc, useDocumentStore } from '../state/document';
import { parseTyped } from '../panels/Slider';
import { DEFAULT_EXPORT } from '../panels/squamaUi';
import { DEFAULT_VIEW } from '../state/store';
import { paramsHistory, startParamsHistory } from '../state/paramsHistory';
import { DEFAULT_PARAMS, useAppStore } from '../state/store';
import * as actions from './actions';

const A = '/Users/me/Pictures/portrait.jpg';
const B = '/Users/me/Pictures/landscape.jpg';
const PROJECT = '/Users/me/Documents/lamp.ad3d';

const SIZES: Record<string, [number, number]> = { [A]: [1536, 2048], [B]: [2048, 1536] };

vi.mock('../engine', () => ({
  getEngine: async () => ({
    isNative: true,
    imageInfo: async (path: string) => {
      const [w, h] = SIZES[path] ?? [1000, 1000];
      return { path, widthPx: w * 2, heightPx: h * 2, workingWidthPx: w, workingHeightPx: h };
    },
  }),
}));

// A project whose point lamp is 1000 mm left of a 300 mm plate's centre and 200 mm out of the face.
let projectParams: SquamaParams & Record<string, unknown> = { ...DEFAULT_PARAMS };
vi.mock('./host', () => ({
  getHost: () => ({
    readProject: async (path: string) => ({
      path,
      doc: { generator: 'shadow_plate', generatorVersion: 1, params: projectParams },
      warnings: [],
    }),
    addRecent: async () => {
      throw new Error('no recent list in tests');
    },
  }),
}));

let stops: Array<() => void> = [];
beforeEach(() => {
  useAppStore.setState({ params: { ...DEFAULT_PARAMS } });
  useSquamaUi.setState({ image: null, lamp: { distanceMm: 600, heightMm: 60 } });
  let t = 0;
  // Every change is its own step (as when the user pauses between actions).
  stops = [startParamsHistory({ now: () => (t += 10_000), coalesceMs: 1000 }), actions.startImageSync()];
});
afterEach(() => stops.forEach((s) => s()));

function layoutNow() {
  return plateLayout(useAppStore.getState().params, useSquamaUi.getState().image, useSquamaInfo.getState().info);
}

/** The lamp distance slider (ParamsPanel moveLamp). */
function dragLampDistance(distanceMm: number): void {
  const p = useAppStore.getState().params;
  const l = layoutNow();
  const now = lampFromPos(l.wMm, l.hMm, p.light!.posMm);
  applyUserPatch({ ...placeLamp(l.wMm, l.hMm, p.lightAzimuthDeg, { distanceMm, heightMm: now.heightMm }), quality: 'final' });
}

describe('undo of a photo change', () => {
  it('brings back the size of the photo it restores', async () => {
    await actions.useImage(A);
    await vi.waitFor(() => expect(useSquamaUi.getState().image?.path).toBe(A), { timeout: 500 });
    await actions.useImage(B);
    await vi.waitFor(() => expect(useSquamaUi.getState().image?.path).toBe(B), { timeout: 500 });

    paramsHistory.undo();
    expect(useAppStore.getState().params.image).toBe(A);
    // startImageSync reloads the size of whatever photo params.image points at.
    await vi.waitFor(() => expect(useSquamaUi.getState().image?.path).toBe(A), { timeout: 500 });
  });
});

describe('point lamp', () => {
  it('a saved lamp position survives opening the project', async () => {
    const posMm: [number, number, number] = [150 - 1000, 189, 200];
    projectParams = { ...DEFAULT_PARAMS, image: 'synthetic', light: { kind: 'point', posMm } };
    await actions.openPath(PROJECT);
    expect(useAppStore.getState().params.light?.posMm).toEqual(posMm);
    expect(isDirty(useAppStore.getState().params, useDocumentStore.getState())).toBe(false);
    expect(paramsHistory.canUndo).toBe(false);
    // The lamp controls show the saved placement: 1000 mm from the 300 × 378 plate's centre, 200 up.
    const l = layoutNow();
    const shown = lampFromPos(l.wMm, l.hMm, posMm);
    expect(shown.distanceMm).toBeCloseTo(1000, 6);
    expect(shown.heightMm).toBe(200);
  });

  it('undoing a lamp move is not immediately re-applied (undo is not stuck)', () => {
    useAppStore.getState().setParams({ light: { kind: 'point', posMm: [150 - 600, 189, 60] }, quality: 'final' });
    const before = useAppStore.getState().params.light?.posMm;
    dragLampDistance(900);
    expect(useAppStore.getState().params.light?.posMm).toEqual([150 - 900, 189, 60]);

    paramsHistory.undo();
    expect(useAppStore.getState().params.light?.posMm).toEqual(before);
    expect(paramsHistory.canRedo).toBe(true);
    // The slider follows the restored lamp.
    const l = layoutNow();
    expect(lampFromPos(l.wMm, l.hMm, before!).distanceMm).toBeCloseTo(600, 6);
    paramsHistory.redo();
    expect(useAppStore.getState().params.light?.posMm).toEqual([150 - 900, 189, 60]);
  });

  it('a width change moves the lamp with the plate in the same undo step', () => {
    useAppStore.getState().setParams({ light: { kind: 'point', posMm: [150 - 600, 189, 60] }, quality: 'final' });
    applyUserPatch({ widthMm: 160, quality: 'final' });
    const p = useAppStore.getState().params;
    const l = layoutNow();
    const after = lampFromPos(l.wMm, l.hMm, p.light!.posMm);
    expect(after.distanceMm).toBeCloseTo(600, 3);
    expect(p.light!.posMm[1]).toBeCloseTo(l.hMm / 2, 3);
    paramsHistory.undo();
    expect(useAppStore.getState().params.widthMm).toBe(300);
    expect(useAppStore.getState().params.light?.posMm).toEqual([150 - 600, 189, 60]);
  });

  it('follows an azimuth change around the plate centre, and never a change of the light itself', () => {
    const prev = { ...DEFAULT_PARAMS, light: { kind: 'point' as const, posMm: [150 - 600, 189, 60] as [number, number, number] } };
    const turned = followLamp(prev, { ...prev, lightAzimuthDeg: 270 }, null);
    expect(turned?.light?.posMm[0]).toBeCloseTo(150, 3);
    expect(turned?.light?.posMm[1]).toBeCloseTo(189 - 600, 3);
    expect(followLamp(prev, { ...prev, light: { kind: 'point', posMm: [0, 0, 1] } }, null)).toBeNull();
    expect(followLamp(prev, { ...prev, maxTiltDeg: 12 }, null)).toBeNull();
  });
});

describe('number boxes', () => {
  it('accept only whole numbers', () => {
    expect(parseTyped('250')).toBe(250);
    expect(parseTyped(' .5 ')).toBe(0.5);
    expect(parseTyped('-3.')).toBe(-3);
    expect(parseTyped('4j')).toBeNull();
    expect(parseTyped('')).toBeNull();
    expect(parseTyped('1e3')).toBeNull();
  });
});

describe('projects', () => {
  it('save and restore the export settings', () => {
    const settings = { ...DEFAULT_EXPORT, orientation: 'standing-q2' as const, stand: true, calibrationTile: true, tile: false, bedMm: [300, 300, 400] as [number, number, number], printerId: 'custom' };
    const doc = buildProjectDoc({ params: DEFAULT_PARAMS, view: DEFAULT_VIEW }, 'creality-cr6-se', settings);
    expect(doc.printer).toBe('custom');
    expect(exportFromDoc(JSON.parse(JSON.stringify(doc)), DEFAULT_EXPORT)).toEqual(settings);
    expect(exportFromDoc({ ...doc, export: undefined }, DEFAULT_EXPORT)).toEqual(DEFAULT_EXPORT);
  });

  it('a project without an optimiser field opens with the optimiser off (the core meaning)', () => {
    const { optimiser: _o, ...v1 } = DEFAULT_PARAMS;
    expect(paramsFromDoc({ generator: 'shadow_plate', generatorVersion: 1, params: v1 }).optimiser).toEqual({ kind: 'none' });
    expect(paramsFromDoc({ generator: 'shadow_plate', generatorVersion: 1, params: { ...DEFAULT_PARAMS } }).optimiser).toEqual({ kind: 'shadow', maxSweeps: 8 });
  });
});

describe('hex plates in the panels', () => {
  it('use the core layout, so sizes, the lamp and core warnings are for the hex plate', () => {
    const params: SquamaParams = { ...DEFAULT_PARAMS, image: 'synthetic', layout: { kind: 'hex', orientation: 'pointy' } };
    // What the core reports for this plate (CLI: "grid: 100 x 73 cells of 6 mm -> plate 300.0 x 379.3 mm").
    const info = {
      jobId: 1,
      quality: 'final' as const,
      layout: { kind: 'hex' as const, orientation: 'pointy' as const, nx: 100, ny: 73, wMm: 300, hMm: 379.31912685758414, cellMm: 6 },
      warnings: [{ code: 'relief_guard', message: 'core warning' }],
      genMs: 1,
      triangles: 1,
      validated: true,
    };
    const layout = plateLayout(params, null, info as never);
    // predictLayout() mirrors the core's hex snapping, so the core report agrees and is used.
    expect(layout.nx).toBe(info.layout.nx);
    expect(layout.hMm).toBeCloseTo(info.layout.hMm, 6);
    // The prediction alone already gives the hex size (flat-top too: 301.4 × 300 for 300 mm).
    expect(plateLayout(params, null, null).hMm).toBeCloseTo(379.31912685758414, 6);
    const flat = plateLayout({ ...params, layout: { kind: 'hex', orientation: 'flat' } }, null, null);
    expect(flat.wMm).toBeCloseTo(301.377, 2);
  });
});
