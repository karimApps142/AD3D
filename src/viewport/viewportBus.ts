// A small registry that lets non-React code (the spike harness, shortcuts) talk to the live
// viewport: wait for frames, force a GPU sync, read pixels, capture a PNG. Hooks run right after
// gl.render in the same task, so the drawing buffer is still valid without preserveDrawingBuffer.

import * as THREE from 'three';
import type { FrameRecord } from '../state/perf';
import type { GeometryMode } from './geometry';
import type { CellPlanes } from './heightfield';

export interface FrameResult extends FrameRecord {
  /** performance.now() after the forced GPU sync (gpuSync only). */
  syncedAt?: number;
  /** Time the sync itself blocked (gpuSync only). */
  syncMs?: number;
}

export interface PixelReadback {
  width: number;
  height: number;
  /** RGBA, bottom row first (WebGL order). */
  data: Uint8Array;
}

export interface AppliedMesh {
  version: number;
  inputAt?: number;
  geometryMs: number;
  mode: GeometryMode;
}

type AfterRenderHook = (gl: THREE.WebGLRenderer, frame: FrameRecord) => void;

interface Handles {
  gl: THREE.WebGLRenderer;
  invalidate: () => void;
  /** Current r3f camera and default controls. */
  getView: () => { camera: THREE.Camera; controls: unknown };
}

class ViewportBus {
  private handles: Handles | null = null;
  private hooks: AfterRenderHook[] = [];
  /** The scene's directional key light (set by LightRig). */
  light: THREE.DirectionalLight | null = null;
  /** The light currently rendering: the key light, or the point lamp (a SpotLight). */
  activeLight: THREE.DirectionalLight | THREE.SpotLight | null = null;
  /** Per-cell planes of the displayed plate (analytic and relief modes; set by PlateMesh). */
  planes: CellPlanes | null = null;
  /** Time the last buildCellPlanes took (ms). */
  planesMs = 0;
  /** Render mode the plate is drawn with. */
  renderMode: 'analytic' | 'shadowMap' | 'relief' = 'analytic';
  /** World-space bounds of the displayed model (set by PlateMesh). */
  readonly modelBox = new THREE.Box3(new THREE.Vector3(0, 0, 0), new THREE.Vector3(300, 378, 4));
  /** Bumped whenever modelBox changes. */
  modelBoxVersion = 0;
  /** The last mesh PlateMesh put into the scene. */
  applied: AppliedMesh | null = null;

  attach(handles: Handles): void {
    this.handles = handles;
  }

  detach(gl: THREE.WebGLRenderer): void {
    if (this.handles?.gl === gl) this.handles = null;
  }

  get ready(): boolean {
    return this.handles !== null;
  }

  get gl(): THREE.WebGLRenderer | null {
    return this.handles?.gl ?? null;
  }

  invalidate(): void {
    this.handles?.invalidate();
  }

  /** The live camera and OrbitControls (null before the canvas mounts). */
  view(): { camera: THREE.Camera; controls: unknown } | null {
    return this.handles?.getView() ?? null;
  }

  setModelBox(box: THREE.Box3): void {
    if (!box.equals(this.modelBox)) {
      this.modelBox.copy(box);
      this.modelBoxVersion++;
    }
  }

  /** Called by the render driver after every frame. */
  runAfterRender(gl: THREE.WebGLRenderer, frame: FrameRecord): void {
    if (this.hooks.length === 0) return;
    const hooks = this.hooks;
    this.hooks = [];
    for (const hook of hooks) {
      try {
        hook(gl, frame);
      } catch (err) {
        console.error('[viewport] after-render hook failed', err);
      }
    }
  }

  private afterNextRender<T>(hook: (gl: THREE.WebGLRenderer, frame: FrameRecord) => T | Promise<T>): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      this.hooks.push((gl, frame) => {
        try {
          Promise.resolve(hook(gl, frame)).then(resolve, reject);
        } catch (err) {
          reject(err);
        }
      });
      this.invalidate();
    });
  }

  /** Resolves after the next rendered frame; with gpuSync also after the GPU finished it. */
  nextFrame(options: { gpuSync?: boolean } = {}): Promise<FrameResult> {
    return this.afterNextRender((gl, frame) => {
      if (!options.gpuSync) return { ...frame };
      const t0 = performance.now();
      gpuSync(gl);
      const t1 = performance.now();
      return { ...frame, syncedAt: t1, syncMs: t1 - t0 };
    });
  }

  /** Reads the whole drawing buffer right after the next frame. */
  readPixels(): Promise<PixelReadback> {
    return this.afterNextRender((gl) => {
      const ctx = gl.getContext();
      const width = ctx.drawingBufferWidth;
      const height = ctx.drawingBufferHeight;
      const data = new Uint8Array(width * height * 4);
      ctx.readPixels(0, 0, width, height, ctx.RGBA, ctx.UNSIGNED_BYTE, data);
      return { width, height, data };
    });
  }

  /** PNG of the canvas as drawn by the next frame. */
  capturePng(): Promise<Uint8Array> {
    return this.afterNextRender(
      (gl) =>
        new Promise<Uint8Array>((resolve, reject) => {
          gl.domElement.toBlob((blob) => {
            if (!blob) return reject(new Error('canvas.toBlob returned null'));
            blob.arrayBuffer().then((buf) => resolve(new Uint8Array(buf)), reject);
          }, 'image/png');
        }),
    );
  }
}

/** Blocks until the GPU has finished all queued work: a 1-pixel readPixels from the canvas. */
export function gpuSync(gl: THREE.WebGLRenderer): void {
  const ctx = gl.getContext();
  const px = new Uint8Array(4);
  ctx.readPixels(0, 0, 1, 1, ctx.RGBA, ctx.UNSIGNED_BYTE, px);
}

export const viewportBus = new ViewportBus();
