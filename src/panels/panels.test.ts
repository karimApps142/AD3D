import { describe, expect, it } from 'vitest';
import { DEFAULT_PARAMS } from '../state/store';
import { dragCrop, hitCrop, isFullCrop, normalizeCrop, rotateCrop, snapCrop, type Crop } from './cropMath';
import { gridToRgba } from './gridImage';
import { plateLayout, predictPlate } from './plateLayout';
import {
  buildExportDto,
  cleanImageOpts,
  defaultExportName,
  imageOptsPatch,
  lampElevationDeg,
  lampPosition,
  paramPatch,
  parseMeasuredTable,
  releasePatch,
  splitSavePath,
} from './squamaDto';
import { DEFAULT_EXPORT, type ExportStatus, IDLE_EXPORT, reduceExportEvent } from './squamaUi';
import {
  autoQuarterTurns,
  fitsBed,
  lightAlongPrintZ,
  maxTileExtent,
  planTiles,
  printSize,
  quarterTurns,
  splitEven,
} from './tilePlan';

const close = (a: readonly number[], b: readonly number[], d = 9) => a.forEach((v, k) => expect(v).toBeCloseTo(b[k], d));

describe('crop rectangle', () => {
  it('normalises, detects full frame and hit-tests handles', () => {
    expect(normalizeCrop([0.8, 0.9, 0.2, 0.1])).toEqual([0.2, 0.1, 0.8, 0.9]);
    expect(normalizeCrop([0.5, 0.5, 0.5, 0.5])[2] - 0.5).toBeCloseTo(0.02, 12);
    expect(isFullCrop(undefined)).toBe(true);
    expect(isFullCrop([0, 0, 1, 1])).toBe(true);
    expect(isFullCrop([0, 0, 0.9, 1])).toBe(false);
    const c: Crop = [0.2, 0.2, 0.8, 0.8];
    expect(hitCrop(c, 0.5, 0.5, 0.02, 0.02)).toBe('move');
    expect(hitCrop(c, 0.2, 0.2, 0.02, 0.02)).toBe('nw');
    expect(hitCrop(c, 0.81, 0.5, 0.02, 0.02)).toBe('e');
    expect(hitCrop(c, 0.05, 0.05, 0.02, 0.02)).toBe('new');
  });

  it('drags without leaving the image', () => {
    const c: Crop = [0.2, 0.2, 0.6, 0.6];
    close(dragCrop(c, 'move', 0.4, 0.4, 0.9, 0.9), [0.6, 0.6, 1, 1]);
    close(dragCrop(c, 'se', 0.6, 0.6, 0.7, 0.8), [0.2, 0.2, 0.7, 0.8]);
    close(dragCrop(c, 'w', 0.2, 0.4, 0.7, 0.4), [0.6, 0.2, 0.7, 0.6]); // crossing flips, not inverts
    close(dragCrop(c, 'new', 0.1, 0.1, 0.3, 0.5), [0.1, 0.1, 0.3, 0.5]);
  });

  it('snaps the crop to the whole-cell aspect ny : nx', () => {
    // Synthetic 500×630 image, 300 mm at cell 6 → nx = 50. A 300×300 px crop → 50 × 50 cells.
    const s = snapCrop([0, 0, 0.6, 300 / 630 + 0.01], 500, 630, 300, 6);
    expect(s.nx).toBe(50);
    const wPx = (s.crop[2] - s.crop[0]) * 500;
    const hPx = (s.crop[3] - s.crop[1]) * 630;
    expect(hPx / wPx).toBeCloseTo(s.ny / s.nx, 9);
    // The core's §2.1 rule on the snapped crop gives the same ny.
    expect(predictPlate({ ...DEFAULT_PARAMS, imageOpts: { crop: s.crop } }, null).ny).toBe(s.ny);
    // A fixed height forces the plate aspect inside the dragged rectangle.
    const f = snapCrop([0, 0, 1, 1], 500, 630, 300, 6, 150);
    expect([f.nx, f.ny]).toEqual([50, 25]);
    expect(((f.crop[3] - f.crop[1]) * 630) / ((f.crop[2] - f.crop[0]) * 500)).toBeCloseTo(0.5, 9);
    expect(f.crop[0]).toBeCloseTo(0, 9);
    expect(f.crop[1]).toBeGreaterThan(0); // centred vertically
    for (const v of f.crop) expect(v).toBeGreaterThanOrEqual(0);
  });

  it('follows the picture through rotation and mirroring', () => {
    const c: Crop = [0.1, 0.2, 0.4, 0.5];
    close(rotateCrop(c, 1, false), [0.5, 0.1, 0.8, 0.4]);
    close(rotateCrop(rotateCrop(c, 1, false), 3, false), c);
    close(rotateCrop(c, 4, false), c);
    close(rotateCrop(c, 0, true), [0.6, 0.2, 0.9, 0.5]);
  });
});

describe('DTO building', () => {
  it('tags drags as preview and releases as final (§13)', () => {
    expect(paramPatch('widthMm', 200, 'drag')).toEqual({ widthMm: 200, quality: 'preview' });
    expect(paramPatch('cellMm', 4, 'commit')).toEqual({ cellMm: 4, quality: 'final' });
    expect(releasePatch({ ...DEFAULT_PARAMS, quality: 'preview' })).toEqual({ quality: 'final' });
    expect(releasePatch({ ...DEFAULT_PARAMS, quality: 'final' })).toBeNull();
    expect(releasePatch({ ...DEFAULT_PARAMS })).toBeNull();
  });

  it('strips default image options', () => {
    expect(cleanImageOpts({ gamma: 1, contrast: 1, brightness: 0, invert: false, crop: [0, 0, 1, 1], rotate: 0 })).toBeUndefined();
    expect(cleanImageOpts({ gamma: 1.5, mirror: true, autoLevels: null })).toEqual({ gamma: 1.5, mirror: true });
    expect(imageOptsPatch({ gamma: 2 }, { invert: true }, 'drag')).toEqual({ imageOpts: { gamma: 2, invert: true }, quality: 'preview' });
  });

  it('keeps the v1 fields untouched and adds only optional M1 ones', () => {
    const params = { ...DEFAULT_PARAMS, quality: 'preview' as const, layout: { kind: 'hex' as const, orientation: 'flat' as const } };
    const dto = buildExportDto(params, DEFAULT_EXPORT, '/Users/me/Prints/mona.3mf');
    expect(dto.outDir).toBe('/Users/me/Prints');
    expect(dto.baseName).toBe('mona');
    expect(dto.params).toMatchObject({ ...DEFAULT_PARAMS, jobId: 0, quality: 'final', layout: { kind: 'hex', orientation: 'flat' } });
    expect(dto.bedMm).toEqual([235, 235, 250]);
    expect(dto.orientation).toBe('standing-auto');
    expect(dto.formats).toEqual(['3mf', 'stl']);
    const untiled = buildExportDto(DEFAULT_PARAMS, { ...DEFAULT_EXPORT, tile: false, formats: [] }, 'C:\\out\\x.stl');
    expect(untiled.bedMm).toBeUndefined();
    expect(untiled.formats).toEqual(['stl']);
    expect(untiled.outDir).toBe('C:\\out');
    // The JSON has exactly the §12.1 keys (plus baseName and the printer's bedMotion).
    expect(Object.keys(JSON.parse(JSON.stringify(dto))).sort()).toEqual(
      ['baseName', 'bedMm', 'bedMotion', 'calibrationTile', 'formats', 'keys', 'marginMm', 'orientation', 'outDir', 'params', 'stand'].sort(),
    );
  });

  it('names files and splits save paths', () => {
    expect(splitSavePath('/a/b/plate.v2.3mf')).toEqual({ outDir: '/a/b', baseName: 'plate.v2' });
    expect(splitSavePath('/plate.stl')).toEqual({ outDir: '/', baseName: 'plate' });
    expect(defaultExportName(DEFAULT_PARAMS, ['stl'])).toBe('shadow_plate.stl');
    expect(defaultExportName({ ...DEFAULT_PARAMS, image: '/x/Mona Lisa.jpg' }, ['3mf', 'stl'])).toBe('Mona_Lisa_plate.3mf');
  });

  it('places the point lamp on the light side', () => {
    close(lampPosition(300, 378, 180, 600, 60), [-450, 189, 60]);
    close(lampPosition(300, 378, 90, 500, 50), [150, 689, 50]);
    expect(lampElevationDeg(100, 100)).toBeCloseTo(45, 9);
  });

  it('parses a manual calibration table', () => {
    expect(parseMeasuredTable('# c\n-10, 0\n0 0.4\n10;1')).toEqual({ tiltDeg: [-10, 0, 10], brightness: [0, 0.4, 1] });
    expect(parseMeasuredTable('0, 0')).toHaveProperty('error');
    expect(parseMeasuredTable('0, 0\n0, 1')).toHaveProperty('error');
    expect(parseMeasuredTable('0, 0\n5, 2')).toHaveProperty('error');
  });
});

describe('tile plan (mirrors squama_cmds.rs)', () => {
  it('matches the CR-6 SE example', () => {
    expect(autoQuarterTurns(180)).toBe(1);
    expect(autoQuarterTurns(270)).toBe(0);
    expect(autoQuarterTurns(90)).toBe(2);
    expect(quarterTurns('standing-q3', 0)).toBe(3);
    expect(quarterTurns('flat', 0)).toBeNull();
    const uv = maxTileExtent('standing-auto', [235, 235, 250], 5, 180);
    expect(uv).toEqual([245, 225]);
    const plan = planTiles(50, 63, 6, uv);
    expect(plan.cols).toEqual([25, 25]);
    expect(plan.rows).toEqual([32, 31]);
    expect(plan.fitsWhole).toBe(false);
    expect(planTiles(30, 30, 6, uv).fitsWhole).toBe(true);
    expect(planTiles(10, 10, 6, [5, 100]).error).toBeTruthy();
    expect(splitEven(10, 3)).toEqual([4, 3, 3]);
  });

  it('checks bed fit in print space', () => {
    expect(printSize(300, 378, 4, 1)).toEqual([378, 4, 300]);
    expect(printSize(300, 378, 4, 0)).toEqual([300, 4, 378]);
    expect(printSize(300, 378, 4, null)).toEqual([300, 378, 4]);
    expect(fitsBed([150, 4, 200], [235, 235, 250], 5)).toBe(true);
    expect(fitsBed([378, 4, 300], [235, 235, 250], 5)).toBe(false);
    expect(fitsBed([4, 225, 240], [235, 235, 250], 5)).toBe(true);
    expect(lightAlongPrintZ(180, 1)).toBe(true);
    expect(lightAlongPrintZ(180, 0)).toBe(false);
    expect(lightAlongPrintZ(180, null)).toBe(false);
  });
});

describe('panel state', () => {
  it('folds export events into the status', () => {
    let s: ExportStatus = { ...IDLE_EXPORT, phase: 'running' };
    s = reduceExportEvent(s, { kind: 'started', totalParts: 4 });
    s = reduceExportEvent(s, { kind: 'progress', part: 'p_r1c1', index: 0, totalParts: 4, stage: 'writing', fraction: 0.2 });
    s = reduceExportEvent(s, { kind: 'partWritten', part: 'p_r1c1', path: '/o/p_r1c1.stl', triangles: 9 });
    s = reduceExportEvent(s, { kind: 'warning', code: 'w', message: 'm' });
    s = reduceExportEvent(s, { kind: 'warning', code: 'w', message: 'again' });
    expect(s).toMatchObject({ totalParts: 4, fraction: 0.2, stage: 'writing', part: 'p_r1c1', written: ['/o/p_r1c1.stl'] });
    expect(s.warnings).toEqual([{ code: 'w', message: 'm' }]);
  });

  it('prefers a matching core layout and never a stale one', () => {
    const info = {
      jobId: 3,
      quality: 'final' as const,
      layout: { kind: 'square' as const, nx: 50, ny: 63, wMm: 300, hMm: 378, cellMm: 6 },
      warnings: [],
      genMs: 1,
      triangles: 1,
      validated: null,
    };
    expect(plateLayout(DEFAULT_PARAMS, null, info)).toEqual(info.layout && { nx: 50, ny: 63, wMm: 300, hMm: 378, cellMm: 6 });
    expect(plateLayout({ ...DEFAULT_PARAMS, cellMm: 3 }, null, info).nx).toBe(100);
  });

  it('draws grids top row first', () => {
    const px = gridToRgba([0, 0, 1, 1], 2, 2, false);
    expect(Array.from(px.slice(0, 4))).toEqual([255, 255, 255, 255]); // top-left = row 1
    expect(px[8]).toBe(0);
  });
});
