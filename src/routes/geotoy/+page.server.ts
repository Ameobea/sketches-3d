import {
  listPublicCompositions,
  type Composition,
  type CompositionVersion,
} from 'src/geoscript/geotoyAPIClient';

import type { PageServerLoad } from './$types';

const PAGE_SIZE = 20;

export const load: PageServerLoad = async ({
  fetch,
  url,
}): Promise<{
  featuredCompositions: {
    comp: Composition;
    latest: CompositionVersion;
  }[];
  currentPage: number;
  hasMore: boolean;
}> => {
  const pageParam = url.searchParams.get('page');
  const currentPage = pageParam ? Math.max(1, parseInt(pageParam, 10)) : 1;
  const offset = (currentPage - 1) * PAGE_SIZE;

  // Fetch one extra to determine if there are additional pages
  const compositions = await listPublicCompositions(
    { featuredOnly: true, count: PAGE_SIZE + 1, offset, includeCode: false },
    fetch
  );

  const hasMore = compositions.length > PAGE_SIZE;
  const featuredCompositions = hasMore ? compositions.slice(0, PAGE_SIZE) : compositions;

  return { featuredCompositions, currentPage, hasMore };
};
