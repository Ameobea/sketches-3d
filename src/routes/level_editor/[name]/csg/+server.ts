import { error, json, type RequestHandler } from '@sveltejs/kit';

import { loadLevelData } from 'src/viz/levelDef/loadLevelData.server';
import type { CsgTreeNode, ObjectDef, ObjectGroupDef } from 'src/viz/levelDef/types';
import {
  findNodeById,
  findNodeWithParent,
  isGeneratedDef,
  isObjectGroup,
} from 'src/viz/levelDef/levelDefTreeUtils';
import { guardDev, openLevel, validateName } from '../../levelEditorUtils.server';

const buildLeafForObj = (objDef: ObjectDef, position?: [number, number, number]): Record<string, unknown> => {
  const leaf: Record<string, unknown> = { asset: objDef.asset };
  if (position && position.some(v => v !== 0)) leaf.position = position;
  if (objDef.rotation && objDef.rotation.some(v => v !== 0)) leaf.rotation = objDef.rotation;
  if (objDef.scale && objDef.scale.some((v, i) => v !== [1, 1, 1][i])) leaf.scale = objDef.scale;
  return leaf;
};

/**
 * Convert one or more existing objects to a CSG asset.
 *
 * Single-object: the new CSG tree is a single leaf carrying the object's
 * rotation + scale (translation stays on the level object).
 *
 * Multi-object: requires all selected objects to be sibling leaves whose
 * assets are geoscript or csg. The first object becomes the CSG-converted
 * level object (its position is the anchor); the others are deleted. The
 * tree is a union op whose children are leaf nodes carrying each input's
 * rotation, scale, and position relative to the anchor.
 */
export const POST: RequestHandler = async ({ params, request }) => {
  guardDev();
  const name = validateName(params.name);
  const body = (await request.json()) as { objectId?: string; objectIds?: string[] };
  const objectIds: string[] = body.objectIds ?? (body.objectId ? [body.objectId] : []);
  if (objectIds.length === 0) error(400, 'objectId or objectIds is required');

  const level = openLevel(name);

  // Resolve all objects, validating each is a non-generated leaf with a
  // geoscript/csg asset.
  type Resolved = {
    id: string;
    def: ObjectDef;
    parentArray: (ObjectDef | ObjectGroupDef)[];
    index: number;
  };
  const resolved: Resolved[] = [];
  for (const id of objectIds) {
    const found = findNodeWithParent(level.def.objects, id);
    if (!found) {
      const mergedLevel = await loadLevelData(name);
      const mergedNode = findNodeById(mergedLevel.objects, id);
      if (mergedNode && isGeneratedDef(mergedNode)) {
        error(400, `Generated node "${id}" is read-only in the level editor`);
      }
      error(404, `Object "${id}" not found`);
    }
    if (isObjectGroup(found.node)) error(400, `Object "${id}" is a group, not a leaf object`);
    const objDef = found.node;
    if (objDef.asset === undefined)
      error(400, `Object "${id}" is a marker with no asset; cannot convert to CSG`);
    const assetDef = level.def.assets[objDef.asset];
    if (!assetDef || (assetDef.type !== 'geoscript' && (assetDef as any).type !== 'csg')) {
      error(400, `Asset "${objDef.asset}" must be a geoscript or csg asset`);
    }
    resolved.push({ id, def: objDef, parentArray: found.parentArray, index: found.index });
  }

  // For multi: enforce all share the same parent.
  if (resolved.length > 1) {
    const parentArray = resolved[0].parentArray;
    if (!resolved.every(r => r.parentArray === parentArray)) {
      error(400, 'All selected objects must be siblings (share the same parent)');
    }
  }

  const primary = resolved[0];
  const primaryDef = primary.def;
  if (primaryDef.asset === undefined) error(400, 'Primary object is a marker with no asset');

  // Generate unique CSG asset name based on primary object's asset. The asset
  // ID may include slash-separated subdirs (e.g. `dir/leaf`); use only the
  // basename so the generated CSG asset id is a clean identifier.
  const lastSlash = primaryDef.asset.lastIndexOf('/');
  const assetStem = lastSlash === -1 ? primaryDef.asset : primaryDef.asset.slice(lastSlash + 1);
  const baseName = `csg_${assetStem}`;
  let csgAssetName = baseName;
  let n = 1;
  while (level.def.assets[csgAssetName]) {
    csgAssetName = `${baseName}_${n++}`;
  }

  let tree: Record<string, unknown>;
  if (resolved.length === 1) {
    tree = buildLeafForObj(primaryDef);
  } else {
    // Anchor at the primary object's position; each child leaf gets a position
    // relative to the anchor (so world placement is preserved when the CSG-
    // converted primary stays at its original position).
    const anchor = primaryDef.position ?? [0, 0, 0];
    const children = resolved.map(r => {
      const pos = r.def.position ?? [0, 0, 0];
      const rel: [number, number, number] = [pos[0] - anchor[0], pos[1] - anchor[1], pos[2] - anchor[2]];
      return buildLeafForObj(r.def, rel);
    });
    tree = { op: 'union', children };
  }

  level.def.assets[csgAssetName] = { type: 'csg', tree } as any;

  // Replace the primary object's asset reference, stripping rotation + scale
  // (they've been baked into the tree). Position stays on the level object.
  primaryDef.asset = csgAssetName;
  delete primaryDef.rotation;
  delete primaryDef.scale;

  // Remove the non-primary objects from their (shared) parent array. Splice in
  // descending index order so earlier indices remain valid.
  const deletedIds: string[] = [];
  if (resolved.length > 1) {
    const toRemove = resolved
      .slice(1)
      .map(r => ({ index: r.index, id: r.id }))
      .sort((a, b) => b.index - a.index);
    for (const { index, id } of toRemove) {
      primary.parentArray.splice(index, 1);
      deletedIds.push(id);
    }
  }

  level.save();
  return json({ csgAssetName, tree, primaryId: primary.id, deletedIds }, { status: 201 });
};

/** Update a CSG asset's tree */
export const PATCH: RequestHandler = async ({ params, request }) => {
  guardDev();
  const name = validateName(params.name);

  const body = (await request.json()) as {
    assetName: string;
    tree: CsgTreeNode;
  };

  const level = openLevel(name);
  const asset = level.def.assets[body.assetName];
  if (!asset || (asset as any).type !== 'csg') {
    error(404, `CSG asset "${body.assetName}" not found`);
  }

  // Validate all referenced assets exist and are geoscript
  const validateNode = (node: CsgTreeNode) => {
    if ('asset' in node) {
      const refDef = level.def.assets[node.asset];
      if (!refDef) error(400, `CSG leaf references unknown asset "${node.asset}"`);
      if (refDef.type !== 'geoscript' && (refDef as any).type !== 'csg') {
        error(400, `CSG leaf must reference a geoscript or csg asset, got "${refDef.type}"`);
      }
    } else {
      for (const child of node.children) {
        validateNode(child);
      }
    }
  };
  validateNode(body.tree);

  (asset as any).tree = body.tree;
  level.save();
  return new Response(null, { status: 204 });
};
