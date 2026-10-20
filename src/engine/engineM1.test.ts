import { beforeEach, describe, expect, it, vi } from 'vitest';
import { DEFAULT_PARAMS } from '../state/store';
import { createMockEngine, mockJobInfo } from './mockEngine';
import { encodeMeshPayload } from './payload';
import { useSquamaInfo } from './squamaInfo';
import { encodeTarget, encodeThumbnail } from './squamaWire';
import type { ExportEvent, SquamaExportDto, SquamaJobInfo } from './types';

const invoke = vi.fn();
const open = vi.fn();
const save = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invoke(...args),
  Channel: class {
    onmessage: (m: unknown) => void = () => {};
  },
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: (...a: unknown[]) => open(...a), save: (...a: unknown[]) => save(...a) }));

interface FakeChannel {
  onmessage: (m: ExportEvent) => void;
}

const info: SquamaJobInfo = {
  jobId: 5,
  quality: 'final',
  layout: { kind: 'square', nx: 50, ny: 63, wMm: 300, hMm: 378, cellMm: 6 },
  warnings: [{ code: 'small_cell', message: 'x' }],
  genMs: 3,
  triangles: 100,
  validated: true,
};

describe('Tauri engine, M1 commands', () => {
  beforeEach(() => {
    invoke.mockReset();
    open.mockReset();
    save.mockReset();
    useSquamaInfo.setState({ info: null });
  });

  it('passes M1 DTO fields through and publishes the job info afterwards', async () => {
    const { createTauriEngine } = await import('./tauriEngine');
    const bytes = encodeMeshPayload({ positions: new Float32Array(9), indices: new Uint32Array([0, 1, 2]), jobId: 5 });
    invoke.mockImplementation(async (cmd: string) => (cmd === 'generate_squama' ? bytes : info));
    const params = {
      ...DEFAULT_PARAMS,
      jobId: 5,
      quality: 'final' as const,
      layout: { kind: 'hex' as const, orientation: 'pointy' as const },
      imageOpts: { gamma: 1.4, rotate: 90 as const },
    };
    await createTauriEngine().generateSquama(params);
    expect(invoke).toHaveBeenCalledWith('generate_squama', { params });
    await vi.waitFor(() => expect(useSquamaInfo.getState().info).toEqual(info));
    expect(invoke).toHaveBeenCalledWith('squama_job_info', { jobId: 5 });
  });

  it('decodes targets and thumbnails, and wraps the dialogs', async () => {
    const { createTauriEngine } = await import('./tauriEngine');
    const e = createTauriEngine();
    invoke.mockResolvedValueOnce(encodeTarget({ nx: 1, ny: 2, g: Float32Array.from([0.5, 1]) }));
    expect((await e.squamaTarget({ ...DEFAULT_PARAMS, jobId: 0 })).g).toEqual(Float32Array.from([0.5, 1]));
    invoke.mockResolvedValueOnce(encodeThumbnail({ width: 1, height: 1, data: Uint8Array.from([7]) }));
    expect(await e.imageThumbnail('/p.jpg', 64, 90, true)).toEqual({ width: 1, height: 1, data: Uint8Array.from([7]) });
    expect(invoke).toHaveBeenLastCalledWith('squama_image_thumbnail', { path: '/p.jpg', maxPx: 64, rotate: 90, mirror: true });
    open.mockResolvedValueOnce('/photos/a.png');
    expect(await e.pickImage()).toBe('/photos/a.png');
    expect(open.mock.calls[0][0]).toMatchObject({ multiple: false, directory: false });
    open.mockResolvedValueOnce(null);
    expect(await e.pickImage()).toBeNull();
    save.mockResolvedValueOnce('/out/plate.3mf');
    expect(await e.pickSavePath('plate.3mf')).toBe('/out/plate.3mf');
    expect(save.mock.calls[0][0]).toMatchObject({ defaultPath: 'plate.3mf' });
  });

  it('streams export events over a channel, and cancels and reveals', async () => {
    const { createTauriEngine } = await import('./tauriEngine');
    const e = createTauriEngine();
    const result = { outDir: '/o', files: ['/o/a.stl'], parts: [], tiles: null, warnings: [], elapsedMs: 1 };
    invoke.mockImplementationOnce(async (_cmd: string, args: { onEvent: FakeChannel }) => {
      args.onEvent.onmessage({ kind: 'started', totalParts: 1 });
      return result;
    });
    const events: ExportEvent[] = [];
    const dto = { params: { ...DEFAULT_PARAMS, jobId: 0 }, outDir: '/o', formats: ['stl'], orientation: 'flat' } as SquamaExportDto;
    expect(await e.exportSquama(dto, (ev) => events.push(ev))).toEqual(result);
    expect(events).toEqual([{ kind: 'started', totalParts: 1 }]);
    expect(invoke.mock.calls[0][0]).toBe('export_squama');
    expect(invoke.mock.calls[0][1].export).toBe(dto);
    invoke.mockResolvedValueOnce(true);
    expect(await e.cancelExport()).toBe(true);
    expect(invoke).toHaveBeenLastCalledWith('cancel_export');
    invoke.mockResolvedValueOnce(null);
    await e.revealInFinder('/o/a.stl');
    expect(invoke).toHaveBeenLastCalledWith('reveal_in_finder', { path: '/o/a.stl' });
  });
});

describe('mock engine, M1 methods', () => {
  const engine = createMockEngine({ useWorker: false });

  it('reports layout and warnings for each job', async () => {
    useSquamaInfo.setState({ info: null });
    await engine.generateSquama({ ...DEFAULT_PARAMS, cellMm: 2.5, jobId: 11 });
    const got = await engine.squamaJobInfo(11);
    expect(got?.layout.nx).toBe(120);
    expect(got?.warnings.map((w) => w.code)).toContain('small_cell');
    expect(useSquamaInfo.getState().info?.jobId).toBe(11);
    expect(mockJobInfo({ ...DEFAULT_PARAMS, jobId: 1 }).layout).toMatchObject({ nx: 50, ny: 63 });
  });

  it('serves the synthetic image and refuses native-only work', async () => {
    expect((await engine.squamaTarget({ ...DEFAULT_PARAMS, jobId: 0 })).nx).toBe(50);
    expect((await engine.imageThumbnail('synthetic', 64, 0, false)).height).toBeLessThanOrEqual(64);
    expect(await engine.pickImage()).toBeNull();
    await expect(engine.imageInfo('/x.jpg')).rejects.toThrow(/desktop app/);
    await expect(engine.exportSquama({} as SquamaExportDto, () => {})).rejects.toThrow(/desktop app/);
    expect(await engine.cancelExport()).toBe(false);
  });

  it('never lets an older report replace a newer one', () => {
    useSquamaInfo.setState({ info: null });
    useSquamaInfo.getState().publish({ ...info, jobId: 9 });
    useSquamaInfo.getState().publish({ ...info, jobId: 8 });
    expect(useSquamaInfo.getState().info?.jobId).toBe(9);
  });
});
