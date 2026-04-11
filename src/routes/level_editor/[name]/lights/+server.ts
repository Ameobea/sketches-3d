import { error, json, type RequestHandler } from '@sveltejs/kit';

import { LightDefSchema } from 'src/viz/levelDef/types';
import type { LightDef } from 'src/viz/levelDef/types';
import { guardDev, openLevel, validateName } from '../../levelEditorUtils.server';

/** Add a new light; returns the created LightDef as JSON */
export const POST: RequestHandler = async ({ params, request }) => {
  guardDev();
  const name = validateName(params.name);

  const body = (await request.json()) as Partial<LightDef> & { type: LightDef['type']; id?: string };

  const level = openLevel(name);
  if (!level.def.lights) level.def.lights = [];

  const allIds = new Set(level.def.lights.map(l => l.id));
  const stem = body.id ?? body.type ?? 'light';
  const resolveId = (s: string): string => {
    if (!allIds.has(s)) return s;
    const prefix = `${s}_`;
    let n = 1;
    while (allIds.has(`${prefix}${n}`)) n++;
    return `${prefix}${n}`;
  };

  const id = resolveId(stem);
  const candidate = { ...body, id };

  const parseResult = LightDefSchema.safeParse(candidate);
  if (!parseResult.success) {
    error(400, `Invalid light def: ${parseResult.error.message}`);
  }

  level.def.lights.push(parseResult.data);
  level.save();
  return json(parseResult.data, { status: 201 });
};

/** Update light properties (color, intensity, position, etc.) */
export const PATCH: RequestHandler = async ({ params, request }) => {
  guardDev();
  const name = validateName(params.name);

  const body = (await request.json()) as Partial<LightDef> & { id: string };

  const level = openLevel(name);
  const lights = level.def.lights ?? [];
  const idx = lights.findIndex(l => l.id === body.id);
  if (idx === -1) {
    error(404, `Light "${body.id}" not found in level "${name}"`);
  }

  const updated = { ...lights[idx], ...body };
  const parseResult = LightDefSchema.safeParse(updated);
  if (!parseResult.success) {
    error(400, `Invalid light update: ${parseResult.error.message}`);
  }

  level.def.lights![idx] = parseResult.data;
  level.save();
  return new Response(null, { status: 204 });
};

/** Delete a light by id */
export const DELETE: RequestHandler = async ({ params, request }) => {
  guardDev();
  const name = validateName(params.name);

  const { id } = (await request.json()) as { id: string };

  const level = openLevel(name);
  const lights = level.def.lights ?? [];
  const idx = lights.findIndex(l => l.id === id);
  if (idx === -1) {
    error(404, `Light "${id}" not found in level "${name}"`);
  }

  level.def.lights!.splice(idx, 1);
  level.save();
  return new Response(null, { status: 204 });
};
