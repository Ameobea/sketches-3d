import type { RequestHandler } from '@sveltejs/kit';

import type { GeoscriptAssetMeta } from 'src/viz/levelDef/types';
import { guardDev, openLevel, validateName } from '../../levelEditorUtils.server';

/** Body: `{ [assetId]: GeoscriptAssetMeta | null }`; null removes the entry. */
export const POST: RequestHandler = async ({ params, request }) => {
  guardDev();
  const name = validateName(params.name);
  const updates = (await request.json()) as Record<string, GeoscriptAssetMeta | null>;
  const store = openLevel(name);

  for (const [id, meta] of Object.entries(updates)) {
    const asset = store.def.assets?.[id];
    if (!asset || asset.type === 'gltf') {
      continue;
    }
    if (meta === null) {
      delete (asset as Record<string, unknown>)._meta;
    } else {
      (asset as Record<string, unknown>)._meta = meta;
    }
  }

  store.saveMeta();
  return new Response(null, { status: 204 });
};
