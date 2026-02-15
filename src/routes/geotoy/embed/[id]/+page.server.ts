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

const BASE_URL = 'https://3d.ameo.design/geotoy_api';

interface EmbedData {
  composition: Composition;
  version: CompositionVersion;
  showTitle: boolean;
  showAuthor: boolean;
  showDescription: boolean;
}

export const load: PageServerLoad = async ({ fetch, params, url }): Promise<EmbedData> => {
  const id = parseInt(params.id, 10);
  if (isNaN(id)) {
    error(400, 'Invalid composition ID');
  }

  const versionParam = url.searchParams.get('version');
  let versionID: number | undefined;
  if (typeof versionParam === 'string' && versionParam.length > 0) {
    versionID = parseInt(versionParam, 10);
    if (isNaN(versionID)) {
      error(400, 'Invalid version number');
    }
  }

  const showTitle = url.searchParams.get('title') === '1';
  const showAuthor = url.searchParams.get('author') === '1';
  const showDescription = url.searchParams.get('description') === '1';

  const [comp, version] = await Promise.allSettled([
    getComposition(id, fetch, undefined, undefined, BASE_URL),
    typeof versionID === 'number'
      ? getCompositionVersion(id, versionID, fetch, undefined, undefined, BASE_URL)
      : getCompositionLatest(id, fetch, undefined, undefined, BASE_URL),
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
      error(500, `Failed to load version: ${version.reason}`);
    }
  }

  return {
    composition: comp.value,
    version: version.value,
    showTitle,
    showAuthor,
    showDescription,
  };
};
