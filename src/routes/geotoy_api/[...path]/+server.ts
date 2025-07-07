import { type RequestHandler } from '@sveltejs/kit';

import { GEOTOY_API_BASE_URL } from 'src/geoscript/geotoyAPIClient';

export const fallback: RequestHandler = async ({ request, fetch, params }) => {
  const path = params.path;
  const hostname = GEOTOY_API_BASE_URL.replace(/^https?:\/\//, '');
  const protocol = GEOTOY_API_BASE_URL.startsWith('https') ? 'https:' : 'http:';
  const url = new URL(path ?? '', `${protocol}//${hostname}`);

  const originalUrl = new URL(request.url);
  for (const [key, value] of originalUrl.searchParams.entries()) {
    url.searchParams.append(key, value);
  }
  if (originalUrl.hash) {
    url.hash = originalUrl.hash;
  }

  const fetchOptions: RequestInit = {
    method: request.method,
    headers: request.headers,
    body: request.method === 'GET' ? null : request.body,
    ...(request.method !== 'GET' ? { duplex: 'half' } : {}),
  };

  const response = await fetch(url.toString(), fetchOptions);

  return new Response(response.body, {
    status: response.status,
    headers: response.headers,
  });
};
