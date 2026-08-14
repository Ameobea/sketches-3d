/**
 * Renders (or `geotoy eval`s) the latest version of every composition in a geoscript
 * backend DB via the transient render path (backend :5810 → thumbnail_generator :5812 →
 * dev :4800). Used for before/after diff validation of migrations and refactors.
 *
 *   bun scripts/geotoy-corpus-render.ts --out <dir> [--db <path>] [--size <px>] [--only <id,id,...>]
 *                                        [--concurrency <n>]   (default 6)
 *   bun scripts/geotoy-corpus-render.ts --out <dir> --eval [--expr <geoscript>] [--meshes <fmt>] [--only ...]
 *
 * `KNOWN_TIMEOUT_IDS` are excluded by default (`--include-timeouts` to keep them, or name
 * one in `--only`); the other known-bad comps fail in milliseconds and stay in for signal.
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

/**
 * Comps that burn the full `timeoutMs` every run (~2min of a ~6min capture) without
 * producing a raster to diff. The other expected failures are syntax errors that fail
 * instantly, so they stay in and keep their `_failures.json` entries as regression signal.
 */
const KNOWN_TIMEOUT_IDS = new Set([64, 123]);
const includeTimeouts = args.includes('--include-timeouts');
const concurrencyRaw = Number(optVal('--concurrency') ?? 6);
// A NaN here silently renders nothing (`Array.from({length: NaN})` is empty) and still
// reports success, which reads exactly like "everything was already present".
if (!Number.isFinite(concurrencyRaw) || concurrencyRaw < 1) die('--concurrency must be a positive number');
const concurrency = Math.floor(concurrencyRaw);

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

const outPathFor = (id: number) => join(outDir, evalMode ? `${id}.eval.json` : `${id}.png`);

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
const skippedTimeouts: number[] = [];

const pending = rows
  .filter(row => {
    if (only) return only.includes(row.id);
    // Naming one in `--only` is an explicit request, so it wins over the default exclusion.
    if (!includeTimeouts && KNOWN_TIMEOUT_IDS.has(row.id)) {
      skippedTimeouts.push(row.id);
      return false;
    }
    return true;
  })
  .filter(row => {
    if (!existsSync(outPathFor(row.id))) return true;
    skipped += 1;
    return false;
  });

const renderRow = async (row: Row) => {
  const tree = JSON.parse(row.tree);
  // Verbatim: the render page accepts a stored v1 blob, so no downgrade can drift here.
  const metadata = JSON.parse(row.metadata);
  if (metadata.version !== 1) {
    throw new Error(
      `composition ${row.id}: metadata is not v1 — run the backend once so its migrations apply.`
    );
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
      writeFileSync(outPathFor(row.id), normalizeEnvelope(await res.text()));
    } else {
      writeFileSync(outPathFor(row.id), Buffer.from(await res.arrayBuffer()));
    }
    rendered += 1;
    console.log(`✓ ${row.id} ${row.title} (${Date.now() - started}ms)`);
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    failures.push({ id: row.id, title: row.title, error });
    console.log(`✗ ${row.id} ${row.title}: ${error.split('\n')[0]}`);
  }
};

// Each render is served by its own short-lived browser (thumbnail_generator launches and
// closes one per request) and both hops upstream are plain async handlers, so nothing
// upstream serializes and workers only contend for CPU.
let next = 0;
await Promise.all(
  Array.from({ length: Math.min(concurrency, pending.length) }, async () => {
    while (next < pending.length) await renderRow(pending[next++]);
  })
);

// Sorted so the file is byte-stable regardless of completion order under `--concurrency`.
failures.sort((a, b) => a.id - b.id);
writeFileSync(join(outDir, '_failures.json'), JSON.stringify(failures, null, 2));
console.log(
  `\n${rendered} rendered, ${skipped} already present, ${failures.length} failed` +
    (skippedTimeouts.length > 0
      ? `, ${skippedTimeouts.length} known-timeout skipped (${skippedTimeouts})`
      : '')
);
