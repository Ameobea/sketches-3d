import { existsSync, readFileSync } from 'fs';
import { basename, join } from 'path';

import { error, json } from '@sveltejs/kit';
import type { RequestHandler } from '@sveltejs/kit';

import { getAssetsDir } from 'src/viz/levelDef/levelPaths.server';

import { guardDev, openLevel, validateName } from '../../levelEditorUtils.server';

/**
 * Registers a shared asset library file as a geoscript asset in the given level's def.
 *
 * If the file is already registered (same `file` path), returns the existing asset's id
 * without modifying the level def.
 *
 * Body: `{ file: "__ASSETS__/meshes/spinners/gear1.geo", id?: string }`
 * Response: `{ id: string, code: string }`
 *
 * Dev only.
 */
export const POST: RequestHandler = async ({ params, request }) => {
  guardDev();
  const name = validateName(params.name);

  const body = (await request.json()) as { file: string; id?: string };

  if (!body.file.startsWith('__ASSETS__/')) {
    error(400, 'file must start with __ASSETS__/');
  }

  const relativePath = body.file.slice('__ASSETS__/'.length);
  const filePath = join(getAssetsDir(), relativePath);

  if (!existsSync(filePath)) {
    error(404, `Asset file not found: ${body.file}`);
  }

  const code = readFileSync(filePath, 'utf-8');

  const level = openLevel(name);

  // If this file is already registered in the level def, reuse it.
  const existing = Object.entries(level.def.assets).find(
    ([, def]) => def.type === 'geoscript' && 'file' in def && def.file === body.file
  );
  if (existing) {
    return json({ id: existing[0], code });
  }

  // Generate a unique ID from the filename stem.
  const stem = basename(filePath, '.geo');
  let id = body.id ?? stem;
  if (id in level.def.assets) {
    let n = 1;
    while (`${id}_${n}` in level.def.assets) n++;
    id = `${id}_${n}`;
  }

  level.def.assets[id] = { type: 'geoscript', file: body.file };
  level.save();

  return json({ id, code }, { status: 201 });
};
