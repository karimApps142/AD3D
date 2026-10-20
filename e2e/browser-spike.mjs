#!/usr/bin/env node
// Headless browser check of the frontend spike harness (mock engine).
//
// Loads the app with ?spike=1 in Chrome for Testing, waits for window.__SPIKE_REPORT__, then saves
// a screenshot, the harness's viewport.png artifact and the report JSON.
//
//   pnpm dev                       # in another terminal (port 1420)
//   node e2e/browser-spike.mjs --out /tmp/shot.png [--artifact-dir dir] [--swiftshader]
//
// Options: --url, --out, --artifact-dir, --chrome <executable>, --swiftshader, --dpr, --width,
// --height, --timeout <seconds>. Without --chrome it uses the newest Chrome for Testing in
// ~/.cache/puppeteer (or CHROME_PATH).

import { existsSync, readdirSync } from 'node:fs';
import { mkdir, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { parseArgs } from 'node:util';
import puppeteer from 'puppeteer';

const { values: args } = parseArgs({
  options: {
    url: { type: 'string', default: 'http://localhost:1420/?spike=1' },
    out: { type: 'string', default: 'e2e/out/frontend-shot.png' },
    'artifact-dir': { type: 'string' },
    chrome: { type: 'string' },
    swiftshader: { type: 'boolean', default: false },
    dpr: { type: 'string', default: '2' },
    width: { type: 'string', default: '1440' },
    height: { type: 'string', default: '900' },
    timeout: { type: 'string', default: '300' },
  },
});

function versionKey(dir) {
  return (dir.match(/\d+/g) ?? []).map(Number);
}

function compareDesc(a, b) {
  const ka = versionKey(a);
  const kb = versionKey(b);
  for (let i = 0; i < Math.max(ka.length, kb.length); i++) {
    const d = (kb[i] ?? 0) - (ka[i] ?? 0);
    if (d !== 0) return d;
  }
  return 0;
}

function findCachedChrome() {
  const root = path.join(os.homedir(), '.cache', 'puppeteer', 'chrome');
  if (!existsSync(root)) return undefined;
  const dirs = readdirSync(root, { withFileTypes: true })
    .filter((d) => d.isDirectory())
    .map((d) => d.name)
    .sort(compareDesc);
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

const outPath = path.resolve(args.out);
const artifactDir = path.resolve(args['artifact-dir'] ?? path.dirname(outPath));
await mkdir(path.dirname(outPath), { recursive: true });
await mkdir(artifactDir, { recursive: true });

const launchArgs = ['--no-first-run', '--no-default-browser-check', '--ignore-gpu-blocklist', '--enable-gpu-rasterization'];
if (args.swiftshader) launchArgs.push('--use-angle=swiftshader', '--enable-unsafe-swiftshader');

console.log(`chrome: ${executablePath}`);
console.log(`url:    ${args.url}`);
const browser = await puppeteer.launch({
  executablePath,
  headless: true,
  args: launchArgs,
  defaultViewport: {
    width: Number(args.width),
    height: Number(args.height),
    deviceScaleFactor: Number(args.dpr),
  },
});

let exitCode = 0;
try {
  const page = await browser.newPage();
  page.on('console', (msg) => {
    const text = msg.text();
    if (text.startsWith('[spike] report ')) return; // printed below, pretty
    if (msg.type() === 'error' || msg.type() === 'warn' || text.startsWith('[')) {
      console.log(`[page:${msg.type()}] ${text.slice(0, 400)}`);
    }
  });
  page.on('pageerror', (err) => console.log(`[page:exception] ${err.message}`));

  const t0 = Date.now();
  await page.goto(args.url, { waitUntil: 'load', timeout: 60_000 });
  await page.waitForFunction(() => window.__SPIKE_REPORT__ !== undefined, {
    timeout: Number(args.timeout) * 1000,
    polling: 250,
  });
  console.log(`spike finished in ${((Date.now() - t0) / 1000).toFixed(1)} s`);
  const report = await page.evaluate(() => window.__SPIKE_REPORT__);

  // Let the result banner and the restored plate settle, then capture the whole window.
  await new Promise((r) => setTimeout(r, 1500));
  await page.screenshot({ path: outPath });
  console.log(`screenshot: ${outPath}`);

  const pngBase64 = await page.evaluate(() => {
    const bytes = window.__SPIKE_ARTIFACTS__?.['viewport.png'];
    if (!bytes) return null;
    let s = '';
    for (let i = 0; i < bytes.length; i += 0x8000) {
      s += String.fromCharCode.apply(null, Array.from(bytes.subarray(i, i + 0x8000)));
    }
    return btoa(s);
  });
  if (pngBase64) {
    const artifactPath = path.join(artifactDir, 'viewport.png');
    await writeFile(artifactPath, Buffer.from(pngBase64, 'base64'));
    console.log(`artifact:   ${artifactPath}`);
  } else {
    console.log('artifact:   viewport.png was not saved by the harness');
    exitCode = 1;
  }
  const reportPath = path.join(artifactDir, 'frontend-spike-report.json');
  await writeFile(reportPath, JSON.stringify(report, null, 2));
  console.log(`report:     ${reportPath}`);
  console.log(JSON.stringify(report, null, 2));
} catch (err) {
  console.error('browser spike failed:', err);
  exitCode = 1;
} finally {
  await browser.close();
}
process.exit(exitCode);
