import { error, json, type RequestHandler } from '@sveltejs/kit';

import type { CsgTreeNode, ObjectDef } from 'src/viz/levelDef/types';
import { guardDev, validateName, readLevel, writeLevel } from '../../levelEditorUtils.server';

/** Convert an existing object to a CSG tree (single leaf at identity transform) */
export const POST: RequestHandler = async ({ params, request }) => {
  guardDev();
  const name = validateName(params.name);
  const { objectId } = (await request.json()) as { objectId: string };

  const { filePath, levelDef } = readLevel(name);

  const objDef = levelDef.objects.find((o: ObjectDef) => o.id === objectId);
  if (!objDef) error(404, `Object "${objectId}" not found`);

  const assetDef = levelDef.assets[objDef.asset];
  if (!assetDef || (assetDef.type !== 'geoscript' && (assetDef as any).type !== 'csg')) {
    error(400, `Asset "${objDef.asset}" must be a geoscript or csg asset`);
  }

  // Generate unique CSG asset name
  const baseName = `csg_${objDef.asset}`;
  let csgAssetName = baseName;
  let n = 1;
  while (levelDef.assets[csgAssetName]) {
    csgAssetName = `${baseName}_${n++}`;
  }

  // Create CSG asset with single leaf at identity transform
  levelDef.assets[csgAssetName] = {
    type: 'csg',
    tree: {
      asset: objDef.asset,
    },
  } as any;

  // Update the object to reference the CSG asset
  objDef.asset = csgAssetName;

  writeLevel(filePath, levelDef);
  return json({ csgAssetName, tree: levelDef.assets[csgAssetName] }, { status: 201 });
};

/** Update a CSG asset's tree */
export const PATCH: RequestHandler = async ({ params, request }) => {
  guardDev();
  const name = validateName(params.name);

  const body = (await request.json()) as {
    assetName: string;
    tree: CsgTreeNode;
  };

  const { filePath, levelDef } = readLevel(name);
  const asset = levelDef.assets[body.assetName];
  if (!asset || (asset as any).type !== 'csg') {
    error(404, `CSG asset "${body.assetName}" not found`);
  }

  // Validate all referenced assets exist and are geoscript
  const validateNode = (node: CsgTreeNode) => {
    if ('asset' in node) {
      const refDef = levelDef.assets[node.asset];
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
  writeLevel(filePath, levelDef);
  return new Response(null, { status: 204 });
};
