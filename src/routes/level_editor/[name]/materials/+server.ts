import { error, json, type RequestHandler } from '@sveltejs/kit';

import { getLevelDir } from 'src/viz/levelDef/levelPaths.server';
import {
  LIBRARY_MATERIAL_PREFIX,
  libraryMaterialExists,
  resolveLibraryMaterial,
} from 'src/viz/levelDef/libraryMaterials.server';
import { externalizeShaderFiles } from 'src/viz/levelDef/shaderFiles.server';
import type { MaterialDef } from 'src/viz/levelDef/types';
import { guardDev, openLevel, validateName } from '../../levelEditorUtils.server';

/** Resolve a shared-library material ref into a build-ready def + its textures (read-only). */
export const POST: RequestHandler = async ({ request }) => {
  guardDev();
  const { libRef } = (await request.json()) as { libRef: string };
  if (!libRef?.startsWith(LIBRARY_MATERIAL_PREFIX)) error(400, 'libRef must be a library material path');
  if (!libraryMaterialExists(libRef)) error(404, `Library material not found: ${libRef}`);
  return json(await resolveLibraryMaterial(libRef));
};

/** Upsert (create or full replace) a material */
export const PUT: RequestHandler = async ({ params, request }) => {
  guardDev();
  const name = validateName(params.name);

  const body = (await request.json()) as { name: string; def: MaterialDef };
  if (!body.name) error(400, 'Missing material name');
  if (body.name.startsWith('__ASSETS__/')) {
    error(400, 'Library-prefixed materials are read-only; edit the file under src/assets/materials/ instead');
  }

  const level = openLevel(name);
  if (!level.def.materials) level.def.materials = {};
  // Re-attach the prior `{ file }` shader refs so GLSL isn't inlined into materials.json.
  const prevRaw = level.def.materials[body.name];
  level.def.materials[body.name] = externalizeShaderFiles(body.def, prevRaw, getLevelDir(name));

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
