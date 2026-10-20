#!/usr/bin/env node
// Headless browser check of the Squama preview viewport (browser mock engine).
//
// Drives the display modes through window.__AD3D_VIEWPORT__, saves a PNG of the canvas for each,
// and checks the pixels: the analytic shader casts shadows (and they go when shadows are off),
// ambient light lowers the contrast, the shadow-map fallback and the relief colour map render,
// the point lamp changes the picture, the front viewer fills the view, tile labels appear, and no
// shader failed to compile.
//
//   pnpm dev --port 1487 --strictPort      # in another terminal
//   node e2e/viewport-preview.mjs --url http://localhost:1487/ [--out-dir dir] [--swiftshader]
//
// Exit code 0 when every check passes. Without --chrome it uses the newest Chrome for Testing in
// ~/.cache/puppeteer (or CHROME_PATH).

import { existsSync, readdirSync } from 'node:fs';
import { mkdir, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { parseArgs } from 'node:util';
import puppeteer from 'puppeteer';

const { values: args } = parseArgs({
  options: {
    url: { type: 'string', default: 'http://localhost:1420/' },
    'out-dir': { type: 'string', default: path.join(os.tmpdir(), 'ad3d-viewport-check') },
    chrome: { type: 'string' },
    swiftshader: { type: 'boolean', default: false },
    width: { type: 'string', default: '1280' },
    height: { type: 'string', default: '860' },
    timeout: { type: 'string', default: '120' },
  },
});

function findCachedChrome() {
  const root = path.join(os.homedir(), '.cache', 'puppeteer', 'chrome');
  if (!existsSync(root)) return undefined;
  const key = (d) => (d.match(/\d+/g) ?? []).map(Number);
  const dirs = readdirSync(root, { withFileTypes: true })
    .filter((d) => d.isDirectory())
    .map((d) => d.name)
    .sort((a, b) => {
      const ka = key(a), kb = key(b);
      for (let i = 0; i < Math.max(ka.length, kb.length); i++) {
        const d = (kb[i] ?? 0) - (ka[i] ?? 0);
        if (d) return d;
      }
      return 0;
    });
  for (const dir of dirs) {
    for (const sub of ['chrome-mac-x64', 'chrome-mac-arm64', 'chrome-mac']) {
      const exe = path.join(root, dir, sub, 'Google Chrome for Testing.app', 'Contents', 'MacOS', 'Google Chrome for Testing');
      if (existsSync(exe)) return exe;
    }
    for (const sub of ['chrome-linux64/chrome', 'chrome-win64/chrome.exe']) {
      const exe = path.join(root, dir, sub);
      if (existsSync(exe)) return exe;
    }
  }
  return undefined;
}

const executablePath = args.chrome ?? process.env.CHROME_PATH ?? findCachedChrome();
if (!executablePath) {
  console.error('No Chrome found: pass --chrome or set CHROME_PATH');
  process.exit(2);
}
const outDir = path.resolve(args['out-dir']);
await mkdir(outDir, { recursive: true });

const launchArgs = ['--no-first-run', '--no-default-browser-check', '--ignore-gpu-blocklist', '--enable-gpu-rasterization'];
if (args.swiftshader) launchArgs.push('--use-angle=swiftshader', '--enable-unsafe-swiftshader');

const browser = await puppeteer.launch({
  executablePath,
  headless: true,
  args: launchArgs,
  defaultViewport: { width: Number(args.width), height: Number(args.height), deviceScaleFactor: 1 },
});

const results = [];
function check(name, ok, detail) {
  results.push({ name, ok, detail });
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? `  (${detail})` : ''}`);
}

let exitCode = 0;
try {
  const page = await browser.newPage();
  const shaderErrors = [];
  page.on('console', (msg) => {
    const text = msg.text();
    if (/Shader Error|WebGLProgram|VALIDATE_STATUS|shader/i.test(text) && msg.type() === 'error') shaderErrors.push(text.slice(0, 2000));
    if (msg.type() === 'error') console.log(`[page:error] ${text.slice(0, 600)}`);
  });
  page.on('pageerror', (err) => {
    shaderErrors.push(`pageerror: ${err.message}`);
    console.log(`[page:exception] ${err.message}`);
  });

  await page.goto(args.url, { waitUntil: 'load', timeout: 60_000 });
  await page.waitForFunction(
    () => {
      const h = window.__AD3D_VIEWPORT__;
      return !!h && h.bus.ready && h.bus.applied !== null && h.bus.planes !== null && h.bus.planes.validCells > 0;
    },
    { timeout: Number(args.timeout) * 1000, polling: 200 },
  );
  check('cell planes built from the plate', true, await page.evaluate(() => {
    const p = window.__AD3D_VIEWPORT__.bus.planes;
    return `${p.cols}×${p.rows} cells, ${window.__AD3D_VIEWPORT__.bus.planesMs.toFixed(1)} ms`;
  }));

  // Helpers inside the page: apply a patch, wait two frames, then measure the plate rectangle.
  await page.evaluate(() => {
    const h = window.__AD3D_VIEWPORT__;
    window.__vpSet = async (vp, view, shadowIntensity) => {
      if (vp) h.viewport.getState().set(vp);
      if (view) h.app.getState().setView(view);
      for (const l of [h.bus.light, h.bus.activeLight]) if (l && shadowIntensity !== undefined) l.shadow.intensity = shadowIntensity;
      await h.bus.nextFrame();
      await new Promise((r) => setTimeout(r, 150));
      await h.bus.nextFrame();
    };
    window.__vpMeasure = async () => {
      const px = await h.bus.readPixels();
      const { camera } = h.bus.view();
      const box = h.bus.modelBox;
      // Plate rectangle on screen (front face corners), shrunk 5% to stay off the edges.
      const proj = (x, y, z) => {
        const v = { x, y, z };
        const e = camera.matrixWorldInverse.elements;
        const p = camera.projectionMatrix.elements;
        const cx = e[0] * v.x + e[4] * v.y + e[8] * v.z + e[12];
        const cy = e[1] * v.x + e[5] * v.y + e[9] * v.z + e[13];
        const cz = e[2] * v.x + e[6] * v.y + e[10] * v.z + e[14];
        const cw = e[3] * v.x + e[7] * v.y + e[11] * v.z + e[15];
        const X = p[0] * cx + p[4] * cy + p[8] * cz + p[12] * cw;
        const Y = p[1] * cx + p[5] * cy + p[9] * cz + p[13] * cw;
        const W = p[3] * cx + p[7] * cy + p[11] * cz + p[15] * cw;
        return [((X / W + 1) / 2) * px.width, ((Y / W + 1) / 2) * px.height];
      };
      const a = proj(box.min.x, box.min.y, box.max.z);
      const b = proj(box.max.x, box.max.y, box.max.z);
      const x0 = Math.ceil(Math.min(a[0], b[0])), x1 = Math.floor(Math.max(a[0], b[0]));
      const y0 = Math.ceil(Math.min(a[1], b[1])), y1 = Math.floor(Math.max(a[1], b[1]));
      const mx = Math.round((x1 - x0) * 0.05), my = Math.round((y1 - y0) * 0.05);
      const lum = [];
      let chroma = 0;
      for (let y = Math.max(0, y0 + my); y < Math.min(px.height, y1 - my); y += 2) {
        for (let x = Math.max(0, x0 + mx); x < Math.min(px.width, x1 - mx); x += 2) {
          const o = (y * px.width + x) * 4;
          const r = px.data[o], g = px.data[o + 1], bl = px.data[o + 2];
          lum.push(0.2126 * r + 0.7152 * g + 0.0722 * bl);
          chroma += Math.abs(r - g) + Math.abs(g - bl);
        }
      }
      lum.sort((p, q) => p - q);
      const q = (f) => lum[Math.min(lum.length - 1, Math.floor(f * lum.length))];
      const mean = lum.reduce((s, v) => s + v, 0) / lum.length;
      const max = q(0.995);
      return {
        rect: [x0, y0, x1, y1],
        coverage: ((x1 - x0) * (y1 - y0)) / (px.width * px.height),
        n: lum.length,
        mean,
        p05: q(0.05),
        p95: q(0.95),
        dark: lum.filter((v) => v < 0.25 * max).length / lum.length,
        chroma: chroma / lum.length,
        // Coarse fingerprint for "did the picture change": 32×32 block means.
        blocks: (() => {
          const out = [];
          const bw = (x1 - x0) / 32, bh = (y1 - y0) / 32;
          for (let j = 0; j < 32; j++) for (let i = 0; i < 32; i++) {
            const x = Math.floor(x0 + (i + 0.5) * bw), y = Math.floor(y0 + (j + 0.5) * bh);
            const o = (Math.min(px.height - 1, Math.max(0, y)) * px.width + Math.min(px.width - 1, Math.max(0, x))) * 4;
            out.push(px.data[o] + px.data[o + 1] + px.data[o + 2]);
          }
          return out;
        })(),
      };
    };
    window.__vpPng = async () => {
      const bytes = await h.bus.capturePng();
      let s = '';
      for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode.apply(null, Array.from(bytes.subarray(i, i + 0x8000)));
      return btoa(s);
    };
  });

  const shot = async (name) => {
    const b64 = await page.evaluate(() => window.__vpPng());
    const file = path.join(outDir, `viewport-${name}.png`);
    await writeFile(file, Buffer.from(b64, 'base64'));
    return file;
  };
  const measure = () => page.evaluate(() => window.__vpMeasure());
  const set = (vp, view, shadow) => page.evaluate((a, b, c) => window.__vpSet(a, b, c), vp, view, shadow);
  const changed = (a, b) => a.blocks.filter((v, i) => Math.abs(v - b.blocks[i]) > 24).length / a.blocks.length;

  // 1. Front viewer, analytic shadows, design light (180°, 5°).
  await set({ frontView: true, renderMode: 'analytic', lightKind: 'directional', showTiles: false }, { followDesignLight: true, ambient: 0.02 }, 1);
  const front = await measure();
  await shot('front-analytic');
  check('front viewer fills the view with the plate', front.coverage > 0.45, `coverage ${(front.coverage * 100).toFixed(0)}%`);
  check('analytic preview shows an image (contrast)', front.p95 - front.p05 > 40, `p05 ${front.p05.toFixed(0)}, p95 ${front.p95.toFixed(0)}`);

  // 2. Same frame without cast shadows: fewer dark pixels.
  await set(null, null, 0);
  const noShadow = await measure();
  await shot('front-analytic-noshadow');
  await set(null, null, 1);
  check('analytic cast shadows darken pixels', front.mean < noShadow.mean - 1 && front.dark >= noShadow.dark && changed(front, noShadow) > 0.005,
    `mean ${front.mean.toFixed(1)} vs ${noShadow.mean.toFixed(1)} without, dark ${(front.dark * 100).toFixed(1)}% vs ${(noShadow.dark * 100).toFixed(1)}%, ${(changed(front, noShadow) * 100).toFixed(1)}% blocks changed`);

  // 3. Ambient room light washes the image out.
  await set(null, { ambient: 0.5 });
  const washed = await measure();
  await shot('front-analytic-ambient50');
  await set(null, { ambient: 0.02 });
  const ratio = (m) => (m.p95 + 1) / (m.p05 + 1);
  check('ambient light lowers the contrast', ratio(washed) < ratio(front) * 0.8, `p95/p05 ${ratio(front).toFixed(2)} → ${ratio(washed).toFixed(2)}`);

  // 4. Shadow-map fallback renders.
  await set({ renderMode: 'shadowMap' });
  const sm = await measure();
  await shot('front-shadowmap');
  check('shadow-map fallback renders', sm.p95 > 60 && sm.p95 - sm.p05 > 20, `p05 ${sm.p05.toFixed(0)}, p95 ${sm.p95.toFixed(0)}`);

  // 5. Relief colour map.
  await set({ renderMode: 'relief' });
  const relief = await measure();
  await shot('front-relief');
  check('relief colour map is colourful', relief.chroma > 40, `mean chroma ${relief.chroma.toFixed(0)}`);

  // 6. Point lamp at 400 mm.
  await set({ renderMode: 'analytic', lightKind: 'point', lampDistanceMm: 400 }, { followDesignLight: false, previewAzimuthDeg: 180, previewElevationDeg: 5 });
  const lamp = await measure();
  await shot('front-lamp400');
  check('point lamp changes the picture', changed(front, lamp) > 0.02, `${(changed(front, lamp) * 100).toFixed(1)}% blocks changed`);
  check('point lamp lights the plate', lamp.p95 > 60, `p95 ${lamp.p95.toFixed(0)}`);

  // 7. Tile overlay labels.
  await set({ lightKind: 'directional', showTiles: true }, { followDesignLight: true });
  await page.waitForSelector('.tile-label', { timeout: 5000 }).catch(() => {});
  const labels = await page.$$eval('.tile-label strong', (els) => els.map((e) => e.textContent));
  await page.screenshot({ path: path.join(outDir, 'viewport-tiles-window.png') });
  check('tile overlay labels every part', labels.length >= 2, labels.join(', '));

  // 8. Back to the perspective view.
  await set({ frontView: false, showTiles: false });
  const persp = await shot('perspective-analytic');
  await page.screenshot({ path: path.join(outDir, 'viewport-window.png') });
  check('perspective view renders', true, persp);

  check('no shader or page errors', shaderErrors.length === 0, shaderErrors.join(' | ').slice(0, 1500));
  await writeFile(path.join(outDir, 'viewport-check.json'), JSON.stringify(results, null, 2));
  console.log(`screenshots in ${outDir}`);
  if (results.some((r) => !r.ok)) exitCode = 1;
} catch (err) {
  console.error('viewport check failed:', err);
  exitCode = 1;
} finally {
  await browser.close();
}
process.exit(exitCode);
