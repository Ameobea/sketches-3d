// Stitch two `shot.mjs` capture sets into side-by-side comparison strips
// (before | after | amplified diff) per preset, for eyeballing a material edit.
//
//   node scripts/pomShotHarness/compose.mjs <labelA> <labelB> [material] [preset,...]
//   e.g. node scripts/pomShotHarness/compose.mjs grooved_before grooved_after grooved_plastic
//
// diff = |A-B| per channel, ×DIFF_AMP so sub-perceptual changes show. sharp is
// borrowed from the geoscript thumbnail_generator.
import { createRequire } from 'node:module';
import { existsSync } from 'node:fs';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire('/Users/casey/dream/geoscript_backend/thumbnail_generator/package.json');
const sharp = require('sharp');

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT = `${HERE}/out`;
const DIFF_AMP = 6;

const [labelA, labelB, material = 'grooved_plastic', presetsArg = 'headOn,grazing,grazingX'] =
  process.argv.slice(2);
if (!labelA || !labelB) {
  console.error('usage: node compose.mjs <labelA> <labelB> [material] [preset,...]');
  process.exit(1);
}

for (const preset of presetsArg.split(',')) {
  const a = `${OUT}/${labelA}__${preset}.png`,
    b = `${OUT}/${labelB}__${preset}.png`;
  if (!existsSync(a) || !existsSync(b)) {
    console.log('skip', preset, '(missing capture)');
    continue;
  }
  const { width: W, height: H } = await sharp(a).metadata();
  const bBuf = await sharp(b).toBuffer();
  const diff = await sharp(a).composite([{ input: bBuf, blend: 'difference' }]).linear(DIFF_AMP, 0).png().toBuffer();
  const out = `${OUT}/cmp_${material}__${preset}.png`;
  await sharp({ create: { width: W * 3, height: H, channels: 3, background: '#101010' } })
    .composite([
      { input: a, left: 0, top: 0 },
      { input: b, left: W, top: 0 },
      { input: diff, left: W * 2, top: 0 },
    ])
    .png()
    .toFile(out);
  console.log('wrote', out, `(${labelA} | ${labelB} | diff×${DIFF_AMP})`);
}
