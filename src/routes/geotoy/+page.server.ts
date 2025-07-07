import {
  listPublicCompositions,
  type Composition,
  type CompositionVersion,
} from 'src/geoscript/geotoyAPIClient';

import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({
  fetch,
}): Promise<{
  featuredCompositions: {
    comp: Composition;
    latest: CompositionVersion;
  }[];
}> => {
  const featuredCompositions = await listPublicCompositions({ featuredOnly: true, count: 20 }, fetch);
  return { featuredCompositions };
};
