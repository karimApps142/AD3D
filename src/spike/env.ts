// Environment block of the spike report: browser, display and GPU.

import { version as reactVersion } from 'react';
import * as THREE from 'three';
import { viewportBus } from '../viewport/viewportBus';

export interface SpikeEnv {
  userAgent: string;
  platform: string;
  hardwareConcurrency: number;
  devicePixelRatio: number;
  canvasSize: { cssWidth: number; cssHeight: number; width: number; height: number } | null;
  webglVersion: string | null;
  webglRenderer: string | null;
  maskedRenderer: string | null;
  webglVendor: string | null;
  unmaskedRenderer: string | null;
  unmaskedVendor: string | null;
  maxTextureSize: number | null;
  antialias: boolean | null;
  visibilityState: DocumentVisibilityState;
  three: string;
  react: string;
}

export function collectEnv(): SpikeEnv {
  const gl = viewportBus.gl;
  const canvas = gl?.domElement ?? null;
  const ctx = gl?.getContext() ?? null;
  let unmaskedRenderer: string | null = null;
  let unmaskedVendor: string | null = null;
  if (ctx) {
    const ext = ctx.getExtension('WEBGL_debug_renderer_info');
    if (ext) {
      unmaskedRenderer = String(ctx.getParameter(ext.UNMASKED_RENDERER_WEBGL));
      unmaskedVendor = String(ctx.getParameter(ext.UNMASKED_VENDOR_WEBGL));
    }
  }
  return {
    userAgent: navigator.userAgent,
    platform: navigator.platform,
    hardwareConcurrency: navigator.hardwareConcurrency,
    devicePixelRatio: window.devicePixelRatio,
    canvasSize: canvas
      ? { cssWidth: canvas.clientWidth, cssHeight: canvas.clientHeight, width: canvas.width, height: canvas.height }
      : null,
    webglVersion: ctx ? String(ctx.getParameter(ctx.VERSION)) : null,
    // UNMASKED_RENDERER_WEBGL when the browser exposes it (the real GPU), else the masked string.
    webglRenderer: unmaskedRenderer ?? (ctx ? String(ctx.getParameter(ctx.RENDERER)) : null),
    maskedRenderer: ctx ? String(ctx.getParameter(ctx.RENDERER)) : null,
    webglVendor: ctx ? String(ctx.getParameter(ctx.VENDOR)) : null,
    unmaskedRenderer,
    unmaskedVendor,
    maxTextureSize: ctx ? Number(ctx.getParameter(ctx.MAX_TEXTURE_SIZE)) : null,
    antialias: ctx?.getContextAttributes()?.antialias ?? null,
    visibilityState: document.visibilityState,
    three: THREE.REVISION,
    react: reactVersion,
  };
}
