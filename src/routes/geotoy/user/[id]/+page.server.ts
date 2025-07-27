import { error } from '@sveltejs/kit';

import {
  APIError,
  getUser,
  listMyCompositions,
  listPublicCompositions,
  type CompositionAndVersion,
  type User,
} from 'src/geoscript/geotoyAPIClient';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({
  fetch,
  params,
  parent,
  cookies,
}): Promise<{ user: User; compositions: CompositionAndVersion[]; isMe: boolean }> => {
  const id = parseInt(params.id, 10);
  if (isNaN(id)) {
    error(400, 'Invalid user ID');
  }

  try {
    const [user, { me }] = await Promise.all([getUser(id, fetch), parent()]);
    const sessionID = cookies.get('session_id');

    const isMe = !!sessionID && user.id === me?.id;
    const compositions = await (() => {
      if (isMe) {
        return listMyCompositions(sessionID, fetch);
      } else {
        return listPublicCompositions(
          { featuredOnly: false, count: 100000, includeCode: false, userID: user.id },
          fetch
        );
      }
    })();

    return { user, compositions, isMe };
  } catch (err) {
    if (err instanceof APIError) {
      error(err.status, err.message);
    } else {
      console.error('Unexpected error fetching user:', err);
      error(500, 'Internal server error');
    }
  }
};
