import { error, json, type RequestHandler } from '@sveltejs/kit';

import { loadLevelData } from 'src/viz/levelDef/loadLevelData.server';
import type { ObjectDef, ObjectGroupDef } from 'src/viz/levelDef/types';
import {
  findNodeById,
  findNodeWithParent,
  flattenAllNodes,
  flattenLeaves,
  GENERATED_NODE_USERDATA_KEY,
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

/** Update the transform, material, and/or id of an existing object or group. */
export const PATCH: RequestHandler = async ({ params, request }) => {
  guardDev();
  const name = validateName(params.name);

  const body = (await request.json()) as {
    id: string;
    /** If provided, rename the node. Conflicts are resolved by appending `_N`. */
    newId?: string;
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

  if (body.newId !== undefined && body.newId !== body.id) {
    const allIds = new Set(flattenAllNodes(level.def.objects).map(n => n.id));
    allIds.delete(body.id); // the current node's own id is not a conflict
    let resolvedId = body.newId;
    if (allIds.has(resolvedId)) {
      const prefix = `${resolvedId}_`;
      let n = 1;
      while (allIds.has(`${prefix}${n}`)) n++;
      resolvedId = `${prefix}${n}`;
    }
    objDef.id = resolvedId;
    level.save();
    return json({ resolvedId });
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
      }
    | {
        /** Paste a full group subtree (copy-paste). Server assigns fresh IDs. */
        type: 'group_paste';
        def: ObjectGroupDef;
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

  if (body.type === 'group_paste') {
    // Validate all referenced assets exist
    for (const leaf of flattenLeaves([body.def])) {
      if (!level.def.assets[leaf.asset]) error(400, `Unknown asset "${leaf.asset}" in pasted group`);
    }

    // Assign fresh IDs to every node in the subtree. We build the set of used IDs
    // incrementally so siblings within the pasted tree don't collide with each other.
    const allIds = new Set(flattenAllNodes(level.def.objects).map(n => n.id));

    const withFreshIds = (node: ObjectDef | ObjectGroupDef): ObjectDef | ObjectGroupDef => {
      const stem = isObjectGroup(node) ? 'group' : (node as ObjectDef).asset;
      const prefix = `${stem}_`;
      let maxN = -1;
      for (const id of allIds) {
        if (id === stem || id.startsWith(prefix)) {
          const suffix = id === stem ? 0 : parseInt(id.slice(prefix.length), 10);
          if (!isNaN(suffix) && suffix > maxN) maxN = suffix;
        }
      }
      const freshId = maxN < 0 ? stem : `${prefix}${maxN + 1}`;
      allIds.add(freshId);

      // Strip the generated marker so the copy is a regular, editable node.
      const { [GENERATED_NODE_USERDATA_KEY]: _drop, ...restUserData } = node.userData ?? {};
      const userData = Object.keys(restUserData).length > 0 ? restUserData : undefined;

      if (isObjectGroup(node)) {
        return {
          ...node,
          id: freshId,
          userData,
          children: node.children.map(withFreshIds),
        } as ObjectGroupDef;
      }
      return { ...node, id: freshId, userData } as ObjectDef;
    };

    const newDef = withFreshIds(body.def) as ObjectGroupDef;
    level.def.objects.push(newDef);
    level.save();
    return json(newDef, { status: 201 });
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

/**
 * Group multiple sibling nodes into a new parent group.
 *
 * Body: { nodeIds: string[], position: [number, number, number] }
 *
 * All nodes must be siblings (same parent array). The new group is inserted
 * where the first listed node was, and each node's local position is adjusted
 * to preserve world-space placement.
 */
export const PUT: RequestHandler = async ({ params, request }) => {
  guardDev();
  const name = validateName(params.name);

  const body = (await request.json()) as {
    nodeIds: string[];
    position: [number, number, number];
  };

  if (!body.nodeIds || body.nodeIds.length < 2) {
    error(400, 'Need at least 2 node IDs to group');
  }

  const level = openLevel(name);

  // Locate all nodes and verify they share the same parent array
  const lookups = body.nodeIds.map(id => findNodeWithParent(level.def.objects, id));
  for (let i = 0; i < lookups.length; i++) {
    if (!lookups[i]) error(404, `Node "${body.nodeIds[i]}" not found`);
  }

  const parentArray = lookups[0]!.parentArray;
  for (const lookup of lookups) {
    if (lookup!.parentArray !== parentArray) {
      error(400, 'All nodes must be siblings (same parent) to be grouped');
    }
  }

  // Generate a unique group ID
  const allNodes = flattenAllNodes(level.def.objects);
  const allIds = new Set(allNodes.map(n => n.id));
  let groupId = 'group';
  if (allIds.has(groupId)) {
    let n = 1;
    while (allIds.has(`group_${n}`)) n++;
    groupId = `group_${n}`;
  }

  // Compute the insertion index (where the first selected node was)
  const firstIdx = parentArray.indexOf(lookups[0]!.node);

  // Remove nodes from parent array (iterate in reverse to preserve indices)
  const nodesToGroup = lookups.map(l => l!.node);
  const idsToRemove = new Set(body.nodeIds);
  for (let i = parentArray.length - 1; i >= 0; i--) {
    if (idsToRemove.has(parentArray[i].id)) {
      parentArray.splice(i, 1);
    }
  }

  // Adjust each node's position to be relative to the new group position
  const round = (n: number) => Math.round(n * 10000) / 10000;
  for (const node of nodesToGroup) {
    const oldPos = node.position ?? [0, 0, 0];
    node.position = [
      round(oldPos[0] - body.position[0]),
      round(oldPos[1] - body.position[1]),
      round(oldPos[2] - body.position[2]),
    ];
  }

  // Create the new group
  const newGroup = {
    id: groupId,
    children: nodesToGroup,
    position: body.position,
    rotation: [0, 0, 0] as [number, number, number],
    scale: [1, 1, 1] as [number, number, number],
  };

  // Insert where the first node was
  const insertIdx = Math.min(firstIdx, parentArray.length);
  parentArray.splice(insertIdx, 0, newGroup);

  level.save();
  return json(newGroup, { status: 201 });
};
