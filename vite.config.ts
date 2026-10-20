/// <reference types="vitest/config" />
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// Tauri contract: the dev server sits on a fixed port (tauri.conf.json `devUrl`), never clears the
// terminal (so Rust logs stay visible), and exposes TAURI_* env vars to the frontend.
const host = process.env.TAURI_DEV_HOST;
const tauriDebug = !!process.env.TAURI_ENV_DEBUG;

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  envPrefix: ['VITE_', 'TAURI_'],
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    watch: {
      // The Rust workspace (target/ can hold GBs), docs and prototypes are not frontend sources.
      ignored: [
        '**/src-tauri/**',
        '**/target/**',
        '**/crates/**',
        '**/docs/**',
        '**/prototypes/**',
        '**/e2e/**',
      ],
    },
  },
  build: {
    // WKWebView on macOS (Safari 15+ engine); ES2021 keeps WebView2 on Windows happy too.
    target: ['es2021', 'safari15'],
    minify: tauriDebug ? false : 'oxc',
    sourcemap: tauriDebug,
    // three.js alone is ~700 kB minified; one chunk is fine for a local desktop app.
    chunkSizeWarningLimit: 1600,
  },
  worker: {
    format: 'es',
  },
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
  },
});
