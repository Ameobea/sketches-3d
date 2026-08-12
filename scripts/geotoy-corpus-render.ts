/**
 * Renders (or `geotoy eval`s) the latest version of every composition in a geoscript
 * backend DB via the transient render path (backend :5810 → thumbnail_generator :5812 →
 * dev :4800). Used for before/after diff validation of migrations and refactors.
 *
 *   bun scripts/geotoy-corpus-render.ts --out <dir> [--db <path>] [--size <px>] [--only <id,id,...>]
 *   bun scripts/geotoy-corpus-render.ts --out <dir> --eval [--expr <geoscript>] [--meshes <fmt>] [--only ...]
 *
 * Eval envelopes are normalized (runtimeMs zeroed, keys sorted) so they diff stably.
 * Canonical eval-baseline invocations (see docs/geotoy-app-shell-refactor.md):
 *   --eval --only 102,103,108,125,128
 *   --eval --only 128 --expr 'vec3(1, 2, 3) * 2.5'
 *   --eval --only 128 --meshes obj
 */
import { Database } from 'bun:sqlite';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'fs';
import { join } from 'path';

const ROOT = join(import.meta.dir, '..');

const args = process.argv.slice(2);
const optVal = (flag: string): string | undefined => {
  const i = args.indexOf(flag);
  return i >= 0 ? args[i + 1] : undefined;
};

const dbPath = optVal('--db') ?? join(ROOT, 'geoscript_backend', 'geoscript_backend.sqlite3');
const outDir = optVal('--out') ?? die('--out is required');
const size = Number(optVal('--size') ?? 512);
const only = optVal('--only')
  ?.split(',')
  .map(s => Number(s));
const evalMode = args.includes('--eval');
const evalExpr = optVal('--expr');
const evalMeshes = optVal('--meshes') ?? 'summary';

const sortKeysDeep = (v: unknown): unknown => {
  if (Array.isArray(v)) return v.map(sortKeysDeep);
  if (v && typeof v === 'object') {
    return Object.fromEntries(
      Object.keys(v as Record<string, unknown>)
        .sort()
        .map(k => [k, sortKeysDeep((v as Record<string, unknown>)[k])])
    );
  }
  return v;
};

const normalizeEnvelope = (raw: string): string => {
  const env = JSON.parse(raw);
  if (env.stats) env.stats.runtimeMs = 0;
  return JSON.stringify(sortKeysDeep(env), null, 2);
};

function die(msg: string): never {
  console.error(`✗ ${msg}`);
  process.exit(1);
}

const token =
  process.env.GEOTOY_CLI_TOKEN ??
  /cli_token:\s*'([^']+)'/.exec(readFileSync(join(ROOT, 'geoscript_backend', 'config.yml'), 'utf8'))?.[1] ??
  die('no CLI token (config.yml cli_token or $GEOTOY_CLI_TOKEN)');

mkdirSync(outDir, { recursive: true });

interface Row {
  id: number;
  title: string;
  tree: string;
  metadata: string;
}

const rows = new Database(dbPath, { readonly: true })
  .query(
    `SELECT c.id, c.title, v.tree, v.metadata FROM compositions c
     JOIN composition_versions v
       ON v.id = (SELECT MAX(id) FROM composition_versions WHERE composition_id = c.id)
     ORDER BY c.id`
  )
  .all() as Row[];

const failures: { id: number; title: string; error: string }[] = [];
let rendered = 0;
let skipped = 0;

for (const row of rows) {
  if (only && !only.includes(row.id)) continue;
  const outPath = join(outDir, evalMode ? `${row.id}.eval.json` : `${row.id}.png`);
  if (existsSync(outPath)) {
    skipped += 1;
    continue;
  }
  const tree = JSON.parse(row.tree);
  const metadata = JSON.parse(row.metadata);
  // Transient payloads carry a single `view`; map migrated per-tree views down to it.
  if (metadata.views) {
    const activeId = metadata.activeTreeId ?? tree.trees?.[0]?.id;
    const view = metadata.views[activeId];
    delete metadata.views;
    delete metadata.activeTreeId;
    if (view) metadata.view = view;
  }
  const payload = {
    tree,
    metadata,
    options: evalMode
      ? { dev: true, timeoutMs: 60_000, eval: { expr: evalExpr, samples: 0, meshes: evalMeshes } }
      : { format: 'png', dev: true, width: size, height: size, timeoutMs: 60_000 },
  };
  const started = Date.now();
  try {
    const res = await fetch('http://localhost:5810/render/transient', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'X-CLI-Token': token },
      body: JSON.stringify(payload),
    });
    if (!res.ok) {
      const text = await res.text().catch(() => '');
      throw new Error(`${res.status}: ${text.slice(0, 300)}`);
    }
    if (evalMode) {
      writeFileSync(outPath, normalizeEnvelope(await res.text()));
    } else {
      writeFileSync(outPath, Buffer.from(await res.arrayBuffer()));
    }
    rendered += 1;
    console.log(`✓ ${row.id} ${row.title} (${Date.now() - started}ms)`);
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    failures.push({ id: row.id, title: row.title, error });
    console.log(`✗ ${row.id} ${row.title}: ${error.split('\n')[0]}`);
  }
}

writeFileSync(join(outDir, '_failures.json'), JSON.stringify(failures, null, 2));
console.log(`\n${rendered} rendered, ${skipped} already present, ${failures.length} failed`);
