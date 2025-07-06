import { me, type User } from 'src/geoscript/geoscriptAPIClient';

import type { LayoutServerLoad } from './$types';

export const load: LayoutServerLoad = async ({ fetch, cookies }): Promise<{ me: User | null }> => {
  const sessionID = cookies.get('session_id');
  return { me: sessionID ? await me(fetch, sessionID).catch(() => null) : null };
};
