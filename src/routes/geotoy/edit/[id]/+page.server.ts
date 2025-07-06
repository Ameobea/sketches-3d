import {
  APIError,
  getComposition,
  getCompositionLatest,
  getCompositionVersion,
  type Composition,
  type CompositionVersion,
} from 'src/geoscript/geoscriptAPIClient';

import type { PageServerLoad } from './$types';
import { error } from '@sveltejs/kit';

export const load: PageServerLoad = async ({
  fetch,
  params,
  cookies,
  url,
}): Promise<{ comp: Composition; latest: CompositionVersion }> => {
  const id = parseInt(params.id, 10);
  if (isNaN(id)) {
    throw new Error('Invalid composition ID');
  }

  const versionID = url.searchParams.get('version_id');
  const adminToken = url.searchParams.get('admin_token');

  const sessionID = cookies.get('session_id');
  const [comp, version] = await Promise.allSettled([
    getComposition(id, fetch, sessionID, adminToken),
    versionID
      ? getCompositionVersion(id, +versionID, fetch, sessionID, adminToken)
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

  return { comp: comp.value, latest: latest.value };
};
