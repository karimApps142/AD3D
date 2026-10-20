// Spike harness (docs/PLAN.md §2). Runs inside the app, against whichever engine is active, and
// measures the frontend side of the acceptance criteria:
//
//   C1  evidence that the viewport renders the plate with self-shadowing from a movable light,
//       plus a PNG of the canvas (saveArtifact "viewport.png").
//   C4  1M-triangle mesh: request → bytes (IPC), bytes → geometry (parse + BufferGeometry +
//       attach), request → frame (next rendered frame + forced GPU sync). Warm-up, then 5 cold runs
//       (a small plate is shown first, so every run creates new GPU buffers: the realistic "first
//       big mesh arrives" path) and 5 warm runs (same-size buffers reused in place). Both must pass.
//       The 1M mesh is pre-generated and cached, so generation is excluded (encode is included);
//       the frame time is gl.render + a 1-px readPixels, not present-to-screen.
//   C5  slider drag on a 50×63 plate for 5 s, twice: one input per animation frame ('raf'), and one
//       input every 4 ms from a timer ('timer') so latest-wins coalescing is exercised. Preview
//       updates rendered per second; both must reach 30. Full resolution (no low-res drag mode yet).
//       Before it, the page records document.hasFocus() and the measured rAF rate, and labels the
//       run 'throttled' when the compositor is not delivering ~60 Hz.
//
// Then finishSpike(report). Started when getLaunchFlags().spike is true or the URL has ?spike=1.

import { gridFromBBox } from '../engine/squamaMath';
import type { Engine, SquamaParams } from '../engine/types';
import { type FrameRecord, perf } from '../state/perf';
import { getPreviewController, notePayload } from '../state/preview';
import { DEFAULT_PARAMS, useAppStore } from '../state/store';
import { SHADOW_MAP_SIZE } from '../viewport/LightRig';
import { type FrameResult, viewportBus } from '../viewport/viewportBus';
import { collectEnv } from './env';
import { comparePixels } from './pixels';
import { dragSlider } from './sliderDriver';
import { countPerWindow, intervals, median, percentile, round } from './stats';

export const C4_TRIANGLES = 1_000_000;
export const C4_RUNS = 5;
export const C4_TARGET_MS = 150;
export const C5_DURATION_MS = 5000;
export const C5_TARGET_UPS = 30;
export const C5_PLATE = { widthMm: 300, cellMm: 6, nx: 50, ny: 63 } as const;
const WATCHDOG_MS = 240_000;
const WAIT_MS = 30_000;

type Json = Record<string, unknown>;

export interface SpikeReport {
  kind: 'ad3d-spike-frontend';
  version: 1;
  engine: { name: string; isNative: boolean };
  url: string;
  startedAt: string;
  finishedAt?: string;
  durationMs?: number | null;
  timedOut?: boolean;
  summary?: Json;
  criteria: { C1?: Json; C4?: Json; C5?: Json };
  env?: Json;
  errors: string[];
}

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

function status(phase: string, running = true): void {
  useAppStore.setState({ spike: { running, phase } });
  window.__SPIKE_STATUS__ = phase;
  console.log(`[spike] ${phase}`);
}

function withTimeout<T>(promise: Promise<T>, ms: number, what: string): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`timed out after ${ms} ms waiting for ${what}`)), ms);
    promise.then(
      (v) => {
        clearTimeout(timer);
        resolve(v);
      },
      (e) => {
        clearTimeout(timer);
        reject(e);
      },
    );
  });
}

async function waitFor(pred: () => boolean, ms: number, what: string): Promise<void> {
  const t0 = performance.now();
  while (!pred()) {
    if (performance.now() - t0 > ms) throw new Error(`timed out after ${ms} ms waiting for ${what}`);
    await sleep(20);
  }
}

function nextFrame(options: { gpuSync?: boolean } = {}): Promise<FrameResult> {
  return withTimeout(viewportBus.nextFrame(options), 15_000, 'a rendered frame (is the window visible?)');
}

async function frames(n: number): Promise<void> {
  for (let i = 0; i < n; i++) await nextFrame();
}

function sameParams(a: SquamaParams | undefined, b: SquamaParams): boolean {
  if (!a) return false;
  return (Object.keys(b) as Array<keyof SquamaParams>).every((k) => a[k] === b[k]);
}

/** Waits until the plate for the current params is attached, the loop is idle, and one frame drew it. */
async function waitForPlate(): Promise<void> {
  await waitFor(
    () => {
      const s = useAppStore.getState();
      const loop = getPreviewController()?.loop;
      return (
        !!s.mesh &&
        s.mesh.kind === 'squama' &&
        sameParams(s.mesh.params, s.params) &&
        !!loop &&
        !loop.busy &&
        !loop.hasPending &&
        viewportBus.applied?.version === s.mesh.version
      );
    },
    WAIT_MS,
    'the plate for the current parameters',
  );
  await nextFrame();
}

const SHADOW_TYPES = ['basic', 'pcf', 'pcf-soft', 'vsm'];

async function measureC1(engine: Engine): Promise<Json> {
  status('C1: shadows, light and screenshot');
  await waitForPlate();
  await frames(2);
  const gl = viewportBus.gl;
  const light = viewportBus.light;
  if (!gl || !light) throw new Error('viewport or key light missing');

  // Shadows: the same frame with shadow intensity 1 and 0. Pixels that get darker are in shadow.
  const shadowed = await viewportBus.readPixels();
  const intensity = light.shadow.intensity;
  light.shadow.intensity = 0;
  let unshadowed;
  try {
    unshadowed = await viewportBus.readPixels();
  } finally {
    light.shadow.intensity = intensity;
  }
  const shadowDiff = comparePixels(shadowed, unshadowed);

  // Movable light: swing the preview light 90° and compare.
  const { lightAzimuthDeg, lightElevationDeg } = useAppStore.getState().params;
  useAppStore.getState().setView({
    followDesignLight: false,
    previewAzimuthDeg: (lightAzimuthDeg + 90) % 360,
    previewElevationDeg: lightElevationDeg,
  });
  let moved;
  try {
    moved = await viewportBus.readPixels();
  } finally {
    useAppStore.getState().setView({ followDesignLight: true });
  }
  const moveDiff = comparePixels(shadowed, moved);

  await frames(2);
  const png = await viewportBus.capturePng();
  await engine.saveArtifact('viewport.png', png);

  const cam = light.shadow.camera;
  return {
    artifact: 'viewport.png',
    pngBytes: png.byteLength,
    canvas: { width: shadowed.width, height: shadowed.height },
    shadows: {
      enabled: gl.shadowMap.enabled,
      type: SHADOW_TYPES[gl.shadowMap.type] ?? String(gl.shadowMap.type),
      mapSize: light.shadow.mapSize.x,
      expectedMapSize: SHADOW_MAP_SIZE,
      bias: light.shadow.bias,
      frustumMm: {
        width: round(cam.right - cam.left),
        height: round(cam.top - cam.bottom),
        depth: round(cam.far - cam.near),
      },
    },
    shadowedPixels: shadowDiff.darker,
    shadowedFraction: round(shadowDiff.darkerFraction, 4),
    lightMoveChangedPixels: moveDiff.changed,
    lightMoveChangedFraction: round(moveDiff.changedFraction, 4),
    pass: gl.shadowMap.enabled && shadowDiff.darker > 0 && moveDiff.changed > 0,
    note: 'Frontend evidence; the native run also shows that the Tauri app launched.',
  };
}

async function c4Once(engine: Engine): Promise<Json> {
  const t0 = performance.now();
  const payload = await engine.syntheticMesh(C4_TRIANGLES);
  const tBytes = payload.timing?.receivedAt ?? performance.now();
  const tParsed = payload.timing?.parsedAt ?? tBytes;
  if (payload.triangleCount !== C4_TRIANGLES) {
    throw new Error(`synthetic mesh has ${payload.triangleCount} triangles, expected ${C4_TRIANGLES}`);
  }
  notePayload(payload);
  useAppStore.getState().showMesh({ payload, kind: 'synthetic' });
  const tGeometry = performance.now();
  const version = useAppStore.getState().mesh?.version;
  const applied = viewportBus.applied;
  if (!applied || applied.version !== version) throw new Error('the mesh was not attached to the scene');
  const frame = await nextFrame({ gpuSync: true });
  if (frame.meshVersion !== version) throw new Error('the next frame did not draw the new mesh');
  const tFrame = frame.syncedAt ?? frame.t;
  return {
    ipcMs: round(tBytes - t0),
    parseMs: round(tParsed - tBytes),
    geometryMs: round(tGeometry - tBytes),
    frameMs: round(tFrame - tGeometry),
    totalMs: round(tFrame - t0),
    genMs: round(payload.genMs),
    encodeMs: round(payload.encodeMs),
    renderCpuMs: round(frame.cpuMs),
    gpuSyncMs: round(frame.syncMs ?? Number.NaN),
    geometryMode: applied.mode,
    // True when the parsed positions/indices are views into the received buffer (payload parsing),
    // not a statement about GPU buffers; see geometryMode for those.
    payloadZeroCopy: payload.payloadZeroCopy,
    bytes: payload.byteLength,
    triangles: payload.triangleCount,
    vertices: payload.vertexCount,
  };
}

const C4_KEYS = ['ipcMs', 'parseMs', 'geometryMs', 'frameMs', 'totalMs', 'genMs', 'encodeMs', 'renderCpuMs', 'gpuSyncMs'];

function medianRow(runs: Json[]): Record<string, number | null> {
  const row: Record<string, number | null> = {};
  for (const key of C4_KEYS) {
    row[key] = round(median(runs.map((r) => (typeof r[key] === 'number' ? (r[key] as number) : Number.NaN))));
  }
  return row;
}

async function measureC4(engine: Engine): Promise<Json> {
  const loop = getPreviewController()?.loop ?? null;
  loop?.pause();
  await loop?.whenIdle();
  const plate = useAppStore.getState().mesh;
  try {
    if (!plate || plate.kind !== 'squama') throw new Error('C4 needs the plate on screen first');
    status('C4: warm-up (1M triangles)');
    const warmup = await c4Once(engine);
    // Cold: show the small plate first, so the 1M mesh needs new GPU buffers every time.
    const cold: Json[] = [];
    for (let i = 0; i < C4_RUNS; i++) {
      useAppStore.getState().showMesh({ payload: plate.payload, kind: 'squama', params: plate.params });
      await frames(2);
      status(`C4: cold run ${i + 1}/${C4_RUNS}`);
      cold.push(await c4Once(engine));
    }
    // Warm: back to back, same-size buffers reused in place.
    const warm: Json[] = [];
    for (let i = 0; i < C4_RUNS; i++) {
      await frames(2);
      status(`C4: warm run ${i + 1}/${C4_RUNS}`);
      warm.push(await c4Once(engine));
    }
    const coldMedian = medianRow(cold);
    const warmMedian = medianRow(warm);
    const coldOk = coldMedian.totalMs !== null && coldMedian.totalMs < C4_TARGET_MS;
    const warmOk = warmMedian.totalMs !== null && warmMedian.totalMs < C4_TARGET_MS;
    return {
      definition:
        'ipcMs = request → bytes on the UI thread; geometryMs = bytes → parsed + BufferGeometry attached; ' +
        'frameMs = attached → next frame rendered + GPU sync (1-px readPixels); totalMs = request → frame. ' +
        'cold = new GPU buffers every run (small plate shown in between); warm = buffers reused in place. ' +
        'The mesh is cached in Rust (generation excluded, encode included); shared-vertex torus (V = T/2).',
      triangles: C4_TRIANGLES,
      targetMs: C4_TARGET_MS,
      warmup,
      cold: { runs: cold, modes: cold.map((r) => r.geometryMode), median: coldMedian, pass: coldOk },
      warm: { runs: warm, modes: warm.map((r) => r.geometryMode), median: warmMedian, pass: warmOk },
      // Headline: the slower (cold) path.
      median: coldMedian,
      pass: coldOk && warmOk,
    };
  } finally {
    loop?.resume();
    getPreviewController()?.refresh();
  }
}

/** Focus and the rAF rate over `ms`, to label runs on a throttled compositor. */
async function displayConditions(ms = 1000): Promise<Json> {
  const times: number[] = [];
  await new Promise<void>((resolve) => {
    const t0 = performance.now();
    const f = (t: number) => {
      times.push(t);
      if (t - t0 < ms) requestAnimationFrame(f);
      else resolve();
    };
    requestAnimationFrame(f);
  });
  const iv = intervals(times);
  const p50 = percentile(iv, 50);
  const rafHz = Number.isFinite(p50) && p50 > 0 ? 1000 / p50 : Number.NaN;
  const hasFocus = document.hasFocus();
  return {
    hasFocus,
    visibilityState: document.visibilityState,
    rafHz: round(rafHz, 1),
    rafIntervalMs: { p50: round(p50), p95: round(percentile(iv, 95)) },
    throttled: !hasFocus || document.visibilityState !== 'visible' || !(rafHz >= 50),
  };
}

async function measureC5(): Promise<Json> {
  const conditions = await displayConditions();
  const raf = await measureC5Mode('raf');
  const timer = await measureC5Mode('timer');
  const rafUps = typeof raf.medianUpdatesPerSec === 'number' ? raf.medianUpdatesPerSec : null;
  const timerUps = typeof timer.medianUpdatesPerSec === 'number' ? timer.medianUpdatesPerSec : null;
  return {
    conditions,
    raf,
    timer,
    medianUpdatesPerSec: rafUps,
    timerMedianUpdatesPerSec: timerUps,
    targetUpdatesPerSec: C5_TARGET_UPS,
    throttled: conditions.throttled,
    pass: raf.pass === true && timer.pass === true,
  };
}

async function measureC5Mode(driver: 'raf' | 'timer'): Promise<Json> {
  const ctl = getPreviewController();
  if (!ctl) throw new Error('the preview loop is not running');
  status(`C5 (${driver}): generating the 50×63 plate`);
  useAppStore.getState().setParams({ ...DEFAULT_PARAMS, widthMm: C5_PLATE.widthMm, cellMm: C5_PLATE.cellMm });
  await waitForPlate();
  const mesh = useAppStore.getState().mesh;
  if (!mesh) throw new Error('no plate');
  const grid = gridFromBBox(mesh.payload.bboxMin, mesh.payload.bboxMax, C5_PLATE.cellMm);
  const gridOk = grid.nx === C5_PLATE.nx && grid.ny === C5_PLATE.ny;
  const input = document.querySelector<HTMLInputElement>('input[type="range"][data-param="maxTiltDeg"]');
  if (!input) throw new Error('max tilt slider not found');
  await frames(3);

  const log: FrameRecord[] = [];
  const listener = (f: FrameRecord) => {
    log.push(f);
  };
  perf.frameListeners.add(listener);
  // Per-update engine numbers: generation time from the payload, and request → bytes on the UI thread.
  const genTimes: number[] = [];
  const ipcTimes: number[] = [];
  const unsubscribe = useAppStore.subscribe((st, prev) => {
    if (st.mesh === prev.mesh || !st.mesh || st.mesh.kind !== 'squama') return;
    genTimes.push(st.mesh.payload.genMs);
    const t = st.mesh.payload.timing;
    if (t) ipcTimes.push(t.receivedAt - t.requestedAt);
  });
  const before = { ...ctl.loop.stats };
  status(`C5 (${driver}): dragging max tilt for 5 s`);
  let drag;
  try {
    drag = await dragSlider(input, { durationMs: C5_DURATION_MS, lo: 2, hi: 18, periodMs: 1600, digits: 1, driver, intervalMs: 4 });
  } finally {
    perf.frameListeners.delete(listener);
    unsubscribe();
  }
  const after = { ...ctl.loop.stats };

  const start = drag.startedAt;
  const end = start + C5_DURATION_MS;
  const inWindow = log.filter((f) => f.t >= start && f.t < end);
  const updates = inWindow.filter((f) => f.meshChanged);
  const windows = countPerWindow(updates.map((f) => f.t), start, C5_DURATION_MS / 1000);
  const frameIntervals = intervals(inWindow.map((f) => f.t));
  const latencies = updates.map((f) => f.latencyMs ?? Number.NaN);
  const renderCpu = inWindow.map((f) => f.cpuMs);
  const paramChanges = after.requested - before.requested;
  const sent = after.sent - before.sent;
  const medianUps = median(windows);

  await ctl.loop.whenIdle();
  status(`C5 (${driver}): restoring defaults`);
  useAppStore.getState().setParams(DEFAULT_PARAMS);
  await waitForPlate();

  return {
    plate: { expected: { nx: C5_PLATE.nx, ny: C5_PLATE.ny }, measured: grid, ok: gridOk, triangles: mesh.payload.triangleCount, vertices: mesh.payload.vertexCount },
    driver,
    slider:
      driver === 'raf'
        ? 'maxTiltDeg swept 2–18° (triangle wave, 1.6 s period), native value setter + input event every animation frame'
        : 'maxTiltDeg swept 2–18° (triangle wave, 1.6 s period), native value setter + input event every 4 ms (timer, independent of rAF)',
    durationMs: round(drag.endedAt - drag.startedAt, 0),
    updatesPerSecondWindows: windows,
    medianUpdatesPerSec: round(medianUps, 1),
    meanUpdatesPerSec: round(updates.length / (C5_DURATION_MS / 1000), 1),
    framesRendered: inWindow.length,
    frameIntervalMs: { p50: round(percentile(frameIntervals, 50)), p95: round(percentile(frameIntervals, 95)) },
    renderCpuMs: { p50: round(percentile(renderCpu, 50)), p95: round(percentile(renderCpu, 95)) },
    engineGenMs: { p50: round(percentile(genTimes, 50)), p95: round(percentile(genTimes, 95)) },
    requestToBytesMs: { p50: round(percentile(ipcTimes, 50)), p95: round(percentile(ipcTimes, 95)) },
    inputToFrameMs: { p50: round(percentile(latencies, 50)), p95: round(percentile(latencies, 95)) },
    inputEvents: drag.inputEvents,
    valueChanges: drag.valueChanges,
    paramChanges,
    requestsSent: sent,
    responsesApplied: after.applied - before.applied,
    responsesDropped: after.dropped - before.dropped,
    errors: after.errors - before.errors,
    coalescedFraction: paramChanges > 0 ? round((paramChanges - sent) / paramChanges, 3) : null,
    targetUpdatesPerSec: C5_TARGET_UPS,
    pass: gridOk && medianUps >= C5_TARGET_UPS,
  };
}

async function step(report: SpikeReport, name: string, fn: () => Promise<Json>): Promise<Json> {
  try {
    return await fn();
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    report.errors.push(`${name}: ${message}`);
    console.error(`[spike] ${name} failed`, err);
    return { error: message, pass: false };
  }
}

function summarize(report: SpikeReport): Json {
  type C4Part = { median?: { totalMs?: number | null } };
  const c4 = report.criteria.C4 as { warmup?: { totalMs?: number }; cold?: C4Part; warm?: C4Part; pass?: boolean } | undefined;
  const c5 = report.criteria.C5 as
    | { medianUpdatesPerSec?: number | null; timerMedianUpdatesPerSec?: number | null; throttled?: boolean; pass?: boolean }
    | undefined;
  const c1 = report.criteria.C1 as { pass?: boolean } | undefined;
  return {
    C1_pass: c1?.pass ?? false,
    C4_medianTotalMs: c4?.cold?.median?.totalMs ?? null,
    C4_coldMedianTotalMs: c4?.cold?.median?.totalMs ?? null,
    C4_warmMedianTotalMs: c4?.warm?.median?.totalMs ?? null,
    C4_warmupTotalMs: c4?.warmup?.totalMs ?? null,
    C4_pass: c4?.pass ?? false,
    C5_medianUpdatesPerSec: c5?.medianUpdatesPerSec ?? null,
    C5_timerMedianUpdatesPerSec: c5?.timerMedianUpdatesPerSec ?? null,
    C5_throttled: c5?.throttled ?? null,
    C5_pass: c5?.pass ?? false,
  };
}

let started = false;

/** Starts the harness once per page load. */
export function startSpikeOnce(engine: Engine): void {
  if (started) return;
  started = true;
  void runSpike(engine);
}

export async function runSpike(engine: Engine): Promise<SpikeReport> {
  const t0 = performance.now();
  const report: SpikeReport = {
    kind: 'ad3d-spike-frontend',
    version: 1,
    engine: { name: engine.name, isNative: engine.isNative },
    url: window.location.href,
    startedAt: new Date().toISOString(),
    criteria: {},
    errors: [],
  };

  let finished = false;
  const finish = async (timedOut: boolean) => {
    if (finished) return;
    finished = true;
    if (timedOut) report.timedOut = true;
    report.finishedAt = new Date().toISOString();
    report.durationMs = round(performance.now() - t0, 0);
    report.summary = summarize(report);
    window.__SPIKE_REPORT__ = report;
    const s = report.summary;
    useAppStore.setState({
      spike: {
        running: false,
        phase: timedOut ? 'timed out' : 'done',
        summary:
          `C1 ${s.C1_pass ? 'pass' : 'FAIL'} · C4 cold ${s.C4_coldMedianTotalMs ?? '–'} / warm ${s.C4_warmMedianTotalMs ?? '–'} ms ` +
          `${s.C4_pass ? 'pass' : 'FAIL'} · C5 ${s.C5_medianUpdatesPerSec ?? '–'} (timer ${s.C5_timerMedianUpdatesPerSec ?? '–'}) updates/s ` +
          `${s.C5_pass ? 'pass' : 'FAIL'}${s.C5_throttled ? ' (throttled)' : ''}`,
      },
    });
    window.__SPIKE_STATUS__ = 'done';
    try {
      await engine.finishSpike(report);
    } catch (err) {
      console.error('[spike] finishSpike failed', err);
    }
  };
  const watchdog = setTimeout(() => {
    report.errors.push(`watchdog: the spike did not finish within ${WATCHDOG_MS / 1000} s`);
    void finish(true);
  }, WATCHDOG_MS);

  try {
    status('waiting for the viewport and the first plate');
    await waitFor(() => viewportBus.ready, WAIT_MS, 'the viewport');
    await waitForPlate();
    report.env = { ...collectEnv() };
    report.criteria.C1 = await step(report, 'C1', () => measureC1(engine));
    report.criteria.C4 = await step(report, 'C4', () => measureC4(engine));
    report.criteria.C5 = await step(report, 'C5', () => measureC5());
  } catch (err) {
    report.errors.push(err instanceof Error ? err.message : String(err));
    console.error('[spike] aborted', err);
  } finally {
    clearTimeout(watchdog);
  }
  await finish(false);
  return report;
}
