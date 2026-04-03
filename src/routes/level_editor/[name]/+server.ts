import { error, json, type RequestHandler } from '@sveltejs/kit';

import { loadLevelData } from 'src/viz/levelDef/loadLevelData.server';
import type { ObjectDef } from 'src/viz/levelDef/types';
import {
  findNodeById,
  flattenAllNodes,
  isGeneratedDef,
  isObjectGroup,
  removeNodeById,
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

/** Delete an object or group by id */
export const DELETE: RequestHandler = async ({ params, request }) => {
  guardDev();
  const name = validateName(params.name);

  const { id } = (await request.json()) as { id: string };
  const level = openLevel(name);

  const { removed, nodes: updatedObjects } = removeNodeById(level.def.objects, id);
  if (!removed) {
    await failIfGeneratedNode(name, id);
    error(404, `Object "${id}" not found in level "${name}"`);
  }
  level.def.objects = updatedObjects;
  level.save();
  return new Response(null, { status: 204 });
};

/** Add a new object or group; returns the created def as JSON */
export const POST: RequestHandler = async ({ params, request }) => {
  guardDev();
  const name = validateName(params.name);

  const body = (await request.json()) as
    | {
        type?: 'object';
        asset: string;
        material?: string;
        position: [number, number, number];
        rotation?: [number, number, number];
        scale?: [number, number, number];
        /** If provided (undo/redo restore), use this id exactly instead of generating one. */
        id?: string;
      }
    | {
        type: 'group';
        position: [number, number, number];
        rotation?: [number, number, number];
        scale?: [number, number, number];
        id?: string;
      };

  const level = openLevel(name);
  const allNodes = flattenAllNodes(level.def.objects);

  const resolveId = async (stem: string, requestedId?: string): Promise<string> => {
    if (requestedId) {
      const mergedLevel = await loadLevelData(name);
      if (findNodeById(mergedLevel.objects, requestedId)) {
        error(409, `Node "${requestedId}" already exists in level "${name}"`);
      }
      if (allNodes.some(n => n.id === requestedId)) {
        error(409, `Node "${requestedId}" already exists in level "${name}"`);
      }
      return requestedId;
    }
    const prefix = `${stem}_`;
    let maxN = -1;
    for (const n of allNodes) {
      if (n.id === stem || n.id.startsWith(prefix)) {
        const suffix = n.id === stem ? 0 : parseInt(n.id.slice(prefix.length), 10);
        if (!isNaN(suffix) && suffix > maxN) maxN = suffix;
      }
    }
    return maxN < 0 ? stem : `${prefix}${maxN + 1}`;
  };

  if (body.type === 'group') {
    const id = await resolveId('group', body.id);
    const newGroup = {
      id,
      children: [] as import('src/viz/levelDef/types').ObjectGroupDef['children'],
      position: body.position,
      rotation: body.rotation ?? ([0, 0, 0] as [number, number, number]),
      scale: body.scale ?? ([1, 1, 1] as [number, number, number]),
    };
    level.def.objects.push(newGroup);
    level.save();
    return json(newGroup, { status: 201 });
  }

  // Default: create ObjectDef
  if (!level.def.assets[body.asset]) {
    error(400, `Unknown asset "${body.asset}"`);
  }
  if (body.material && !level.def.materials?.[body.material]) {
    error(400, `Unknown material "${body.material}"`);
  }

  const id = await resolveId(body.asset, body.id);

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
