// "Save preview image": a PNG of the canvas exactly as the next frame draws it.

import { getEngine } from '../engine';
import type { Engine } from '../engine/types';
import { viewportBus } from './viewportBus';
import { useViewportStore } from './viewportStore';

/** Engine extension for saving a preview PNG through the host (native file write). */
export type PreviewImageSaver = Required<Pick<Engine, 'savePreviewImage'>>;

function hasSaver(engine: Engine): engine is Engine & PreviewImageSaver {
  return typeof engine.savePreviewImage === 'function';
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

/**
 * Captures the canvas and saves it: through the engine when it can write files (the desktop
 * app's `save_preview_image`, which writes to ~/Pictures/AD3D), else as a browser download.
 * Resolves to where it went.
 */
export async function savePreviewImage(): Promise<string> {
  const bytes = await viewportBus.capturePng();
  const name = previewFileName();
  let where: string;
  const engine = await getEngine().catch(() => null);
  if (engine && hasSaver(engine)) where = await engine.savePreviewImage(bytes, name);
  else where = download(bytes, name);
  useViewportStore.getState().set({ lastSaved: where });
  return where;
}
