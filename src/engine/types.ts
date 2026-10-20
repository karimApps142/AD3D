// Shared engine contract between the UI and whatever generates meshes (Rust via Tauri, or the
// in-browser mock). Units: millimetres; the DTO carries degrees because it mirrors the UI controls,
// and the Rust side converts to radians.

export type Vec3 = [number, number, number];

/** JS → Rust request for one Squama plate (JSON, camelCase keys). */
export interface SquamaParamsDto {
  /** Echoed back in the payload header so stale responses can be dropped. u32. */
  jobId: number;
  /** Requested plate width; the generator snaps it to whole cells. */
  widthMm: number;
  /** Square cell side. */
  cellMm: number;
  /** Direction the light comes from in the plate plane: 0 = right, 90 = top, 180 = left, 270 = bottom. */
  lightAzimuthDeg: number;
  /** Grazing angle of the light above the plate plane (LuBan "Light angle", 0–20). */
  lightElevationDeg: number;
  /** Largest facet tilt (LuBan "Surface angle", 1–20). */
  maxTiltDeg: number;
  /** Minimum material behind any facet (LuBan "Thickness"). */
  baseMm: number;
  /** "synthetic" (built-in test pattern, aspect H/W = 63/50) or a filesystem path to a photo. */
  image: string;

  // ---- M1 additions (docs/design/squama-mesher.md §12.1). All optional, so v1 payloads stay valid.
  /** 'preview' while a slider drags, 'final' on release. Rust default: 'preview'. */
  quality?: Quality;
  /** Requested plate height; snapped to whole cells. Absent = from the crop aspect. */
  heightMm?: number;
  layout?: LayoutSpec;
  /** Overrides the v1 (directional) light fields. Plate space, mm. */
  light?: { kind: 'point'; posMm: Vec3 };
  /** Height budget above the base for the optimiser. */
  maxReliefMm?: number;
  imageOpts?: ImageOpts;
  response?: ResponseSpec;
  optimiser?: OptimiserSpec;
}

export type Quality = 'preview' | 'final';
export type HexOrientation = 'pointy' | 'flat';
export type LayoutSpec = { kind: 'square' } | { kind: 'hex'; orientation: HexOrientation };
export type ResponseSpec =
  | { kind: 'lambert'; ambient: number }
  | { kind: 'measured'; tiltDeg: number[]; brightness: number[] };
export type OptimiserSpec = { kind: 'none' } | { kind: 'shadow'; maxSweeps?: number };

/** §12.1 imageOpts (§16 tone order: invert, auto-levels, brightness, contrast, gamma). */
export interface ImageOpts {
  /** Normalised x0, y0, x1, y1 in the rotated/mirrored image, y down. */
  crop?: [number, number, number, number];
  /** 0.2–5, default 1 (v ← v^(1/γ)). */
  gamma?: number;
  /** 0–3, default 1, pivot 0.5. */
  contrast?: number;
  /** −1…1, default 0, added. */
  brightness?: number;
  invert?: boolean;
  /** Percentiles (lo, hi); null/absent = off. */
  autoLevels?: [number, number] | null;
  /** AD3D addition: clockwise quarter turn in degrees (0, 90, 180, 270), applied before the crop. */
  rotate?: 0 | 90 | 180 | 270;
  /** AD3D addition: left↔right flip after the rotation. */
  mirror?: boolean;
}

export type ExportOrientation = 'standing-auto' | 'standing-q0' | 'standing-q1' | 'standing-q2' | 'standing-q3' | 'flat';
export type ExportFormat = 'stl' | '3mf';

/** §12.1 SquamaExport. `baseName` (file stem from the save dialog) is an AD3D addition. */
export interface SquamaExportDto {
  params: SquamaParamsDto;
  outDir: string;
  baseName?: string;
  formats: ExportFormat[];
  orientation: ExportOrientation;
  bedMm?: Vec3;
  marginMm?: number;
  keys?: 'none' | 'pegs';
  stand?: boolean;
  calibrationTile?: boolean;
  /** AD3D addition: the axis the printer's bed moves in (default 'y', the CR-6 SE). */
  bedMotion?: 'x' | 'y' | 'none';
}

export interface CoreWarning {
  code: string;
  message: string;
}

/** Match of the simulated appearance (§15.1 forward model, cast shadows) against the target. */
export interface MatchScore {
  /** Area-weighted Pearson correlation, −1…1. */
  pearson: number;
  /** RMSE relative to the target range. */
  rmseRel: number;
  /** Visible contrast p99 / p1; null when the darkest facets are fully black. */
  contrast: number | null;
}

/** What the core reports about one generation (squama_job_info). */
export interface SquamaJobInfo {
  jobId: number;
  quality: Quality;
  layout: {
    kind: 'square' | 'hex';
    orientation?: HexOrientation | null;
    nx: number;
    ny: number;
    wMm: number;
    hMm: number;
    cellMm: number;
    facets?: number;
  };
  warnings: CoreWarning[];
  genMs: number;
  triangles: number;
  validated: boolean | null;
  /** Achievable contrast (B_hi − B_lo) / B_hi of the design light. */
  contrast?: number;
  maxTiltUsedDeg?: number;
  /** Final quality only: match of the simulated plate (after the optimiser when it ran). */
  score?: MatchScore | null;
  /** The plain inverse's score, when the optimiser ran. */
  scoreBefore?: MatchScore | null;
  optimiserSweeps?: number | null;
  timings?: Record<string, number>;
}

/** The tile plan an export would use (squama_tile_plan). */
export interface TilePlan {
  /** Cells (square) or lattice units (hex) per column / row, from the lower left. */
  cols: number[];
  rows: number[];
  /** Tile boundaries in plate mm. */
  uEdgesMm: number[];
  vEdgesMm: number[];
  quarterTurns: number | null;
  wMm: number;
  hMm: number;
}

export interface ImageInfo {
  path: string;
  widthPx: number;
  heightPx: number;
  workingWidthPx: number;
  workingHeightPx: number;
}

/** 8-bit sRGB grey image, top row first. */
export interface GrayImage {
  width: number;
  height: number;
  data: Uint8Array;
}

/** Cell-averaged target (linear g ∈ [0, 1]), row 0 at the bottom of the plate (v = 0). */
export interface TargetGrid {
  nx: number;
  ny: number;
  g: Float32Array;
  /** Simulated appearance from the core's forward model (§15.1), when it provides one. */
  sim?: Float32Array;
}

export type ExportEvent =
  | { kind: 'started'; totalParts: number }
  | { kind: 'progress'; part: string; index: number; totalParts: number; stage: string; fraction: number }
  | { kind: 'partWritten'; part: string; path: string; triangles: number }
  | { kind: 'warning'; code: string; message: string };

export interface ExportPartReport {
  name: string;
  files: string[];
  triangles: number;
  sizeMm: Vec3;
  fitsBed: boolean | null;
  validated: boolean;
}

export interface ExportResult {
  outDir: string;
  files: string[];
  parts: ExportPartReport[];
  tiles: { cols: number[]; rows: number[] } | null;
  warnings: CoreWarning[];
  elapsedMs: number;
  /** Optimiser before → after, when it ran. */
  optimise?: { sweeps: number; before: MatchScore; after: MatchScore; timedOut: boolean } | null;
}

/** The user-editable part of the request (everything but the job id). */
export type SquamaParams = Omit<SquamaParamsDto, 'jobId'>;

export interface LaunchFlags {
  spike: boolean;
}

/** performance.now() timestamps recorded by the engine around one request. */
export interface MeshTiming {
  /** Just before the request left the UI thread. */
  requestedAt: number;
  /** When the raw bytes arrived on the UI thread (before parsing). */
  receivedAt: number;
  /** After the header was validated and the typed-array views were created. */
  parsedAt: number;
}

/** A decoded binary mesh payload (see payload.ts for the wire format). */
export interface MeshPayload {
  readonly version: number;
  readonly vertexCount: number;
  readonly indexCount: number;
  readonly triangleCount: number;
  readonly bboxMin: Vec3;
  readonly bboxMax: Vec3;
  /** Generation time reported by the producer (Rust or mock worker). */
  readonly genMs: number;
  /** Encode time reported by the producer. */
  readonly encodeMs: number;
  readonly jobId: number;
  readonly flags: number;
  /** xyz per vertex, plate space (three.js x = u, y = v, z = w). Usually a view into the payload. */
  readonly positions: Float32Array<ArrayBuffer>;
  /** Triangle list (u32). Usually a view into the payload. */
  readonly indices: Uint32Array<ArrayBuffer>;
  /** §12.2 face kinds (flags bit 1): low byte = kind (Facet, Wall, Back, Key, Stand), high byte = part. */
  readonly faceKinds?: Uint16Array<ArrayBuffer> | null;
  /** Total payload size in bytes. */
  readonly byteLength: number;
  /** True when positions/indices are views into the received buffer (no copy was made). */
  readonly payloadZeroCopy: boolean;
  /** Filled in by the engine that fetched the payload. */
  timing?: MeshTiming;
}

export interface Engine {
  /** "tauri" when running inside the desktop shell, "mock" in a plain browser. */
  readonly name: 'mock' | 'tauri';
  readonly isNative: boolean;
  generateSquama(params: SquamaParamsDto): Promise<MeshPayload>;
  /** A mesh with exactly `nTriangles` triangles, for transfer benchmarks. */
  syntheticMesh(nTriangles: number): Promise<MeshPayload>;
  getLaunchFlags(): Promise<LaunchFlags>;
  /** Persist a spike artifact (e.g. "viewport.png"). Names must match [A-Za-z0-9._-]+. */
  saveArtifact(name: string, bytes: Uint8Array): Promise<void>;
  /** Hand the machine-readable spike report to the host. The native host may exit afterwards. */
  finishSpike(report: object): Promise<void>;

  // ---- M1 Squama workflow.
  /** Layout and warnings of a recent generateSquama job, or null when it is unknown. */
  squamaJobInfo(jobId: number): Promise<SquamaJobInfo | null>;
  /** Cell-averaged target for the 2D view (same cache as generation). */
  squamaTarget(params: SquamaParamsDto): Promise<TargetGrid>;
  /** Native open dialog for a photo; null when cancelled or unavailable. */
  pickImage(): Promise<string | null>;
  imageInfo(path: string): Promise<ImageInfo>;
  imageThumbnail(path: string, maxPx: number, rotate: number, mirror: boolean): Promise<GrayImage>;
  /** Native save dialog; resolves to the chosen path, or null. */
  pickSavePath(defaultName: string): Promise<string | null>;
  exportSquama(dto: SquamaExportDto, onEvent: (e: ExportEvent) => void): Promise<ExportResult>;
  /** The tile plan an export with these settings would use; null when the engine cannot plan. */
  squamaTilePlan?(dto: SquamaExportDto): Promise<TilePlan | null>;
  /** Writes a PNG of the viewport; resolves to where it went. */
  savePreviewImage?(png: Uint8Array, suggestedName: string): Promise<string>;
  /** True when an export was running and has been asked to stop. */
  cancelExport(): Promise<boolean>;
  revealInFinder(path: string): Promise<void>;
}

export const ARTIFACT_NAME_RE = /^[A-Za-z0-9._-]+$/;

export function assertArtifactName(name: string): void {
  if (!ARTIFACT_NAME_RE.test(name) || name === '.' || name === '..') {
    throw new Error(`invalid artifact name ${JSON.stringify(name)}; use [A-Za-z0-9._-]+`);
  }
}
