import { error } from '@sveltejs/kit';

import {
  APIError,
  getComposition,
  getCompositionHistory,
  type Composition,
  type CompositionVersion,
} from 'src/geoscript/geotoyAPIClient';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({
  fetch,
  params,
  parent,
  cookies,
}): Promise<{ composition: Composition; versions: CompositionVersion[]; isAuthor: boolean }> => {
  const id = parseInt(params.id, 10);
  if (isNaN(id)) {
    error(400, 'Invalid composition ID');
  }

  try {
    const sessionID = cookies.get('session_id');
    const [composition, versions, { me }] = await Promise.all([
      getComposition(id, fetch, sessionID),
      getCompositionHistory(id, fetch, sessionID),
      parent(),
    ]);

    const isAuthor = !!me && composition.author_id === me.id;

    return { composition, versions, isAuthor };
  } catch (err) {
    if (err instanceof APIError) {
      error(err.status, err.message);
    } else {
      console.error('Unexpected error fetching composition history:', err);
      error(500, 'Internal server error');
    }
  }
};
