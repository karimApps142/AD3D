// Native engine: the Rust core behind Tauri commands. Meshes come back as raw bytes
// (tauri::ipc::Response → ArrayBuffer) in the binary payload format and are parsed zero-copy.
//
// Command contract (implemented in src-tauri):
//   generate_squama  { params: SquamaParamsDto }      → Response(bytes)
//   synthetic_mesh   { nTriangles: u32 }              → Response(bytes)   (Rust arg n_triangles)
//   get_launch_flags                                  → { spike: bool }
//   save_artifact    raw body = file bytes,
//                    header "x-artifact-name"         → ()
//   finish_spike     { report: <JSON object> }        → ()   (the host may exit afterwards)
// M1 (src-tauri/src/squama_cmds.rs):
//   squama_job_info  { jobId }                         → SquamaJobInfo | null
//   squama_target    { params }                        → Response(AD3T bytes)
//   squama_image_open { path }                         → ImageInfo
//   squama_image_thumbnail { path, maxPx, rotate, mirror } → Response(thumbnail bytes)
//   export_squama    { export, onEvent: Channel }      → ExportResult
//   cancel_export                                      → bool
//   reveal_in_finder { path }                          → ()
// Dialogs come from tauri-plugin-dialog (permissions dialog:allow-open, dialog:allow-save).

import { Channel, invoke } from '@tauri-apps/api/core';
import { parseMeshPayload } from './payload';
import { useSquamaInfo } from './squamaInfo';
import { parseTarget, parseThumbnail } from './squamaWire';
import {
  assertArtifactName,
  type Engine,
  type ExportEvent,
  type ExportResult,
  type ImageInfo,
  type LaunchFlags,
  type MeshPayload,
  type SquamaJobInfo,
} from './types';

/** Image formats the core decodes (workspace `image` features). */
export const IMAGE_EXTENSIONS = ['png', 'jpg', 'jpeg', 'tif', 'tiff', 'webp', 'bmp'];

/** Tauri resolves raw responses to an ArrayBuffer; older IPC fallbacks deliver number[]. */
export function toBinary(response: unknown): ArrayBuffer | ArrayBufferView {
  if (response instanceof ArrayBuffer || ArrayBuffer.isView(response)) return response;
  if (Array.isArray(response)) return new Uint8Array(response as number[]).buffer;
  throw new Error(`expected a binary response, got ${typeof response}`);
}

export function createTauriEngine(): Engine {
  async function fetchMesh(command: string, args: Record<string, unknown>): Promise<MeshPayload> {
    const requestedAt = performance.now();
    const response = await invoke<unknown>(command, args);
    const receivedAt = performance.now();
    const payload = parseMeshPayload(toBinary(response));
    payload.timing = { requestedAt, receivedAt, parsedAt: performance.now() };
    return payload;
  }

  return {
    name: 'tauri',
    isNative: true,
    async generateSquama(params) {
      const payload = await fetchMesh('generate_squama', { params });
      // Fire and forget: the layout/warnings report must not delay the mesh.
      Promise.resolve()
        .then(() => invoke<SquamaJobInfo | null>('squama_job_info', { jobId: payload.jobId }))
        .then((info) => {
          if (info) useSquamaInfo.getState().publish(info);
        })
        .catch((err: unknown) => console.warn('[engine] squama_job_info failed', err));
      return payload;
    },
    syntheticMesh(nTriangles) {
      return fetchMesh('synthetic_mesh', { nTriangles });
    },
    async getLaunchFlags() {
      const flags = await invoke<Partial<LaunchFlags> | null>('get_launch_flags');
      return { spike: !!flags?.spike };
    },
    async saveArtifact(name, bytes) {
      assertArtifactName(name);
      await invoke('save_artifact', bytes, { headers: { 'x-artifact-name': name } });
    },
    async finishSpike(report) {
      await invoke('finish_spike', { report });
    },
    squamaJobInfo(jobId) {
      return invoke<SquamaJobInfo | null>('squama_job_info', { jobId });
    },
    async squamaTarget(params) {
      return parseTarget(toBinary(await invoke<unknown>('squama_target', { params })));
    },
    async pickImage() {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const picked = await open({
        multiple: false,
        directory: false,
        title: 'Open a photo',
        filters: [{ name: 'Images', extensions: IMAGE_EXTENSIONS }],
      });
      return typeof picked === 'string' ? picked : null;
    },
    imageInfo(path) {
      return invoke<ImageInfo>('squama_image_open', { path });
    },
    async imageThumbnail(path, maxPx, rotate, mirror) {
      const bytes = await invoke<unknown>('squama_image_thumbnail', { path, maxPx, rotate, mirror });
      return parseThumbnail(toBinary(bytes));
    },
    async pickSavePath(defaultName) {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const picked = await save({
        title: 'Export shadow plate',
        defaultPath: defaultName,
        filters: [
          { name: '3D model', extensions: ['3mf', 'stl'] },
        ],
      });
      return picked ?? null;
    },
    exportSquama(dto, onEvent: (e: ExportEvent) => void) {
      const channel = new Channel<ExportEvent>();
      channel.onmessage = onEvent;
      return invoke<ExportResult>('export_squama', { export: dto, onEvent: channel });
    },
    cancelExport() {
      return invoke<boolean>('cancel_export');
    },
    async revealInFinder(path) {
      await invoke('reveal_in_finder', { path });
    },
  };
}
