import {
  APIError,
  getComposition,
  getCompositionLatest,
  getMaterial,
  type Composition,
  type CompositionVersion,
} from 'src/geoscript/geotoyAPIClient';

import type { PageServerLoad } from './$types';
import { error } from '@sveltejs/kit';
import type { MaterialDescriptor } from 'src/geoscript/materials';

const DEMO_COMPOSITION_ID = 63;

export const load: PageServerLoad = async ({
  fetch,
  params,
  url,
}): Promise<{ mat: MaterialDescriptor; comp: Composition; version: CompositionVersion }> => {
  const id = parseInt(params.id, 10);
  if (isNaN(id)) {
    throw new Error('Invalid material ID');
  }

  const adminToken = url.searchParams.get('admin_token') ?? undefined;

  const [materialDef, comp, version] = await Promise.allSettled([
    getMaterial(id, fetch, adminToken),
    getComposition(DEMO_COMPOSITION_ID, fetch, adminToken),
    getCompositionLatest(DEMO_COMPOSITION_ID, fetch, adminToken),
  ]);

  if (materialDef.status === 'rejected') {
    if (materialDef.reason instanceof APIError) {
      error(materialDef.reason.status, materialDef.reason.message);
    } else {
      error(500, `Failed to load material: ${materialDef.reason}`);
    }
  }
  if (comp.status === 'rejected') {
    if (comp.reason instanceof APIError) {
      error(comp.reason.status, comp.reason.message);
    } else {
      error(500, `Failed to load composition: ${comp.reason}`);
    }
  }
  if (version.status === 'rejected') {
    if (version.reason instanceof APIError) {
      error(version.reason.status, version.reason.message);
    } else {
      error(500, `Failed to load composition version: ${version.reason}`);
    }
  }

  version.value.metadata.materials = {
    defaultMaterialID: 'default',
    materials: {
      default: materialDef.value.materialDefinition,
    },
  };

  return { mat: materialDef.value, comp: comp.value, version: version.value };
};
