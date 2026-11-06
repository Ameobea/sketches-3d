#!/usr/bin/env node
// Visual-regression capture: renders each scene pose / geotoy comp via the app's `?capture=1` hooks on the
// real GPU and writes `<name>.png` + `<name>.json` (camera, renderer.info, console warnings/errors).
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { parseArgs } from 'node:util';
import puppeteer from 'puppeteer-core';

const { values: opt } = parseArgs({
  options: {
    out: { type: 'string', default: path.join(os.homedir(), 'three-upgrade-baselines', `run-${Date.now()}`) },
    config: { type: 'string', default: path.join(import.meta.dirname, 'scenes.json') },
    base: { type: 'string', default: 'http://localhost:5897' },
    quality: { type: 'string', default: 'high' },
    only: { type: 'string' },
    'all-locations': { type: 'boolean', default: false },
    'no-geotoy': { type: 'boolean', default: false },
    'no-scenes': { type: 'boolean', default: false },
    chrome: { type: 'string', default: '/usr/bin/google-chrome-unstable' },
    'timeout-s': { type: 'string', default: '180' },
  },
});
if (!['low', 'med', 'high'].includes(opt.quality)) throw new Error(`--quality must be low|med|high`);
const cfg = JSON.parse(fs.readFileSync(opt.config, 'utf8'));
const only = opt.only?.split(',');
const pick = name => !only || only.includes(name);
const TIMEOUT = +opt['timeout-s'] * 1e3;
const [W, H] = [1280, 720];
fs.mkdirSync(opt.out, { recursive: true });

const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'visreg-prof-'));
const browser = await puppeteer.launch({
  executablePath: opt.chrome,
  headless: true,
  args: [
    '--headless=new',
    '--use-gl=angle',
    '--use-angle=gl',
    '--ignore-gpu-blocklist',
    '--enable-gpu',
    `--window-size=${W},${H}`,
    '--disable-background-timer-throttling',
    '--disable-renderer-backgrounding',
    '--disable-backgrounding-occluded-windows',
    '--autoplay-policy=no-user-gesture-required',
    `--user-data-dir=${profile}`,
  ],
});

const report = {
  base: opt.base,
  quality: opt.quality,
  started: new Date().toISOString(),
  gpu: null,
  images: [],
  failures: [],
};
const timeout = (ms, what) =>
  new Promise((_, rej) => setTimeout(() => rej(new Error(`${what} timed out after ${ms / 1e3}s`)), ms));
const safe = s => String(s).replace(/[^\w.-]+/g, '_');

const openPage = async (w, h) => {
  const page = await browser.newPage();
  await page.setViewport({ width: w, height: h, deviceScaleFactor: 1 });
  const log = [];
  page.on(
    'console',
    m =>
      ['error', 'warn', 'warning', 'assert'].includes(m.type()) &&
      log.push({ type: m.type(), text: m.text().slice(0, 500) })
  );
  page.on('pageerror', e => log.push({ type: 'pageerror', text: String(e?.stack ?? e).slice(0, 800) }));
  page.on('requestfailed', r =>
    log.push({ type: 'requestfailed', text: `${r.url().slice(0, 200)} ${r.failure()?.errorText}` })
  );
  page.on(
    'response',
    r => r.status() >= 400 && log.push({ type: 'http', text: `${r.status()} ${r.url().slice(0, 200)}` })
  );
  return { page, log };
};

const recordGpu = async page =>
  (report.gpu ??= await page.evaluate(() => {
    const gl = document.createElement('canvas').getContext('webgl2');
    const ext = gl?.getExtension('WEBGL_debug_renderer_info');
    return ext ? gl.getParameter(ext.UNMASKED_RENDERER_WEBGL) : 'unknown';
  }));

const save = (name, png, meta, log) => {
  const buf = Buffer.from(png.split(',')[1], 'base64');
  fs.writeFileSync(path.join(opt.out, `${name}.png`), buf);
  const sidecar = { ...meta, quality: opt.quality, console: [...log] };
  fs.writeFileSync(path.join(opt.out, `${name}.json`), JSON.stringify(sidecar, null, 1));
  const errors = log.filter(l => l.type !== 'warn' && l.type !== 'warning').length;
  report.images.push({
    name,
    sha1: crypto.createHash('sha1').update(buf).digest('hex'),
    errors,
    readyError: meta.readyError,
  });
  console.log(`${name}: ${errors} errors${meta.readyError ? `, NOT READY: ${meta.readyError}` : ''}`);
};

// `vizReady` misses some async work (level texture pool, geoscript worker results), so additionally wait for the
// network to idle and dt=0 re-renders (sim time doesn't advance) to stop changing.
const settle = async (page, name, stableFor = 4, maxTries = 60) => {
  let prev,
    same = 0;
  for (let i = 0; i < maxTries && same < stableFor; i++) {
    await page.waitForNetworkIdle({ idleTime: 500, timeout: 30e3 }).catch(() => {});
    const png = await page.evaluate(
      () => (window.viz.captureFrame(0), window.viz.renderer.domElement.toDataURL('image/png'))
    );
    const h = crypto.createHash('sha1').update(png).digest('hex');
    same = h === prev ? same + 1 : 0;
    prev = h;
  }
  if (same < stableFor) console.log(`WARNING: ${name}: frame never stabilized`);
};

const captureScene = async sc => {
  const { page, log } = await openPage(W, H);
  const url = `${opt.base}/${sc.scene ?? sc.name}?capture=1&quality=${opt.quality}`;
  const t0 = Date.now();
  let readyError = null;
  try {
    await page.goto(url, { waitUntil: 'load', timeout: TIMEOUT });
    await recordGpu(page);
    await Promise.race([
      page
        .waitForFunction(() => !!window.vizReady, { polling: 100, timeout: 0 })
        .then(() =>
          page.evaluate(() =>
            window.vizReady.then(
              () => null,
              e => String(e?.stack ?? e)
            )
          )
        ),
      timeout(TIMEOUT, 'vizReady'),
    ]).then(
      e => (readyError = e),
      e => (readyError = e.message)
    );
    if (!readyError) await settle(page, sc.name);
    const locations = await page.evaluate(() => Object.keys(window.viz?.sceneConf?.locations ?? {}));
    const poses = opt['all-locations'] ? locations : (sc.poses ?? ['spawn']);
    const readyMs = Date.now() - t0;
    for (const p of poses) {
      const { name: poseName, ...pose } = typeof p === 'string' ? { name: p } : p;
      const name = `${sc.name}__${safe(poseName)}`;
      try {
        const meta = await page.evaluate(
          (pose, name, freeze) => {
            window.vizCapture.pose(pose.pos ? pose : name);
            return window.vizCapture.freeze(freeze);
          },
          pose,
          poseName,
          { time: sc.time ?? 10, frames: sc.frames ?? 8 }
        );
        const png = await page.evaluate(() => window.viz.renderer.domElement.toDataURL('image/png'));
        save(name, png, { scene: sc.name, pose: p, url, readyMs, readyError, locations, ...meta }, log);
      } catch (e) {
        report.failures.push({ name, error: String(e.message ?? e).slice(0, 800), console: [...log] });
        console.log(`${name}: FAILED ${String(e.message ?? e).slice(0, 300)}`);
      }
    }
  } catch (e) {
    report.failures.push({ name: sc.name, error: String(e.message ?? e).slice(0, 800), console: [...log] });
    console.log(`${sc.name}: FAILED ${String(e.message ?? e).slice(0, 300)}`);
  }
  await page.close();
};

const captureGeotoy = async id => {
  const { page, log } = await openPage(800, 800);
  const name = `geotoy_${id}`;
  // not `capture=1`: geotoy's own render harness needs the live rAF loop that capture mode freezes
  const url = `${opt.base}/geotoy/edit/${id}?render=true`;
  const t0 = Date.now();
  try {
    const ready = new Promise((res, rej) => {
      page.exposeFunction('onRenderReady', res);
      page.exposeFunction('onRenderError', m => rej(new Error(`onRenderError: ${m}`)));
    });
    await page.goto(url, { waitUntil: 'domcontentloaded', timeout: TIMEOUT });
    await recordGpu(page);
    await Promise.race([ready, timeout(TIMEOUT, 'onRenderReady')]);
    const { png, ...meta } = await page.evaluate(() => {
      const { renderer, camera } = window.viz;
      const { render, memory, programs } = renderer.info;
      return {
        png: renderer.domElement.toDataURL('image/png'),
        camera: { pos: camera.position.toArray(), quat: camera.quaternion.toArray(), fov: camera.fov },
        info: { programs: programs?.length ?? 0, ...render, ...memory },
      };
    });
    save(name, png, { geotoy: id, url, readyMs: Date.now() - t0, ...meta }, log);
  } catch (e) {
    report.failures.push({ name, error: String(e.message ?? e).slice(0, 800), console: [...log] });
    console.log(`${name}: FAILED ${String(e.message ?? e).slice(0, 300)}`);
  }
  await page.close();
};

try {
  if (!opt['no-scenes']) for (const sc of cfg.scenes) if (pick(sc.name)) await captureScene(sc);
  if (!opt['no-geotoy']) for (const id of cfg.geotoy ?? []) if (pick(`geotoy_${id}`)) await captureGeotoy(id);
} finally {
  await browser.close();
  fs.rmSync(profile, { recursive: true, force: true });
}

const bySha = Object.groupBy(report.images, i => i.sha1);
report.duplicates = Object.values(bySha)
  .filter(g => g.length > 1)
  .map(g => g.map(i => i.name));
fs.writeFileSync(path.join(opt.out, 'report.json'), JSON.stringify(report, null, 1));
console.log(
  `\n${report.images.length} images, ${report.failures.length} failures -> ${opt.out}  [GPU: ${report.gpu}]`
);
if (/swiftshader|llvmpipe/i.test(report.gpu ?? ''))
  console.log('WARNING: software rendering, not the real GPU');
for (const d of report.duplicates) console.log(`WARNING: identical images (stale camera?): ${d.join(', ')}`);
process.exit(report.failures.length ? 1 : 0);
