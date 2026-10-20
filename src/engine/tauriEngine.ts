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

import { invoke } from '@tauri-apps/api/core';
import { parseMeshPayload } from './payload';
import { assertArtifactName, type Engine, type LaunchFlags, type MeshPayload } from './types';

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
    generateSquama(params) {
      return fetchMesh('generate_squama', { params });
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
  };
}
