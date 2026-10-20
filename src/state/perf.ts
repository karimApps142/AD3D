// Mutable performance counters, written from hot paths (render loop, mesh updates) without React,
// and read by the stats overlay a few times per second and by the spike harness.

export class TimeRing {
  private readonly data: Float64Array;
  private head = 0;
  private size = 0;

  constructor(capacity = 1024) {
    this.data = new Float64Array(capacity);
  }

  push(t: number): void {
    this.data[this.head] = t;
    this.head = (this.head + 1) % this.data.length;
    if (this.size < this.data.length) this.size++;
  }

  /** Number of stored timestamps >= since. */
  countSince(since: number): number {
    let n = 0;
    for (let i = 0; i < this.size; i++) {
      const idx = (this.head - 1 - i + this.data.length) % this.data.length;
      if (this.data[idx] < since) break;
      n++;
    }
    return n;
  }

  clear(): void {
    this.head = 0;
    this.size = 0;
  }
}

export interface FrameRecord {
  /** performance.now() right after gl.render returned. */
  t: number;
  /** CPU time spent in gl.render (shadow pass + main pass submission). */
  cpuMs: number;
  /** Version of the mesh drawn in this frame. */
  meshVersion: number;
  /** True when this frame is the first to draw a new mesh. */
  meshChanged: boolean;
  /** Input → this frame, for the first frame of a mesh produced by an input. */
  latencyMs?: number;
}

export interface LastNumbers {
  genMs: number;
  encodeMs: number;
  ipcMs: number;
  parseMs: number;
  geometryMs: number;
  geometryMode: string;
  renderMs: number;
  latencyMs: number;
  triangles: number;
  vertices: number;
  bytes: number;
  jobId: number;
}

export const perf = {
  frames: new TimeRing(1024),
  updates: new TimeRing(1024),
  last: {
    genMs: NaN,
    encodeMs: NaN,
    ipcMs: NaN,
    parseMs: NaN,
    geometryMs: NaN,
    geometryMode: '',
    renderMs: NaN,
    latencyMs: NaN,
    triangles: 0,
    vertices: 0,
    bytes: 0,
    jobId: 0,
  } as LastNumbers,
  frameListeners: new Set<(frame: FrameRecord) => void>(),
};

export function recordFrame(frame: FrameRecord): void {
  perf.frames.push(frame.t);
  perf.last.renderMs = frame.cpuMs;
  if (frame.meshChanged) {
    perf.updates.push(frame.t);
    if (frame.latencyMs !== undefined) perf.last.latencyMs = frame.latencyMs;
  }
  for (const listener of perf.frameListeners) listener(frame);
}

/** Events per second over the last second. */
export function ratePerSecond(ring: TimeRing, now = performance.now()): number {
  return ring.countSince(now - 1000);
}
