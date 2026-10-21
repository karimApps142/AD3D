// UX review (round 2) in a headless browser against the mock engine.
import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import puppeteer from 'puppeteer';

const url = process.argv[2] ?? 'http://localhost:1487/';
const out = process.argv[3] ?? '/tmp/ui-review';
await mkdir(out, { recursive: true });
const exe = `${process.env.HOME}/.cache/puppeteer/chrome/mac-148.0.7778.167/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing`;
const browser = await puppeteer.launch({ executablePath: exe, headless: true, args: ['--ignore-gpu-blocklist'], defaultViewport: { width: 1440, height: 900 } });
const page = await browser.newPage();
const logs = [];
page.on('console', (m) => { if (m.type() === 'error' || m.type() === 'warning') logs.push(`[${m.type()}] ${m.text().slice(0, 300)}`); });
page.on('pageerror', (e) => logs.push(`[pageerror] ${e.message}`));
await page.goto(url, { waitUntil: 'load' });
await page.waitForFunction(() => window.__AD3D__?.store.getState().mesh, { timeout: 60000 });
await new Promise((r) => setTimeout(r, 1500));
const shot = async (n) => page.screenshot({ path: path.join(out, `${n}.png`) });
await shot('01-launch');
const R = {};

// Help drawer open on first start?
R.helpOpenAtStart = await page.evaluate(() => !!document.querySelector('[aria-label*="Help" i][role="dialog"], .help-drawer'));

// Lamp effect vs an opened project with a point lamp.
R.lamp = await page.evaluate(async () => {
  const actions = await import('/src/app/actions.ts');
  const { useAppStore } = await import('/src/state/store.ts');
  const { useDocumentStore, isDirty } = await import('/src/state/document.ts');
  const host = (await import('/src/app/host.ts')).getHost();
  const params = { ...useAppStore.getState().params, light: { kind: 'point', posMm: [-400, 120, 80] }, optimiser: { kind: 'none' } };
  delete params.quality;
  await host.writeProject('browser:/lamp.ad3d', { generator: 'shadow_plate', generatorVersion: 1, params });
  await actions.openPath('browser:/lamp.ad3d');
  await new Promise((r) => setTimeout(r, 800));
  const s = useAppStore.getState().params;
  return { posAfterOpen: s.light?.posMm, dirty: isDirty(s, useDocumentStore.getState()) };
});
await shot('02-opened-point-lamp-project');

// Theme toggles.
for (const t of ['light', 'dark']) {
  await page.evaluate(async (t) => (await import('/src/app/actions.ts')).setTheme(t), t);
  await new Promise((r) => setTimeout(r, 400));
  await shot(`03-theme-${t}`);
}
R.theme = await page.evaluate(() => document.documentElement.dataset.theme);

// Keyboard: '?' opens shortcuts; Escape closes; F frames; Mod+K palette.
await page.keyboard.down('Shift'); await page.keyboard.press('Slash'); await page.keyboard.up('Shift');
await new Promise((r) => setTimeout(r, 300));
await shot('04-shortcuts');
R.shortcutsOpen = await page.evaluate(async () => (await import('/src/state/ui.ts')).useUiStore.getState());
await page.keyboard.press('Escape');
await page.keyboard.down('Meta'); await page.keyboard.press('KeyK'); await page.keyboard.up('Meta');
await new Promise((r) => setTimeout(r, 300));
await shot('05-palette');
R.paletteOpen = await page.evaluate(async () => (await import('/src/state/ui.ts')).useUiStore.getState().paletteOpen);
await page.keyboard.press('Escape');

// 'F' typed into the width number box must not frame the model.
R.fInInput = await page.evaluate(async () => {
  const { useAppStore } = await import('/src/state/store.ts');
  const before = useAppStore.getState().fitRequest;
  const box = document.querySelector('input[aria-label="Width value"]');
  box.focus();
  box.dispatchEvent(new KeyboardEvent('keydown', { key: 'f', bubbles: true }));
  return useAppStore.getState().fitRequest - before;
});

// Slider drag via real mouse: Width range.
const box = await page.$('input[type=range][data-param="maxTiltDeg"]');
const bb = await box.boundingBox();
R.drag = await page.evaluate(async () => {
  const { useAppStore } = await import('/src/state/store.ts');
  window.__q = [];
  useAppStore.subscribe((s, p) => { if (s.params !== p.params) window.__q.push(s.params.quality); });
  return true;
});
await page.mouse.move(bb.x + bb.width * 0.4, bb.y + bb.height / 2);
await page.mouse.down();
for (let i = 0; i < 10; i++) { await page.mouse.move(bb.x + bb.width * (0.4 + i * 0.03), bb.y + bb.height / 2); await new Promise((r) => setTimeout(r, 30)); }
await page.mouse.up();
await new Promise((r) => setTimeout(r, 500));
R.dragQualities = await page.evaluate(() => window.__q);
R.undoAfterDrag = await page.evaluate(async () => {
  const { paramsHistory } = await import('/src/state/paramsHistory.ts');
  const { useAppStore } = await import('/src/state/store.ts');
  const t0 = useAppStore.getState().params.maxTiltDeg;
  paramsHistory.undo();
  const s = useAppStore.getState().params;
  return { before: t0, after: s.maxTiltDeg, quality: s.quality };
});

// Lamp slider drag: what quality does it send?
R.lampDrag = await page.evaluate(async () => {
  const { useAppStore } = await import('/src/state/store.ts');
  useAppStore.getState().setParams({ light: { kind: 'point', posMm: [-600, 100, 60] }, quality: 'final' });
  return true;
});
await new Promise((r) => setTimeout(r, 500));
const lamp = await page.$('input[type=range][data-param="lampDistanceMm"]');
if (lamp) {
  const lb = await lamp.boundingBox();
  await page.evaluate(() => { window.__q = []; });
  await page.mouse.move(lb.x + lb.width * 0.2, lb.y + lb.height / 2);
  await page.mouse.down();
  for (let i = 0; i < 8; i++) { await page.mouse.move(lb.x + lb.width * (0.2 + i * 0.03), lb.y + lb.height / 2); await new Promise((r) => setTimeout(r, 30)); }
  await page.mouse.up();
  await new Promise((r) => setTimeout(r, 400));
  R.lampDragQualities = await page.evaluate(() => window.__q);
  R.lampUndo = await page.evaluate(async () => {
    const { paramsHistory } = await import('/src/state/paramsHistory.ts');
    const { useAppStore } = await import('/src/state/store.ts');
    const { useSquamaUi } = await import('/src/panels/squamaUi.ts');
    const before = useAppStore.getState().params.light.posMm.slice();
    paramsHistory.undo();
    await new Promise((r) => setTimeout(r, 300));
    return { before, after: useAppStore.getState().params.light?.posMm, lampUi: useSquamaUi.getState().lamp };
  });
  await shot('06-point-lamp');
}

// Huge plate vs bed: 800 mm wide, tiling off.
await page.evaluate(async () => {
  const { useSquamaUi } = await import('/src/panels/squamaUi.ts');
  const { useAppStore } = await import('/src/state/store.ts');
  useAppStore.getState().setParams({ widthMm: 800, light: undefined, quality: 'final' });
  useSquamaUi.getState().setExport({ tile: false });
});
await new Promise((r) => setTimeout(r, 800));
await page.evaluate(() => document.querySelector('[aria-label="Tiling and export"]')?.scrollIntoView());
await shot('07-too-big-no-tiles');
R.tooBig = await page.evaluate(() => document.querySelector('[aria-label="Tiling and export"]')?.innerText.slice(0, 1200));
// Help drawer
await page.evaluate(async () => (await import('/src/state/ui.ts')).openHelp('squama'));
await new Promise((r) => setTimeout(r, 400));
await shot('08-help');
R.logs = logs;
console.log(JSON.stringify(R, null, 2));
await browser.close();
