/**
 * Applies `geo_compositions/*.geo` sources onto a local sqlite snapshot as new versions, so the
 * corpus render harness (`--db`) can capture migrated compositions before they are pushed.
 *
 *   bun scripts/apply-geo-to-db.ts [--db <path>] [--only <id,id,...>] [file...]
 */
import { Database } from 'bun:sqlite';
import { readFileSync, readdirSync } from 'fs';
import { join } from 'path';

import type { CompositionDoc } from '../src/geoscript/geotoyAPIClient';
import { applyToDoc, parseGeoFile } from './geo-composition-doc';

const ROOT = join(import.meta.dir, '..');
const args = process.argv.slice(2);
const optVal = (flag: string): string | undefined => {
  const i = args.indexOf(flag);
  return i >= 0 ? args[i + 1] : undefined;
};
const dbPath = optVal('--db') ?? join(ROOT, 'geoscript_backend.sqlite3');
const only = optVal('--only')?.split(',').map(Number);
const files = args.filter((a, i) => a.endsWith('.geo') && args[i - 1] !== '--db' && args[i - 1] !== '--only');
const dir = join(ROOT, 'geo_compositions');
const targets = files.length
  ? files
  : readdirSync(dir)
      .filter(f => /^\d+_.*\.geo$/.test(f))
      .map(f => join(dir, f));

const db = new Database(dbPath);
const latest = db.query(
  `SELECT id, tree, metadata FROM composition_versions WHERE composition_id = ? ORDER BY id DESC LIMIT 1`
);
const insert = db.query(`INSERT INTO composition_versions (composition_id, tree, metadata) VALUES (?, ?, ?)`);

let applied = 0;
for (const file of targets) {
  const parsed = parseGeoFile(readFileSync(file, 'utf8'));
  if (only && !only.includes(parsed.id)) {
    continue;
  }
  const row = latest.get(parsed.id) as { id: number; tree: string; metadata: string } | null;
  if (!row) {
    console.log(`${parsed.id} ${parsed.title}: skipped (not in db)`);
    continue;
  }
  const doc = JSON.parse(row.tree) as CompositionDoc;
  const next = applyToDoc(doc, parsed);
  const tree = JSON.stringify(next);
  if (tree === JSON.stringify(doc)) {
    continue;
  }
  insert.run(parsed.id, tree, row.metadata);
  applied += 1;
  console.log(`${parsed.id} ${parsed.title}: applied`);
}
console.log(`${applied} compositions updated in ${dbPath}`);
