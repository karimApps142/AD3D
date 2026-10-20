import { describe, expect, it } from 'vitest';
import { DEFAULT_PARAMS } from '../state/store';
import { mockTarget, mockThumbnail, orientedSize, unorientPoint } from './mockTarget';
import {
  ledgeWarning,
  mergeWarnings,
  predictLayout,
  predictWarnings,
  reliefMm,
  simulateLambert,
  snapCells,
  srgbDecode,
  srgbEncode,
  toneCurve,
  toneFromOpts,
} from './squamaChecks';
import { encodeTarget, encodeThumbnail, parseTarget, parseThumbnail, TARGET_MAGIC } from './squamaWire';

describe('§2.1 snapping', () => {
  it('reproduces LuBan and avoids f64 decimal ties', () => {
    expect(snapCells(200, 6)).toBe(33); // 198 mm
    expect(snapCells(202, 6)).toBe(34); // 204 mm
    expect(snapCells(100.8, 3.2)).toBe(32); // 31.5 rounds half-up in µm
    expect(snapCells(1, 6)).toBe(1);
  });

  it('predicts the synthetic grid and a fixed height', () => {
    expect(predictLayout(300, 6, 500, 630)).toMatchObject({ nx: 50, ny: 63, wMm: 300, hMm: 378 });
    expect(predictLayout(300, 3, 500, 630)).toMatchObject({ nx: 100, ny: 126 });
    expect(predictLayout(300, 6, 500, 630, 200)).toMatchObject({ ny: 33, hMm: 198 });
  });
});

describe('§16 tone curve', () => {
  it('round-trips sRGB and applies the documented order', () => {
    for (const v of [0, 0.001, 0.2, 0.5, 1]) expect(srgbEncode(srgbDecode(v))).toBeCloseTo(v, 9);
    const id = toneFromOpts(undefined);
    expect(toneCurve(0.3, id)).toBeCloseTo(0.3, 12);
    expect(toneCurve(0.3, { ...id, invert: true })).toBeCloseTo(0.7, 12);
    expect(toneCurve(0.5, { ...id, contrast: 2 })).toBeCloseTo(0.5, 12);
    expect(toneCurve(0.75, { ...id, contrast: 2 })).toBeCloseTo(1, 12);
    expect(toneCurve(0.25, { ...id, gamma: 2 })).toBeCloseTo(0.5, 12);
    expect(toneCurve(0.4, id, 0.4, 0.6)).toBeCloseTo(0, 12);
  });

  it('clamps out-of-domain options like the Rust side', () => {
    expect(toneFromOpts({ gamma: 100, contrast: -1, brightness: Number.NaN })).toEqual({
      invert: false,
      gamma: 5,
      contrast: 0,
      brightness: 0,
    });
  });
});

describe('appearance and warnings', () => {
  it('maps the target range to the full Lambert range and reports contrast', () => {
    const { values, contrast } = simulateLambert([0, 1], 5, 10, 0);
    expect(values[1]).toBeCloseTo(1, 6);
    expect(values[0]).toBeCloseTo(0, 6); // sin(α − S) < 0 → black
    expect(contrast).toBe(Number.POSITIVE_INFINITY);
    const washed = simulateLambert([0, 1], 5, 10, 0.1);
    expect(washed.values[0]).toBeCloseTo(0.1 / (0.1 + Math.sin((15 * Math.PI) / 180)), 6);
    expect(washed.contrast).toBeCloseTo((0.1 + Math.sin((15 * Math.PI) / 180)) / 0.1, 6);
  });

  it('predicts the core checks plus our own', () => {
    const base = { ...DEFAULT_PARAMS };
    expect(predictWarnings(base)).toEqual([]);
    const codes = (p: typeof base) => predictWarnings(p).map((w) => w.code);
    expect(codes({ ...base, cellMm: 2 })).toContain('small_cell');
    expect(codes({ ...base, lightElevationDeg: 2 })).toContain('low_elevation');
    expect(codes({ ...base, lightElevationDeg: 15, maxTiltDeg: 10 })).toContain('no_true_black');
    expect(codes({ ...base, cellMm: 1, maxTiltDeg: 1 })).toContain('relief_too_low');
    expect(reliefMm(6, 180, 10)).toBeCloseTo(6 * Math.tan((10 * Math.PI) / 180), 9);
    expect(ledgeWarning(6, 20, true, true)?.code).toBe('ledge_depth');
    expect(ledgeWarning(4, 10, true, true)).toBeNull();
    expect(ledgeWarning(6, 20, true, false)).toBeNull();
    expect(mergeWarnings([{ code: 'a', message: '1' }], [{ code: 'a', message: '2' }, { code: 'b', message: '3' }])).toEqual([
      { code: 'a', message: '1' },
      { code: 'b', message: '3' },
    ]);
  });
});

describe('wire formats', () => {
  it('round-trips the target grid and the thumbnail', () => {
    const t = { nx: 3, ny: 2, g: Float32Array.from([0, 0.25, 0.5, 0.75, 1, 0.125]) };
    const buf = encodeTarget(t);
    expect(new DataView(buf).getUint32(0, true)).toBe(TARGET_MAGIC);
    expect(parseTarget(buf)).toEqual(t);
    const withSim = { ...t, sim: Float32Array.from([1, 1, 1, 0, 0, 0]) };
    expect(parseTarget(encodeTarget(withSim))).toEqual(withSim);
    expect(() => parseTarget(new ArrayBuffer(20))).toThrow();
    const img = { width: 2, height: 2, data: Uint8Array.from([0, 64, 128, 255]) };
    expect(parseThumbnail(encodeThumbnail(img))).toEqual(img);
    expect(() => parseThumbnail(new Uint8Array(9))).toThrow(/mismatch/);
  });
});

describe('mock image pipeline', () => {
  it('orients points like the Rust orient_image', () => {
    expect(orientedSize(500, 630, 90)).toEqual([630, 500]);
    // 90° clockwise: the destination top-left comes from the source bottom-left.
    expect(unorientPoint(0, 0, 90, false)).toEqual([0, 1]);
    expect(unorientPoint(0, 0, 180, false)).toEqual([1, 1]);
    expect(unorientPoint(0, 0, 270, false)).toEqual([1, 0]);
    expect(unorientPoint(0.25, 0.5, 0, true)).toEqual([0.75, 0.5]);
  });

  it('builds a target of the predicted grid, honouring crop and invert', () => {
    const t = mockTarget({ ...DEFAULT_PARAMS });
    expect([t.nx, t.ny]).toEqual([50, 63]);
    const sq = mockTarget({ ...DEFAULT_PARAMS, imageOpts: { crop: [0, 0, 1, 500 / 630] } });
    expect([sq.nx, sq.ny]).toEqual([50, 50]);
    const inv = mockTarget({ ...DEFAULT_PARAMS, imageOpts: { invert: true } });
    // Inversion swaps the brightest and darkest cells.
    const hi = t.g.indexOf(Math.max(...t.g));
    const lo = t.g.indexOf(Math.min(...t.g));
    expect(inv.g[hi]).toBeLessThan(inv.g[lo]);
    const thumb = mockThumbnail(100, 90, false);
    expect(thumb.width).toBeGreaterThan(thumb.height);
    expect(thumb.data.length).toBe(thumb.width * thumb.height);
  });
});
