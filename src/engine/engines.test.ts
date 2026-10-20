import { beforeEach, describe, expect, it, vi } from 'vitest';
import { DEFAULT_PARAMS } from '../state/store';
import { createMockEngine } from './mockEngine';
import { encodeMeshPayload } from './payload';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args), Channel: class {} }));

describe('mock engine (inline, no Worker)', () => {
  const engine = createMockEngine({ useWorker: false });

  it('generates a parsed Squama payload that echoes the job id', async () => {
    const p = await engine.generateSquama({ ...DEFAULT_PARAMS, jobId: 17 });
    expect(p.jobId).toBe(17);
    expect(p.bboxMax[0]).toBe(300);
    expect(p.bboxMax[1]).toBe(378);
    expect(p.triangleCount).toBeGreaterThan(50 * 63 * 2);
    expect(p.timing).toBeDefined();
    expect(p.timing!.receivedAt).toBeGreaterThanOrEqual(p.timing!.requestedAt);
    expect(p.timing!.parsedAt).toBeGreaterThanOrEqual(p.timing!.receivedAt);
    expect(p.genMs).toBeGreaterThanOrEqual(0);
  });

  it('builds a synthetic mesh with the exact triangle count', async () => {
    const p = await engine.syntheticMesh(5001);
    expect(p.triangleCount).toBe(5001);
    expect(p.jobId).toBe(0);
  });

  it('is not native and has no launch flags', async () => {
    expect(engine.isNative).toBe(false);
    expect(await engine.getLaunchFlags()).toEqual({ spike: false });
  });

  it('rejects artifact names that could escape a directory', async () => {
    await expect(engine.saveArtifact('../x.png', new Uint8Array(1))).rejects.toThrow(/invalid artifact name/);
    await expect(engine.saveArtifact('viewport.png', new Uint8Array(3))).resolves.toBeUndefined();
  });
});

describe('Tauri engine', () => {
  beforeEach(() => invoke.mockReset());

  it('calls generate_squama with the DTO and parses the ArrayBuffer', async () => {
    const { createTauriEngine } = await import('./tauriEngine');
    const bytes = encodeMeshPayload({
      positions: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
      indices: new Uint32Array([0, 1, 2]),
      jobId: 9,
    });
    invoke.mockResolvedValueOnce(bytes);
    const params = { ...DEFAULT_PARAMS, jobId: 9 };
    const p = await createTauriEngine().generateSquama(params);
    expect(invoke).toHaveBeenCalledWith('generate_squama', { params });
    expect(p.jobId).toBe(9);
    expect(p.payloadZeroCopy).toBe(true);
    expect(p.positions.buffer).toBe(bytes);
  });

  it('accepts number[] responses from the postMessage IPC fallback', async () => {
    const { createTauriEngine } = await import('./tauriEngine');
    const bytes = encodeMeshPayload({ positions: new Float32Array(9), indices: new Uint32Array([0, 1, 2]) });
    invoke.mockResolvedValueOnce(Array.from(new Uint8Array(bytes)));
    const p = await createTauriEngine().syntheticMesh(1);
    expect(invoke).toHaveBeenCalledWith('synthetic_mesh', { nTriangles: 1 });
    expect(p.triangleCount).toBe(1);
  });

  it('sends artifacts as a raw body with a name header, and the report as JSON', async () => {
    const { createTauriEngine } = await import('./tauriEngine');
    invoke.mockResolvedValue(null);
    const engine = createTauriEngine();
    const png = new Uint8Array([137, 80, 78, 71]);
    await engine.saveArtifact('viewport.png', png);
    expect(invoke).toHaveBeenCalledWith('save_artifact', png, { headers: { 'x-artifact-name': 'viewport.png' } });
    await engine.finishSpike({ ok: 1 });
    expect(invoke).toHaveBeenCalledWith('finish_spike', { report: { ok: 1 } });
    invoke.mockResolvedValueOnce({ spike: true });
    expect(await engine.getLaunchFlags()).toEqual({ spike: true });
    expect(invoke).toHaveBeenLastCalledWith('get_launch_flags');
  });

  it('surfaces payload errors', async () => {
    const { createTauriEngine } = await import('./tauriEngine');
    invoke.mockResolvedValueOnce(new ArrayBuffer(10));
    await expect(createTauriEngine().syntheticMesh(1)).rejects.toThrow(/too short/);
  });
});
