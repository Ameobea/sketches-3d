import { error, type RequestHandler } from '@sveltejs/kit';

import type { MaterialDef } from 'src/viz/levelDef/types';
import { guardDev, validateName, readLevel, writeLevel } from '../../levelEditorUtils.server';

/** Upsert (create or full replace) a material */
export const PUT: RequestHandler = async ({ params, request }) => {
  guardDev();
  const name = validateName(params.name);

  const body = (await request.json()) as { name: string; def: MaterialDef };
  if (!body.name) error(400, 'Missing material name');

  const { filePath, levelDef } = readLevel(name);
  if (!levelDef.materials) levelDef.materials = {};
  levelDef.materials[body.name] = body.def;

  writeLevel(filePath, levelDef);
  return new Response(null, { status: 204 });
};

/** Delete a material by name */
export const DELETE: RequestHandler = async ({ params, request }) => {
  guardDev();
  const name = validateName(params.name);

  const { name: matName } = (await request.json()) as { name: string };
  if (!matName) error(400, 'Missing material name');

  const { filePath, levelDef } = readLevel(name);
  if (levelDef.materials) {
    delete levelDef.materials[matName];
  }

  writeLevel(filePath, levelDef);
  return new Response(null, { status: 204 });
};
