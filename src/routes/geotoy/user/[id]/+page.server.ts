import { error } from '@sveltejs/kit';

import { APIError, getUser, type User } from 'src/geoscript/geoscriptAPIClient';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, params }): Promise<{ user: User }> => {
  const id = parseInt(params.id, 10);
  if (isNaN(id)) {
    error(400, 'Invalid user ID');
  }

  try {
    const user = await getUser(id, fetch);
    // TODO: Also fetch public compositions for user
    return { user };
  } catch (err) {
    if (err instanceof APIError) {
      error(err.status, err.message);
    } else {
      console.error('Unexpected error fetching user:', err);
      error(500, 'Internal server error');
    }
  }
};
