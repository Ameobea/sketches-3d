import { error } from '@sveltejs/kit';
import type { RequestHandler } from '@sveltejs/kit';

import { guardDev, openLevel, validateName } from '../../levelEditorUtils.server';

/**
 * Sets a geotoy composition asset's `materialMap` (geotoy material name → level material id).
 * An empty map removes the field. Dev only.
 *
 * Body: `{ assetId: string, materialMap: Record<string, string> }`
 */
export const PATCH: RequestHandler = async ({ params, request }) => {
  guardDev();
  const name = validateName(params.name);
  const { assetId, materialMap } = (await request.json()) as {
    assetId: string;
    materialMap: Record<string, string>;
  };

  const store = openLevel(name);
  const asset = store.def.assets?.[assetId];
  if (!asset || asset.type !== 'geotoyComposition') {
    error(404, `geotoyComposition asset "${assetId}" not found`);
  }

  if (materialMap && Object.keys(materialMap).length > 0) {
    (asset as Record<string, unknown>).materialMap = materialMap;
  } else {
    delete (asset as Record<string, unknown>).materialMap;
  }

  store.save();
  return new Response(null, { status: 204 });
};
