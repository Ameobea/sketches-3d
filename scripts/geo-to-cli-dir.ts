/**
 * Expands a synced `.geo` file into the directory layout `geotoy_cli` renders:
 * `main.geo` (`_root`), `nodes/<name>.geo` (every other node, flattened to identity-transform
 * children of `_root`), and `globals.geo`. Only one tree is written; the CLI has no multi-tree
 * input short of a full `tree.json`, which the flattened file can't reconstruct.
 *
 *   bun scripts/geo-to-cli-dir.ts <file.geo> <out_dir>
 */
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'fs';
import { join } from 'path';

import { parseGeoFile } from './geo-composition-doc';

const [file, outDir] = process.argv.slice(2);
if (!file || !outDir) {
  console.error('usage: bun scripts/geo-to-cli-dir.ts <file.geo> <out_dir>');
  process.exit(1);
}

const parsed = parseGeoFile(readFileSync(file, 'utf8'));
const tree =
  parsed.trees.find(t => t.name === 'main') ?? parsed.trees.find(t => t.kind === 'mesh') ?? parsed.trees[0];
if (parsed.trees.length > 1) {
  const skipped = parsed.trees.filter(t => t !== tree).map(t => `${t.name} (${t.kind})`);
  console.warn(
    `! ${parsed.trees.length} trees in file; writing \`${tree.name}\` only, dropping ${skipped.join(', ')}`
  );
}
const root = tree.nodes.find(n => n.name === '_root');
if (!root) {
  console.error(`✗ tree \`${tree.name}\` has no _root node`);
  process.exit(1);
}
const children = tree.nodes.filter(n => n !== root);

mkdirSync(outDir, { recursive: true });
rmSync(join(outDir, 'nodes'), { recursive: true, force: true });
rmSync(join(outDir, 'globals.geo'), { force: true });
writeFileSync(join(outDir, 'main.geo'), root.source);
if (children.length) {
  mkdirSync(join(outDir, 'nodes'));
  for (const { name, source } of children) {
    writeFileSync(join(outDir, 'nodes', `${name}.geo`), source);
  }
}
const hasGlobals = !!tree.globalsSource?.trim();
if (hasGlobals) {
  writeFileSync(join(outDir, 'globals.geo'), tree.globalsSource!);
}
console.log(
  `${outDir}: main.geo${children.length ? ` + nodes/{${children.map(n => n.name).join(',')}}.geo` : ''}${hasGlobals ? ' + globals.geo' : ''}`
);
