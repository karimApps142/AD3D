// Handles on `window.__AD3D_VIEWPORT__` for the browser checks (e2e/viewport-preview.mjs) and for
// poking at the preview from the devtools console. Display state and read access only; nothing
// here changes the model.

import { useAppStore } from '../state/store';
import { traceShadow } from './heightfield';
import { savePreviewImage } from './savePreview';
import { viewportBus } from './viewportBus';
import { useViewportStore } from './viewportStore';

export interface ViewportHooks {
  viewport: typeof useViewportStore;
  app: typeof useAppStore;
  bus: typeof viewportBus;
  traceShadow: typeof traceShadow;
  savePreviewImage: typeof savePreviewImage;
}

declare global {
  interface Window {
    __AD3D_VIEWPORT__?: ViewportHooks;
  }
}

export function exposeViewportHooks(): void {
  if (typeof window === 'undefined') return;
  window.__AD3D_VIEWPORT__ = { viewport: useViewportStore, app: useAppStore, bus: viewportBus, traceShadow, savePreviewImage };
}
