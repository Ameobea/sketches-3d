// Headless before/after screenshot capture for the pomBench scene. Drives
// `window.pomBench.shot(material, preset, 'safe')` (sRGB RT readback -> PNG) so a
// material's look can be A/B'd across a source edit. Run with a label before the
// edit and again after, then `compose.mjs <before> <after>` for side-by-side+diff.
//
//   node scripts/pomShotHarness/shot.mjs <label> [material] [preset,preset,...]
//   e.g. node scripts/pomShotHarness/shot.mjs grooved_before grooved_plastic headOn,grazing,grazingX
//
// Requires the vite dev server on :4800. puppeteer is borrowed from the geoscript
// thumbnail_generator (no deps added here).
import { createRequire } from 'node:module';
import { writeFileSync, mkdirSync } from 'node:fs';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire('/Users/casey/dream/geoscript_backend/thumbnail_generator/package.json');
const puppeteer = require('puppeteer');

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT = `${HERE}/out`;
mkdirSync(OUT, { recursive: true });

const [label, material = 'grooved_plastic', presetsArg = 'headOn,grazing,grazingX'] = process.argv.slice(2);
if (!label) {
  console.error('usage: node shot.mjs <label> [material] [preset,preset,...]');
  process.exit(1);
}
const presets = presetsArg.split(',');

const browser = await puppeteer.launch({
  headless: true,
  protocolTimeout: 600_000,
  args: ['--use-gl=angle', '--use-angle=metal', '--ignore-gpu-blocklist', '--enable-gpu', '--no-sandbox'],
});
try {
  const page = await browser.newPage();
  await page.setViewport({ width: 1400, height: 900, deviceScaleFactor: 1 });
  page.on('pageerror', e => console.log('PAGEERROR:', e.message));
  await page.goto('http://localhost:4800/pomBench', { waitUntil: 'networkidle2', timeout: 120_000 });
  await page.waitForFunction(() => globalThis.pomBench && typeof globalThis.pomBench.shot === 'function', {
    timeout: 120_000,
  });
  for (const preset of presets) {
    const dataUrl = await page.evaluate((n, p) => globalThis.pomBench.shot(n, p, 'safe'), material, preset);
    const path = `${OUT}/${label}__${preset}.png`;
    writeFileSync(path, Buffer.from(dataUrl.split(',')[1], 'base64'));
    console.log('wrote', path);
  }
} finally {
  await browser.close();
}
