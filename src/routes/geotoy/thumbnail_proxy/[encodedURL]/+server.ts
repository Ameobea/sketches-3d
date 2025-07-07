import { error, type RequestHandler } from '@sveltejs/kit';

export const fallback: RequestHandler = async ({ fetch, params }) => {
  const path = params.encodedURL || '';
  const decodedPath = atob(path);
  const url = new URL(decodedPath);
  if (url.protocol !== 'https:' || !url.hostname.endsWith('.r2.dev')) {
    error(400, 'Invalid URL');
  }

  return fetch(url.toString());
};
