/**
 * Ports the level-def procedural library materials (`src/assets/materials/procedural/<name>/`)
 * into the Geotoy backend as standalone materials, reusing the exact server-side resolver so the
 * persisted def is byte-identical to what a level load produces (GLSL inlined, colors → int).
 *
 * Creates via the real `POST /materials/` endpoint so thumbnails render through the normal path.
 * Owner = whoever we log in as; there's no admin owner-override, so run against a backend where
 * you have the target account's credentials. Idempotent: re-porting a name you own updates it.
 *
 *   GEOTOY_USERNAME=ameo GEOTOY_PASSWORD=… bun scripts/port-procedural-materials.ts [options]
 *
 * Options:
 *   --base-url <url>   API base (default $GEOTOY_API_URL or https://3d.ameo.design/geotoy_api)
 *   --private          create/update as private (default: public / isShared=true)
 *   --only a,b,c       restrict to these material names
 *   --skip-existing    leave already-owned materials untouched instead of updating them
 *   --dry-run          resolve + validate + print, make no network writes
 *
 * Must be run from the repo root (the resolver reads assets relative to process.cwd()).
 */
import { existsSync, readdirSync } from 'fs';
import { join } from 'path';

import { resolveLibraryMaterial } from '../src/viz/levelDef/libraryMaterials.server';

const PROCEDURAL_SUBDIR = join('src', 'assets', 'materials', 'procedural');
const DEF_SIZE_LIMIT = 500_000;

const args = process.argv.slice(2);
const hasFlag = (f: string) => args.includes(f);
const optVal = (f: string): string | undefined => {
  const i = args.indexOf(f);
  return i >= 0 ? args[i + 1] : undefined;
};

const baseUrl = (
  optVal('--base-url') ??
  process.env.GEOTOY_API_URL ??
  'https://3d.ameo.design/geotoy_api'
).replace(/\/$/, '');
const isShared = !hasFlag('--private');
const dryRun = hasFlag('--dry-run');
const skipExisting = hasFlag('--skip-existing');
const only = optVal('--only')
  ?.split(',')
  .map(s => s.trim())
  .filter(Boolean);

const die = (msg: string): never => {
  console.error(`\n✗ ${msg}`);
  process.exit(1);
};

if (!existsSync(PROCEDURAL_SUBDIR)) {
  die(`Not at repo root: ${PROCEDURAL_SUBDIR} not found (cwd=${process.cwd()})`);
}

const names = readdirSync(PROCEDURAL_SUBDIR, { withFileTypes: true })
  .filter(e => e.isDirectory() && existsSync(join(PROCEDURAL_SUBDIR, e.name, `${e.name}.json`)))
  .map(e => e.name)
  .filter(n => !only || only.includes(n))
  .sort();

if (only) {
  const missing = only.filter(n => !names.includes(n));
  if (missing.length) die(`--only names not found as procedural materials: ${missing.join(', ')}`);
}
if (!names.length) die('No procedural materials matched.');

type Resolved = { name: string; def: Record<string, unknown>; bytes: number };
const resolved: Resolved[] = [];
for (const name of names) {
  const ref = `__ASSETS__/materials/procedural/${name}`;
  const { material, textures } = await resolveLibraryMaterial(ref);
  if (Object.keys(textures).length) {
    die(
      `"${name}" pulls in ${Object.keys(textures).length} texture(s); this porter only handles texture-free procedural materials. Upload the textures to Geotoy and remap slots first.`
    );
  }
  const def = { ...(material as Record<string, unknown>), name };
  const bytes = JSON.stringify(def).length;
  if (bytes > DEF_SIZE_LIMIT) die(`"${name}" def is ${bytes}B, over the ${DEF_SIZE_LIMIT}B column limit.`);
  resolved.push({ name, def, bytes });
}

console.log(`\nResolved ${resolved.length} procedural material(s):`);
for (const r of resolved) {
  const slots = Object.keys((r.def.shaders as Record<string, unknown>) ?? {});
  console.log(
    `  ${r.name.padEnd(22)} ${String(r.def.type).padEnd(14)} ${String(r.bytes).padStart(6)}B  [${slots.join(', ')}]`
  );
}

if (dryRun) {
  console.log(`\nDry run — no network writes. Target: ${baseUrl} (isShared=${isShared}).`);
  process.exit(0);
}

const username = process.env.GEOTOY_USERNAME ?? optVal('--username');
const password = process.env.GEOTOY_PASSWORD ?? optVal('--password');
if (!username || !password) {
  die('Set GEOTOY_USERNAME and GEOTOY_PASSWORD (or pass --username/--password).');
}

const login = async (): Promise<string> => {
  const res = await fetch(`${baseUrl}/users/login`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ username, password }),
  });
  if (!res.ok) die(`Login failed: ${res.status} ${res.statusText}\n${await res.text().catch(() => '')}`);
  const setCookies =
    res.headers.getSetCookie?.() ?? (res.headers.get('set-cookie') ? [res.headers.get('set-cookie')!] : []);
  const cookie = setCookies.map(c => c.split(';')[0]).join('; ');
  if (!cookie.includes('session_id=')) die('Login succeeded but no session_id cookie was returned.');
  return cookie;
};

type MaterialRow = { id: number; name: string; ownerName: string; isShared: boolean };

const cookie = await login();
console.log(`\nLogged in as ${username} → ${baseUrl}`);

const listRes = await fetch(`${baseUrl}/materials/`, { headers: { Cookie: cookie } });
if (!listRes.ok) die(`List materials failed: ${listRes.status} ${listRes.statusText}`);
const existing = (await listRes.json()) as MaterialRow[];
const mineByName = new Map(existing.filter(m => m.ownerName === username).map(m => [m.name, m]));

let created = 0,
  updated = 0,
  skipped = 0;
for (const { name, def, bytes } of resolved) {
  const prior = mineByName.get(name);
  const body = JSON.stringify({ name, materialDefinition: def, isShared });
  const common = { headers: { 'Content-Type': 'application/json', Cookie: cookie }, body };

  if (prior && skipExisting) {
    console.log(`  = ${name.padEnd(22)} skip (exists, id=${prior.id})`);
    skipped++;
    continue;
  }
  const [method, url, verb] = prior
    ? ['PUT', `${baseUrl}/materials/${prior.id}`, 'updated']
    : ['POST', `${baseUrl}/materials/`, 'created'];
  const res = await fetch(url, { method, ...common });
  if (!res.ok)
    die(`${verb} "${name}" failed: ${res.status} ${res.statusText}\n${await res.text().catch(() => '')}`);
  const row = (await res.json()) as { id: number };
  console.log(
    `  ${prior ? '↻' : '+'} ${name.padEnd(22)} ${verb} id=${row.id} (${bytes}B, isShared=${isShared})`
  );
  if (prior) {
    updated++;
  } else {
    created++;
  }
}

console.log(
  `\nDone: ${created} created, ${updated} updated, ${skipped} skipped. Thumbnails render async on the backend.`
);
