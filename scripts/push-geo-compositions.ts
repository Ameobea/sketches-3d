/**
 * Pushes edited `geo_compositions/*.geo` files back to prod as new composition versions. Each
 * file's node sources + globals are applied onto the composition's current latest version
 * (fetched from prod, or from a local sync snapshot with `--db`, dry-run only), so instances,
 * hierarchy, and metadata are carried forward untouched. Nothing is ever overwritten.
 *
 *   bun scripts/push-geo-compositions.ts [--only <id,id,...>] [--dry-run] [--db <path>] [file...]
 *
 * Auth: `GEOTOY_SESSION=<session_id cookie>` in the gitignored `.env.geotoy-session` at the repo root.
 */
import { Database } from 'bun:sqlite';
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'fs';
import { tmpdir } from 'os';
import { join } from 'path';

import type { CompositionDoc, CompositionVersion } from '../src/geoscript/geotoyAPIClient';
import { applyToDoc, parseGeoFile } from './geo-composition-doc';

const API = 'https://3d.ameo.design/geotoy_api';
const ROOT = join(import.meta.dir, '..');

const args = process.argv.slice(2);
const takeFlag = (flag: string): boolean => {
  const i = args.indexOf(flag);
  if (i < 0) {
    return false;
  }
  args.splice(i, 1);
  return true;
};
const takeOpt = (flag: string): string | undefined => {
  const i = args.indexOf(flag);
  return i < 0 ? undefined : args.splice(i, 2)[1];
};

const dryRun = takeFlag('--dry-run');
const only = takeOpt('--only')
  ?.split(',')
  .map(s => Number(s.trim()));
const dbPath = takeOpt('--db');
if (dbPath && !dryRun) {
  console.error('✗ --db reads a possibly stale snapshot; only allowed with --dry-run');
  process.exit(1);
}

const compDir = join(ROOT, 'geo_compositions');
const files = args.length
  ? args
  : readdirSync(compDir)
      .filter(n => /^\d+_.*\.geo$/.test(n))
      .sort()
      .map(n => join(compDir, n));

const readSession = (): string | undefined => {
  const path = join(ROOT, '.env.geotoy-session');
  if (!existsSync(path)) {
    return undefined;
  }
  const m = /^GEOTOY_SESSION=["']?([^"'\n]+)["']?$/m.exec(readFileSync(path, 'utf8'));
  return m?.[1].trim();
};
const session = readSession();
if (!session && !dryRun) {
  console.error('✗ GEOTOY_SESSION missing from .env.geotoy-session');
  process.exit(1);
}
const authHeaders: Record<string, string> = session ? { Cookie: `session_id=${session}` } : {};

const db = dbPath ? new Database(dbPath, { readonly: true }) : null;

type Latest = Pick<CompositionVersion, 'tree' | 'metadata'>;

const fetchLatest = async (id: number): Promise<Latest> => {
  if (db) {
    const row = db
      .query(
        'SELECT tree, metadata FROM composition_versions WHERE composition_id = ? ORDER BY id DESC LIMIT 1'
      )
      .get(id) as { tree: string; metadata: string } | null;
    if (!row) {
      throw new Error('not in snapshot');
    }
    return { tree: JSON.parse(row.tree), metadata: JSON.parse(row.metadata) };
  }
  const res = await fetch(`${API}/compositions/${id}/latest`, { headers: authHeaders });
  if (!res.ok) {
    throw new Error(`GET latest → ${res.status} ${await res.text()}`);
  }
  return res.json();
};

const postVersion = async (id: number, body: Latest): Promise<CompositionVersion> => {
  const res = await fetch(`${API}/compositions/${id}/versions`, {
    method: 'POST',
    headers: { ...authHeaders, 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    throw new Error(`POST version → ${res.status} ${await res.text()}`);
  }
  return res.json();
};

interface Change {
  label: string;
  before: string;
  after: string;
}

const changesBetween = (a: CompositionDoc, b: CompositionDoc): Change[] =>
  a.trees.flatMap(({ name, tree }, i) => {
    const next = b.trees[i].tree;
    const out: Change[] = [];
    if (tree.globalsSource !== next.globalsSource) {
      out.push({ label: `${name}/globals`, before: tree.globalsSource, after: next.globalsSource });
    }
    for (const [id, node] of Object.entries(tree.nodes)) {
      if (node.source !== next.nodes[id].source) {
        out.push({ label: `${name}/${node.name}`, before: node.source, after: next.nodes[id].source });
      }
    }
    return out;
  });

const unifiedDiff = ({ label, before, after }: Change): string => {
  const dir = mkdtempSync(join(tmpdir(), 'geo-push-'));
  const [a, b] = [join(dir, 'a'), join(dir, 'b')];
  writeFileSync(a, before);
  writeFileSync(b, after);
  const out = Bun.spawnSync(['diff', '-u', '-L', `a/${label}`, '-L', `b/${label}`, a, b]).stdout.toString();
  rmSync(dir, { recursive: true });
  return out;
};

let failed = false;
for (const file of files) {
  const fileId = Number(/^(\d+)_/.exec(file.split('/').at(-1)!)?.[1]);
  if (only && !only.includes(fileId)) {
    continue;
  }
  let parsed;
  try {
    parsed = parseGeoFile(readFileSync(file, 'utf8'));
  } catch (err) {
    console.log(`${file}: skipped (${(err as Error).message})`);
    failed = true;
    continue;
  }
  const tag = `${parsed.id} ${parsed.title}`;
  try {
    const latest = await fetchLatest(parsed.id);
    if (latest.tree.version !== 2) {
      throw new Error(`unsupported tree version ${latest.tree.version}`);
    }
    const applied = applyToDoc(latest.tree, parsed);
    const changes = changesBetween(latest.tree, applied);
    if (!changes.length) {
      console.log(`${tag}: unchanged`);
      continue;
    }
    if (dryRun) {
      console.log(`${tag}: would push (${changes.map(c => c.label).join(', ')})`);
      for (const change of changes) {
        process.stdout.write(unifiedDiff(change));
      }
      continue;
    }
    const version = await postVersion(parsed.id, { tree: applied, metadata: latest.metadata });
    console.log(`${tag}: pushed (version ${version.id})`);
  } catch (err) {
    console.log(`${tag}: skipped (${(err as Error).message})`);
    failed = true;
  }
}
process.exit(failed ? 1 : 0);
