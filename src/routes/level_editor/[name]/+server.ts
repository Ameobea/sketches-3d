import { error, json, type RequestHandler } from '@sveltejs/kit';

import { loadLevelData } from 'src/viz/levelDef/loadLevelData.server';
import type { ObjectDef } from 'src/viz/levelDef/types';
import {
  findNodeById,
  flattenLeaves,
  isGeneratedDef,
  isObjectGroup,
} from 'src/viz/levelDef/levelDefTreeUtils';
import { guardDev, openLevel, validateName } from '../levelEditorUtils.server';

const failIfGeneratedNode = async (name: string, id: string) => {
  const mergedLevel = await loadLevelData(name);
  const mergedNode = findNodeById(mergedLevel.objects, id);
  if (mergedNode && isGeneratedDef(mergedNode)) {
    error(400, `Generated node "${id}" is read-only in the level editor`);
  }
};

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

  const level = openLevel(name);
  const objDef = findNodeById(level.def.objects, body.id);
  if (!objDef) {
    await failIfGeneratedNode(name, body.id);
    error(404, `Object "${body.id}" not found in level "${name}"`);
  }

  if (body.position !== undefined) objDef.position = body.position;
  if (body.rotation !== undefined) objDef.rotation = body.rotation;
  if (body.scale !== undefined) objDef.scale = body.scale;
  if ('material' in body && !isObjectGroup(objDef)) {
    if (body.material) {
      objDef.material = body.material;
    } else {
      delete objDef.material;
    }
  }

  level.save();
  return new Response(null, { status: 204 });
};

/** Delete an object by id */
export const DELETE: RequestHandler = async ({ params, request }) => {
  guardDev();
  const name = validateName(params.name);

  const { id } = (await request.json()) as { id: string };
  const level = openLevel(name);

  const removeById = (
    nodes: typeof level.def.objects,
    targetId: string
  ): { removed: boolean; nodes: typeof level.def.objects } => {
    const idx = nodes.findIndex(n => n.id === targetId);
    if (idx !== -1) {
      const next = [...nodes];
      next.splice(idx, 1);
      return { removed: true, nodes: next };
    }
    for (let i = 0; i < nodes.length; i++) {
      const node = nodes[i];
      if ('children' in node) {
        const res = removeById(node.children as typeof level.def.objects, targetId);
        if (res.removed) {
          const next = [...nodes];
          next[i] = { ...node, children: res.nodes };
          return { removed: true, nodes: next };
        }
      }
    }
    return { removed: false, nodes };
  };

  const { removed, nodes: updatedObjects } = removeById(level.def.objects, id);
  if (!removed) {
    await failIfGeneratedNode(name, id);
    error(404, `Object "${id}" not found in level "${name}"`);
  }
  level.def.objects = updatedObjects;
  level.save();
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

  const level = openLevel(name);

  if (!level.def.assets[body.asset]) {
    error(400, `Unknown asset "${body.asset}"`);
  }
  if (body.material && !level.def.materials?.[body.material]) {
    error(400, `Unknown material "${body.material}"`);
  }

  let id: string;
  const allNodes = flattenLeaves(level.def.objects);
  if (body.id) {
    const mergedLevel = await loadLevelData(name);
    const mergedNode = findNodeById(mergedLevel.objects, body.id);
    if (mergedNode) {
      error(409, `Object "${body.id}" already exists in level "${name}"`);
    }
    if (allNodes.some(o => o.id === body.id)) {
      error(409, `Object "${body.id}" already exists in level "${name}"`);
    }
    id = body.id;
  } else {
    // Generate a unique id: <asset>_<n> where n is one past the highest existing suffix
    const prefix = `${body.asset}_`;
    let maxN = -1;
    for (const o of allNodes) {
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

  level.def.objects.push(newObj);
  level.save();
  return json(newObj, { status: 201 });
};
