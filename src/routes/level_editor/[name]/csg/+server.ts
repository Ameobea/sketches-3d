import { error, json, type RequestHandler } from '@sveltejs/kit';

import { loadLevelData } from 'src/viz/levelDef/loadLevelData.server';
import type { CsgTreeNode } from 'src/viz/levelDef/types';
import { findNodeById, isGeneratedDef, isObjectGroup } from 'src/viz/levelDef/levelDefTreeUtils';
import { guardDev, openLevel, validateName } from '../../levelEditorUtils.server';

/** Convert an existing object to a CSG tree (single leaf at identity transform) */
export const POST: RequestHandler = async ({ params, request }) => {
  guardDev();
  const name = validateName(params.name);
  const { objectId } = (await request.json()) as { objectId: string };

  const level = openLevel(name);

  const found = findNodeById(level.def.objects, objectId);
  if (!found) {
    const mergedLevel = await loadLevelData(name);
    const mergedNode = findNodeById(mergedLevel.objects, objectId);
    if (mergedNode && isGeneratedDef(mergedNode)) {
      error(400, `Generated node "${objectId}" is read-only in the level editor`);
    }
    error(404, `Object "${objectId}" not found`);
  }
  if (isObjectGroup(found)) error(400, `Object "${objectId}" is a group, not a leaf object`);
  const objDef = found;

  const assetDef = level.def.assets[objDef.asset];
  if (!assetDef || (assetDef.type !== 'geoscript' && (assetDef as any).type !== 'csg')) {
    error(400, `Asset "${objDef.asset}" must be a geoscript or csg asset`);
  }

  // Generate unique CSG asset name
  const baseName = `csg_${objDef.asset}`;
  let csgAssetName = baseName;
  let n = 1;
  while (level.def.assets[csgAssetName]) {
    csgAssetName = `${baseName}_${n++}`;
  }

  // Create CSG asset with single leaf at identity transform
  level.def.assets[csgAssetName] = {
    type: 'csg',
    tree: {
      asset: objDef.asset,
    },
  } as any;

  // Update the object to reference the CSG asset
  objDef.asset = csgAssetName;

  level.save();
  return json({ csgAssetName, tree: level.def.assets[csgAssetName] }, { status: 201 });
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
