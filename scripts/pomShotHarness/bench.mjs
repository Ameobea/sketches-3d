// Headless pomBench perf run: drives `window.pomBench.run(repeats)` (GPU-timer sweeps)
// and dumps the published table to out/<label>.{json,txt}. Pair two labelled runs across
// a source edit for a same-session A/B (the unchanged materials anchor the machine-state
// scale). Requires the vite dev server on :4800; puppeteer borrowed from thumbnail_generator.
//
//   node scripts/pomShotHarness/bench.mjs <label> [repeats=3]
import { createRequire } from 'node:module';
import { writeFileSync, mkdirSync } from 'node:fs';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire('/Users/casey/dream/geoscript_backend/thumbnail_generator/package.json');
const puppeteer = require('puppeteer');

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT = `${HERE}/out`;
mkdirSync(OUT, { recursive: true });

const [label = 'bench', repeatsArg = '3'] = process.argv.slice(2);
const REPEATS = Number(repeatsArg);

const browser = await puppeteer.launch({
  headless: true,
  protocolTimeout: 1_800_000,
  args: ['--use-gl=angle', '--use-angle=metal', '--ignore-gpu-blocklist', '--enable-gpu', '--no-sandbox'],
});
try {
  const page = await browser.newPage();
  await page.setViewport({ width: 1400, height: 900, deviceScaleFactor: 1 });
  page.on('console', m => {
    const t = m.text();
    if (t.includes('sweep')) console.log('PAGE:', t.split('\n')[0]);
  });
  page.on('pageerror', e => console.log('PAGEERROR:', e.message));
  await page.goto('http://localhost:4800/pomBench', { waitUntil: 'networkidle2', timeout: 120_000 });
  await page.waitForFunction(() => globalThis.pomBench && typeof globalThis.pomBench.run === 'function', {
    timeout: 120_000,
  });
  console.log(`[${label}] running ${REPEATS} sweeps ...`);
  const results = await page.evaluate(async n => {
    await globalThis.pomBench.run(n);
    return globalThis.pomBench.results;
  }, REPEATS);

  writeFileSync(`${OUT}/${label}.json`, JSON.stringify(results, null, 2));
  const fmt = r =>
    [
      String(r.material).padEnd(19),
      String(r.preset).padEnd(8),
      String(r['full(ms)']).padStart(8),
      ('±' + r.std).padStart(8),
      String(r['Δpom(ms)'] ?? '—').padStart(9),
      String(r.evalProxy ?? '—').padStart(10),
    ].join(' ');
  let out = `=== ${label}  ${REPEATS} sweeps  method=${results[0]?.method} ===\n`;
  out += 'material            preset    full(ms)    ±std   Δpom(ms)  evalProxy\n';
  for (const preset of ['headOn', 'grazing']) {
    for (const r of results.filter(r => r.preset === preset)) out += fmt(r) + '\n';
    out += '\n';
  }
  console.log('\n' + out);
  writeFileSync(`${OUT}/${label}.txt`, out);
} finally {
  await browser.close();
}
