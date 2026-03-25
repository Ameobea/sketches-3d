import { error, json, type RequestHandler } from '@sveltejs/kit';

import type { ObjectDef } from 'src/viz/levelDef/types';
import { guardDev, validateName, readLevel, writeLevel } from '../levelEditorUtils.server';

/** Update the transform of an existing object */
export const PATCH: RequestHandler = async ({ params, request }) => {
  guardDev();
  const name = validateName(params.name);

  const body = (await request.json()) as {
    id: string;
    position?: [number, number, number];
    rotation?: [number, number, number];
    scale?: [number, number, number];
    /** Pass a string to assign, null to remove. Omit to leave unchanged. */
    material?: string | null;
  };

  const { filePath, levelDef } = readLevel(name);
  const objDef = levelDef.objects.find((o: ObjectDef) => o.id === body.id);
  if (!objDef) error(404, `Object "${body.id}" not found in level "${name}"`);

  if (body.position !== undefined) objDef.position = body.position;
  if (body.rotation !== undefined) objDef.rotation = body.rotation;
  if (body.scale !== undefined) objDef.scale = body.scale;
  if ('material' in body) {
    if (body.material) {
      objDef.material = body.material;
    } else {
      delete objDef.material;
    }
  }

  writeLevel(filePath, levelDef);
  return new Response(null, { status: 204 });
};

/** Delete an object by id */
export const DELETE: RequestHandler = async ({ params, request }) => {
  guardDev();
  const name = validateName(params.name);

  const { id } = (await request.json()) as { id: string };
  const { filePath, levelDef } = readLevel(name);

  const idx = levelDef.objects.findIndex((o: ObjectDef) => o.id === id);
  if (idx === -1) error(404, `Object "${id}" not found in level "${name}"`);

  levelDef.objects.splice(idx, 1);
  writeLevel(filePath, levelDef);
  return new Response(null, { status: 204 });
};

/** Add a new object; returns the created ObjectDef as JSON */
export const POST: RequestHandler = async ({ params, request }) => {
  guardDev();
  const name = validateName(params.name);

  const body = (await request.json()) as {
    asset: string;
    material?: string;
    position: [number, number, number];
    rotation?: [number, number, number];
    scale?: [number, number, number];
    /** If provided (undo/redo restore), use this id exactly instead of generating one. */
    id?: string;
  };

  const { filePath, levelDef } = readLevel(name);

  if (!levelDef.assets[body.asset]) {
    error(400, `Unknown asset "${body.asset}"`);
  }
  if (body.material && !levelDef.materials?.[body.material]) {
    error(400, `Unknown material "${body.material}"`);
  }

  let id: string;
  if (body.id) {
    if (levelDef.objects.some(o => o.id === body.id)) {
      error(409, `Object "${body.id}" already exists in level "${name}"`);
    }
    id = body.id;
  } else {
    // Generate a unique id: <asset>_<n> where n is one past the highest existing suffix
    const prefix = `${body.asset}_`;
    let maxN = -1;
    for (const o of levelDef.objects) {
      if (o.id === body.asset || o.id.startsWith(prefix)) {
        const suffix = o.id === body.asset ? 0 : parseInt(o.id.slice(prefix.length), 10);
        if (!isNaN(suffix) && suffix > maxN) maxN = suffix;
      }
    }
    id = maxN < 0 ? body.asset : `${prefix}${maxN + 1}`;
  }

  const newObj: ObjectDef = {
    id,
    asset: body.asset,
    position: body.position,
    rotation: body.rotation ?? [0, 0, 0],
    scale: body.scale ?? [1, 1, 1],
    ...(body.material ? { material: body.material } : {}),
  };

  levelDef.objects.push(newObj);
  writeLevel(filePath, levelDef);
  return json(newObj, { status: 201 });
};
