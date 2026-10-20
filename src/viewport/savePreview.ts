// "Save preview image": a PNG of the canvas exactly as the next frame draws it.

import { invoke } from '@tauri-apps/api/core';
import { getEngine } from '../engine';
import type { Engine } from '../engine/types';
import { viewportBus } from './viewportBus';
import { useViewportStore } from './viewportStore';

/** Engine extension for saving a preview PNG through the host (native file write). */
export interface PreviewImageSaver {
  /** Writes the PNG and resolves to the path it was written to. */
  savePreviewImage(bytes: Uint8Array, suggestedName: string): Promise<string>;
}

function hasSaver(engine: Engine): engine is Engine & PreviewImageSaver {
  return typeof (engine as Partial<PreviewImageSaver>).savePreviewImage === 'function';
}

export function previewFileName(now = new Date()): string {
  const p = (n: number) => String(n).padStart(2, '0');
  return `ad3d-preview-${now.getFullYear()}${p(now.getMonth() + 1)}${p(now.getDate())}-${p(now.getHours())}${p(now.getMinutes())}${p(now.getSeconds())}.png`;
}

/** Browser fallback: trigger a download of the PNG. */
function download(bytes: Uint8Array, name: string): string {
  const blob = new Blob([bytes as Uint8Array<ArrayBuffer>], { type: 'image/png' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
  return name;
}

/** Native path: the `save_preview_image` Tauri command writes the PNG and returns its path. */
async function saveNative(bytes: Uint8Array, name: string): Promise<string> {
  return invoke<string>('save_preview_image', bytes, { headers: { 'x-file-name': name } });
}

/**
 * Captures the canvas and saves it: through the engine when it can write files, else through the
 * `save_preview_image` command in the desktop app, else as a browser download. Resolves to where
 * it went.
 *
 * MERGE: wire to engine (types.ts Engine + tauriEngine.ts) — move `saveNative` into the engine as
 * `savePreviewImage(bytes, suggestedName): Promise<string>`; the command already exists in
 * src-tauri/src/lib.rs (`save_preview_image`, raw PNG body, header "x-file-name", writes to
 * ~/Pictures/AD3D). `hasSaver` then picks it up and `saveNative` can go.
 */
export async function savePreviewImage(): Promise<string> {
  const bytes = await viewportBus.capturePng();
  const name = previewFileName();
  let where: string;
  const engine = await getEngine().catch(() => null);
  if (engine && hasSaver(engine)) where = await engine.savePreviewImage(bytes, name);
  else if (engine?.isNative) where = await saveNative(bytes, name);
  else where = download(bytes, name);
  useViewportStore.getState().set({ lastSaved: where });
  return where;
}
