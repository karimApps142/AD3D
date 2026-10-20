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
}

export const ARTIFACT_NAME_RE = /^[A-Za-z0-9._-]+$/;

export function assertArtifactName(name: string): void {
  if (!ARTIFACT_NAME_RE.test(name) || name === '.' || name === '..') {
    throw new Error(`invalid artifact name ${JSON.stringify(name)}; use [A-Za-z0-9._-]+`);
  }
}
