import {
  APIError,
  getComposition,
  getCompositionLatest,
  getCompositionVersion,
  type Composition,
  type CompositionVersion,
} from 'src/geoscript/geotoyAPIClient';

import type { PageServerLoad } from './$types';
import { error } from '@sveltejs/kit';

export const load: PageServerLoad = async ({
  fetch,
  params,
  cookies,
  url,
}): Promise<{ comp: Composition; version: CompositionVersion }> => {
  const id = parseInt(params.id, 10);
  if (isNaN(id)) {
    throw new Error('Invalid composition ID');
  }

  const versionIDRaw = url.searchParams.get('version_id');
  let versionID: number | undefined;
  if (typeof versionIDRaw == 'string' && versionIDRaw.length > 0) {
    versionID = parseInt(versionIDRaw, 10);
    if (isNaN(versionID)) {
      error(400, 'Invalid version ID');
    }
  }
  const adminToken = url.searchParams.get('admin_token') ?? undefined;

  const sessionID = cookies.get('session_id');
  const [comp, version] = await Promise.allSettled([
    getComposition(id, fetch, sessionID, adminToken),
    typeof versionID === 'number'
      ? getCompositionVersion(id, versionID, fetch, sessionID, adminToken)
      : getCompositionLatest(id, fetch, sessionID, adminToken),
  ]);

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
      error(500, `Failed to load latest version: ${version.reason}`);
    }
  }

  return { comp: comp.value, version: version.value };
};
