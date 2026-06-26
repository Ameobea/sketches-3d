/**
 * One-shot migration: Geotoy `physical`/`basic` material defs → shared `customShader`/`customBasicShader`.
 * Reuses the exact runtime converters (`geotoyMaterialConvert`) and validates every output against the
 * shared zod schema, so the persisted shape is guaranteed buildable by `src/viz/materials/buildMaterial`.
 *
 *   bun scripts/migrate-geotoy-materials.ts <db-path> [--write]
 *
 * Without `--write` it is a dry run (transforms + validates, writes nothing). Idempotent: already-shared
 * defs are skipped, so re-running is safe. `name` is preserved on the def (optional in the shared schema):
 * geoscript references materials by name and palette keys are often opaque uuids, so the name is load-bearing.
 */
import { Database } from 'bun:sqlite';

import { geotoyBasicToShared, geotoyPhysicalToShared } from '../src/geoscript/geotoyMaterialConvert';
import { CustomBasicShaderMatDefSchema, CustomShaderMatDefSchema } from '../src/viz/materials/schema';

const dbPath = process.argv[2];
const write = process.argv.includes('--write');
if (!dbPath) {
  console.error('usage: bun scripts/migrate-geotoy-materials.ts <db-path> [--write]');
  process.exit(1);
}

type MigOutcome =
  | { kind: 'migrated'; def: unknown; from: string }
  | { kind: 'skip' }
  | { kind: 'error'; msg: string };

const SHARED_TYPES = new Set(['customShader', 'customBasicShader', 'generated']);

const migrateDef = (raw: any, ctx: string): MigOutcome => {
  const t = raw?.type;
  if (SHARED_TYPES.has(t)) return { kind: 'skip' };
  try {
    if (t === 'physical') {
      const def = JSON.parse(JSON.stringify({ ...geotoyPhysicalToShared(raw), name: raw.name }));
      const r = CustomShaderMatDefSchema.safeParse(def);
      if (!r.success)
        return { kind: 'error', msg: `${ctx}: schema reject — ${r.error.message.slice(0, 300)}` };
      return { kind: 'migrated', def, from: t };
    }
    if (t === 'basic') {
      const def = JSON.parse(JSON.stringify({ ...geotoyBasicToShared(raw), name: raw.name }));
      const r = CustomBasicShaderMatDefSchema.safeParse(def);
      if (!r.success)
        return { kind: 'error', msg: `${ctx}: schema reject — ${r.error.message.slice(0, 300)}` };
      return { kind: 'migrated', def, from: t };
    }
    return { kind: 'error', msg: `${ctx}: unknown material type ${JSON.stringify(t)}` };
  } catch (e) {
    return { kind: 'error', msg: `${ctx}: convert threw — ${(e as Error).message}` };
  }
};

const db = new Database(dbPath, { readwrite: true });

const errors: string[] = [];
const fromCounts: Record<string, number> = {};
let matMigrated = 0,
  matSkipped = 0,
  paletteDefsMigrated = 0,
  paletteDefsSkipped = 0,
  versionsTouched = 0;
let maxMatLen = 0,
  maxMetaLen = 0;

const matUpdates: Array<{ id: number; def: string }> = [];
const matRows = db.query('SELECT id, name, material_definition FROM materials').all() as Array<{
  id: number;
  name: string;
  material_definition: string;
}>;
for (const row of matRows) {
  const out = migrateDef(JSON.parse(row.material_definition), `material#${row.id} "${row.name}"`);
  if (out.kind === 'migrated') {
    const json = JSON.stringify(out.def);
    maxMatLen = Math.max(maxMatLen, json.length);
    matUpdates.push({ id: row.id, def: json });
    matMigrated++;
    fromCounts[out.from] = (fromCounts[out.from] ?? 0) + 1;
  } else if (out.kind === 'skip') matSkipped++;
  else errors.push(out.msg);
}

const verUpdates: Array<{ id: number; meta: string }> = [];
const verRows = db
  .query(
    "SELECT id, metadata FROM composition_versions WHERE json_extract(metadata,'$.materials') IS NOT NULL"
  )
  .all() as Array<{ id: number; metadata: string }>;
for (const row of verRows) {
  const meta = JSON.parse(row.metadata);
  const palette = meta?.materials?.materials;
  if (!palette || typeof palette !== 'object') continue;
  let changed = false;
  for (const name of Object.keys(palette)) {
    const out = migrateDef(palette[name], `version#${row.id} mat "${name}"`);
    if (out.kind === 'migrated') {
      palette[name] = out.def;
      paletteDefsMigrated++;
      fromCounts[out.from] = (fromCounts[out.from] ?? 0) + 1;
      changed = true;
    } else if (out.kind === 'skip') paletteDefsSkipped++;
    else errors.push(out.msg);
  }
  if (changed) {
    const json = JSON.stringify(meta);
    maxMetaLen = Math.max(maxMetaLen, json.length);
    verUpdates.push({ id: row.id, meta: json });
    versionsTouched++;
  }
}

console.log(`\n== Geotoy material migration (${write ? 'WRITE' : 'DRY RUN'}) — ${dbPath} ==\n`);
console.log(
  `materials table:        ${matMigrated} migrated, ${matSkipped} already-shared, ${matRows.length} total`
);
console.log(
  `composition palettes:   ${paletteDefsMigrated} defs migrated across ${versionsTouched} versions, ${paletteDefsSkipped} already-shared`
);
console.log(`source types:           ${JSON.stringify(fromCounts)}`);
console.log(`max migrated size:      material=${maxMatLen}B  metadata=${maxMetaLen}B  (limit 500000)`);
console.log(`schema validation:      ${errors.length === 0 ? 'ALL PASS ✓' : `${errors.length} FAILURE(S)`}`);

if (errors.length) {
  console.log('\n-- errors --');
  for (const e of errors.slice(0, 50)) console.log('  ✗ ' + e);
  console.error('\nAborting: validation/convert errors present. No rows written.');
  process.exit(2);
}

if (maxMatLen > 500000 || maxMetaLen > 500000) {
  console.error('\nAborting: a migrated row exceeds the 500000-byte CHECK constraint.');
  process.exit(3);
}

if (!write) {
  console.log('\nDry run complete — pass --write to persist.');
  process.exit(0);
}

const tx = db.transaction(() => {
  const upMat = db.query('UPDATE materials SET material_definition = ? WHERE id = ?');
  for (const u of matUpdates) upMat.run(u.def, u.id);
  const upVer = db.query('UPDATE composition_versions SET metadata = ? WHERE id = ?');
  for (const u of verUpdates) upVer.run(u.meta, u.id);
});
tx();
console.log(`\nWrote ${matUpdates.length} materials + ${verUpdates.length} composition_versions. Done.`);
