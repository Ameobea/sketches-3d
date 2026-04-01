import { error, type RequestHandler } from '@sveltejs/kit';

import type { MaterialDef } from 'src/viz/levelDef/types';
import { guardDev, openLevel, validateName } from '../../levelEditorUtils.server';

/** Upsert (create or full replace) a material */
export const PUT: RequestHandler = async ({ params, request }) => {
  guardDev();
  const name = validateName(params.name);

  const body = (await request.json()) as { name: string; def: MaterialDef };
  if (!body.name) error(400, 'Missing material name');

  const level = openLevel(name);
  if (!level.def.materials) level.def.materials = {};
  level.def.materials[body.name] = body.def;

  level.save();
  return new Response(null, { status: 204 });
};

/** Delete a material by name */
export const DELETE: RequestHandler = async ({ params, request }) => {
  guardDev();
  const name = validateName(params.name);

  const { name: matName } = (await request.json()) as { name: string };
  if (!matName) error(400, 'Missing material name');

  const level = openLevel(name);
  if (level.def.materials) {
    delete level.def.materials[matName];
  }

  level.save();
  return new Response(null, { status: 204 });
};
