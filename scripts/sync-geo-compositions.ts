/**
 * Pulls the prod geoscript backend DB down and exports the latest version of every composition to
 * `geo_compositions/<id>_<title>.geo`, plus an `_index.tsv` listing (id, title, shared, featured,
 * description).
 *
 * Compositions that exist locally but not in prod are left alone; ones whose prod content changed
 * are overwritten, and a retitled composition's file is renamed to match.
 *
 *   bun scripts/sync-geo-compositions.ts [--no-pull] [--db <path>] [--out <dir>]
 */
import { Database } from 'bun:sqlite';
import { existsSync, readFileSync, readdirSync, renameSync, writeFileSync } from 'fs';
import { join } from 'path';

import type { CompositionDoc, NodeDef, TreeDef } from '../src/geoscript/geotoyAPIClient';

const REMOTE_DB = 'debian@ameo.dev:/opt/dream/db/geoscript_backend.sqlite3';
const ROOT = join(import.meta.dir, '..');

const args = process.argv.slice(2);
const optVal = (flag: string): string | undefined => {
  const i = args.indexOf(flag);
  return i >= 0 ? args[i + 1] : undefined;
};

const dbPath = optVal('--db') ?? join(ROOT, 'geoscript_backend.sqlite3');
const outDir = optVal('--out') ?? join(ROOT, 'geo_compositions');

if (!existsSync(outDir)) {
  console.error(`✗ output dir does not exist: ${outDir}`);
  process.exit(1);
}

if (!args.includes('--no-pull')) {
  console.log(`syncing ${REMOTE_DB} → ${dbPath}`);
  const { exitCode } = Bun.spawnSync(['rsync', REMOTE_DB, dbPath], {
    stdout: 'inherit',
    stderr: 'inherit',
  });
  if (exitCode !== 0) {
    console.error(`✗ rsync failed (exit ${exitCode})`);
    process.exit(1);
  }
}

interface Row {
  id: number;
  title: string;
  description: string;
  is_shared: number;
  is_featured: number;
  tree: string;
}

const db = new Database(dbPath, { readonly: true });
const rows = db
  .query(
    `SELECT c.id, c.title, c.description, c.is_shared, c.is_featured, v.tree
     FROM compositions c
     JOIN composition_versions v
       ON v.id = (SELECT MAX(id) FROM composition_versions WHERE composition_id = c.id)
     ORDER BY c.id`
  )
  .all() as Row[];

/** Root first, then depth-first through children; unreachable nodes trail in id order. */
const orderNodes = (tree: TreeDef): NodeDef[] => {
  const ordered: NodeDef[] = [];
  const seen = new Set<string>();
  const visit = (id: string) => {
    const node = tree.nodes[id];
    if (!node || seen.has(id)) {
      return;
    }
    seen.add(id);
    ordered.push(node);
    node.children.forEach(visit);
  };
  visit(tree.rootId);
  Object.keys(tree.nodes).sort().forEach(visit);
  return ordered;
};

const renderTreeSections = (tree: TreeDef): string[] => {
  const sections = orderNodes(tree).map(node => `// === node: ${node.name} ===\n${node.source}`);
  if (tree.globalsSource.trim()) {
    sections.unshift(`// === globals ===\n${tree.globalsSource}`);
  }
  return sections;
};

const renderGeo = (id: number, title: string, doc: CompositionDoc): string => {
  const sections =
    doc.trees.length === 1
      ? renderTreeSections(doc.trees[0].tree)
      : doc.trees.flatMap(entry => [
          `// ==== tree: ${entry.name} (${entry.kind}) ====`,
          ...renderTreeSections(entry.tree),
        ]);
  return `// Composition ${id}: ${title}\n${sections.join('\n')}\n`;
};

const slugify = (title: string) => title.replace(/[^A-Za-z0-9-]+/g, '_') || 'untitled';
const fileNameFor = (row: Row) => `${String(row.id).padStart(3, '0')}_${slugify(row.title)}.geo`;

const localById = new Map<number, string>();
for (const name of readdirSync(outDir)) {
  const match = /^(\d+)_.*\.geo$/.exec(name);
  if (match) {
    localById.set(Number(match[1]), name);
  }
}

let added = 0;
let updated = 0;
let renamed = 0;
for (const row of rows) {
  const doc = JSON.parse(row.tree) as CompositionDoc;
  if (doc.version !== 2) {
    console.warn(`! composition ${row.id}: unsupported tree version ${doc.version}; skipped`);
    continue;
  }

  const fileName = fileNameFor(row);
  const stale = localById.get(row.id);
  localById.delete(row.id);
  if (stale && stale !== fileName) {
    renameSync(join(outDir, stale), join(outDir, fileName));
    renamed += 1;
    console.log(`~ ${stale} → ${fileName}`);
  }

  const path = join(outDir, fileName);
  const prev = existsSync(path) ? readFileSync(path, 'utf8') : null;
  const content = renderGeo(row.id, row.title, doc);
  if (prev === content) {
    continue;
  }
  writeFileSync(path, content);
  if (prev === null) {
    added += 1;
    console.log(`+ ${fileName}`);
  } else {
    updated += 1;
    console.log(`* ${fileName}`);
  }
}

const escape = (s: string) => s.replace(/\t/g, '\\t').replace(/\r/g, '\\r').replace(/\n/g, '\\n');
const index =
  rows
    .map(row =>
      [
        row.id,
        escape(row.title),
        row.is_shared ? 1 : 0,
        row.is_featured ? 1 : 0,
        escape(row.description),
      ].join('\t')
    )
    .join('\n') + '\n';
writeFileSync(join(outDir, '_index.tsv'), index);

console.log(`\n${rows.length} compositions in prod: ${added} added, ${updated} updated, ${renamed} renamed`);
if (localById.size) {
  const names = [...localById.values()].sort();
  console.log(`${names.length} local-only, left untouched: ${names.join(', ')}`);
}
