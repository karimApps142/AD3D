// Building §12.1 DTOs from panel state. Pure functions, so they are unit-tested.

import type {
  ExportFormat,
  ExportOrientation,
  ImageOpts,
  Quality,
  SquamaExportDto,
  SquamaParams,
  SquamaParamsDto,
  Vec3,
} from '../engine/types';
import { isFullCrop } from './cropMath';
import { PRINTERS } from './tilePlan';

/** Slider interaction phase: 'drag' while the thumb moves, 'commit' for a final value. */
export type ChangePhase = 'drag' | 'commit';

export function qualityFor(phase: ChangePhase): Quality {
  return phase === 'drag' ? 'preview' : 'final';
}

/** Store patch for a parameter change: the value plus the matching quality (§13). */
export function paramPatch<K extends keyof SquamaParams>(key: K, value: SquamaParams[K], phase: ChangePhase): Partial<SquamaParams> {
  return { [key]: value, quality: qualityFor(phase) } as Partial<SquamaParams>;
}

/** Patch to send on slider release: only when the last generation was a drag preview. */
export function releasePatch(current: SquamaParams): Partial<SquamaParams> | null {
  return current.quality === 'preview' ? { quality: 'final' } : null;
}

/** Image opts with defaults removed, so the cache key and DTO stay small. */
export function cleanImageOpts(o: ImageOpts | undefined): ImageOpts | undefined {
  if (!o) return undefined;
  const out: ImageOpts = {};
  if (o.crop && !isFullCrop(o.crop)) out.crop = o.crop;
  if (o.gamma !== undefined && o.gamma !== 1) out.gamma = o.gamma;
  if (o.contrast !== undefined && o.contrast !== 1) out.contrast = o.contrast;
  if (o.brightness !== undefined && o.brightness !== 0) out.brightness = o.brightness;
  if (o.invert) out.invert = true;
  if (o.autoLevels) out.autoLevels = o.autoLevels;
  if (o.rotate) out.rotate = o.rotate;
  if (o.mirror) out.mirror = true;
  return Object.keys(out).length ? out : undefined;
}

/** Patch merging into imageOpts (defaults stripped). */
export function imageOptsPatch(current: ImageOpts | undefined, patch: Partial<ImageOpts>, phase: ChangePhase): Partial<SquamaParams> {
  return { imageOpts: cleanImageOpts({ ...current, ...patch }), quality: qualityFor(phase) };
}

/** Default auto-levels percentiles (§16). */
export const AUTO_LEVELS: [number, number] = [0.01, 0.99];

/**
 * Point-lamp position in plate space: `distanceMm` from the plate centre along the light
 * azimuth, `heightMm` out of the front face (w).
 */
export function lampPosition(wMm: number, hMm: number, azimuthDeg: number, distanceMm: number, heightMm: number): Vec3 {
  const a = (azimuthDeg * Math.PI) / 180;
  const r = (v: number) => Math.round(v * 1000) / 1000;
  return [r(wMm / 2 + distanceMm * Math.cos(a)), r(hMm / 2 + distanceMm * Math.sin(a)), r(heightMm)];
}

/** Light elevation seen from the plate centre for a lamp at (distance, height). */
export function lampElevationDeg(distanceMm: number, heightMm: number): number {
  return (Math.atan2(heightMm, Math.max(1e-9, distanceMm)) * 180) / Math.PI;
}

export interface ExportSettings {
  printerId: string;
  bedMm: Vec3;
  marginMm: number;
  orientation: ExportOrientation;
  formats: ExportFormat[];
  keys: 'none' | 'pegs' | 'auto';
  /** The axis the bed moves in; kept when the bed size is edited (custom printer). */
  bedMotion: 'x' | 'y' | 'none';
  stand: boolean;
  calibrationTile: boolean;
  /** Split to the bed (false = one part whatever its size). */
  tile: boolean;
}

/** Splits an absolute path into folder and file stem (either separator). */
export function splitSavePath(path: string): { outDir: string; baseName: string } {
  const i = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  const outDir = i > 0 ? path.slice(0, i) : i === 0 ? path.slice(0, 1) : '.';
  const file = path.slice(i + 1);
  const dot = file.lastIndexOf('.');
  const baseName = dot > 0 ? file.slice(0, dot) : file;
  return { outDir, baseName: baseName || 'shadow_plate' };
}

/** Default file name for the save dialog. */
export function defaultExportName(params: SquamaParams, formats: ExportFormat[]): string {
  const ext = formats.includes('3mf') ? '3mf' : 'stl';
  let stem = 'shadow_plate';
  if (params.image !== 'synthetic') {
    const file = params.image.split(/[\\/]/).pop() ?? '';
    const dot = file.lastIndexOf('.');
    const name = (dot > 0 ? file.slice(0, dot) : file).replace(/[^\w.-]+/g, '_');
    if (name) stem = `${name}_plate`;
  }
  return `${stem}.${ext}`;
}

/** The §12.1 SquamaExport for a chosen save path. Always final quality. */
export function buildExportDto(params: SquamaParams, settings: ExportSettings, savePath: string): SquamaExportDto {
  const { outDir, baseName } = splitSavePath(savePath);
  const p: SquamaParamsDto = { ...params, jobId: 0, quality: 'final' };
  const dto: SquamaExportDto = {
    params: p,
    outDir,
    baseName,
    formats: settings.formats.length ? [...settings.formats] : ['stl'],
    orientation: settings.orientation,
    marginMm: settings.marginMm,
    keys: settings.keys,
    stand: settings.stand,
    calibrationTile: settings.calibrationTile,
  };
  const printer = PRINTERS.find((p) => p.id === settings.printerId);
  dto.bedMotion = settings.bedMotion ?? printer?.bedMotion ?? 'none';
  if (settings.tile) dto.bedMm = [...settings.bedMm] as Vec3;
  return dto;
}

/** Parses "tilt, brightness" lines (manual calibration entry) into a measured response. */
export function parseMeasuredTable(text: string): { tiltDeg: number[]; brightness: number[] } | { error: string } {
  const tiltDeg: number[] = [];
  const brightness: number[] = [];
  const lines = text.split(/\r?\n/).map((l) => l.trim()).filter((l) => l && !l.startsWith('#'));
  for (const [n, line] of lines.entries()) {
    const parts = line.split(/[\s,;]+/).filter(Boolean);
    const a = Number(parts[0]);
    const b = Number(parts[1]);
    if (parts.length !== 2 || !Number.isFinite(a) || !Number.isFinite(b)) return { error: `line ${n + 1}: expected "tilt, brightness"` };
    if (b < 0 || b > 1) return { error: `line ${n + 1}: brightness must be in 0–1` };
    tiltDeg.push(a);
    brightness.push(b);
  }
  if (tiltDeg.length < 2) return { error: 'need at least two rows' };
  for (let k = 1; k < tiltDeg.length; k++) {
    if (!(tiltDeg[k] > tiltDeg[k - 1])) return { error: 'tilts must increase row by row' };
  }
  return { tiltDeg, brightness };
}
