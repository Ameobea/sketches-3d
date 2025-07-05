import { listPublicCompositions, type Composition } from 'src/geoscript/geoscriptAPIClient';

import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch }): Promise<{ featuredCompositions: Composition[] }> => {
  const featuredCompositions = await listPublicCompositions({ featuredOnly: true, count: 20 }, fetch);
  return { featuredCompositions };
};
