import type { MaterialDefinitions } from './materials';

export const GEOTOY_API_BASE_URL = import.meta.env.VITE_GEOSCRIPT_API_URL || 'http://localhost:5810';

const INTERNAL_PROXY_GEOTOY_API_BASE_URL = '/geotoy_api';

export interface User {
  id: number;
  username: string;
}

export interface Registration {
  username: string;
  password: string;
}

export interface Login {
  username: string;
  password: string;
}

export interface Composition {
  id: number;
  author_id: number;
  author_username: string;
  title: string;
  description: string;
  forked_from_id?: number | null;
  created_at: string;
  updated_at: string;
  is_shared: boolean;
  is_featured: boolean;
}

export interface CompositionVersionMetadata {
  view: {
    cameraPosition: [number, number, number];
    target: [number, number, number];
    fov?: number; // for `PerspectiveCamera`
    zoom?: number; // for `OrthographicCamera`
  };
  materials?: MaterialDefinitions;
}

export interface CompositionVersion {
  id: number;
  composition_id: number;
  source_code: string;
  created_at: string;
  metadata: CompositionVersionMetadata;
  thumbnail_url?: string | null;
}

export interface CreateComposition {
  title: string;
  description: string;
  source_code: string;
  is_shared: boolean;
  metadata: CompositionVersionMetadata;
}

export interface CreateCompositionVersion {
  source_code: string;
  metadata: CompositionVersionMetadata;
}

export class APIError extends Error {
  public status: number;
  public message: string;

  constructor(status: number, message: string) {
    super(`API error: ${status} ${message}`);
    this.status = status;
    this.message = message;
    Object.setPrototypeOf(this, new.target.prototype);
  }
}

const apiFetch = async <T>(
  path: string,
  options: RequestInit = {},
  fetch: typeof globalThis.fetch = globalThis.fetch,
  binary = false,
  baseUrl = INTERNAL_PROXY_GEOTOY_API_BASE_URL
): Promise<T> => {
  const res = await fetch(`${baseUrl}${path}`, {
    ...options,
    credentials: 'include',
    headers: {
      ...(binary ? {} : { 'Content-Type': 'application/json' }),
      ...(options.headers || {}),
    },
  });
  if (!res.ok) {
    const text = await res.text();
    throw new APIError(res.status, text || 'Unknown error');
  }

  if (res.status === 204) {
    return undefined as unknown as T;
  }

  if (binary) {
    const arrayBuffer = await res.arrayBuffer();
    return new Uint8Array(arrayBuffer) as unknown as T;
  }

  const contentType = res.headers.get('Content-Type');
  if (contentType && contentType !== 'application/json') {
    return res.text() as unknown as T;
  }

  return res.json();
};

export const register = (data: Registration): Promise<User> =>
  apiFetch<User>('/users/register', {
    method: 'POST',
    body: JSON.stringify(data),
  });

export const login = (data: Login): Promise<User> =>
  apiFetch<User>('/users/login', {
    method: 'POST',
    body: JSON.stringify(data),
  });

export const logout = (): Promise<void> => apiFetch<void>('/users/logout', { method: 'POST' });

export const me = (fetch: typeof globalThis.fetch = globalThis.fetch, sessionID?: string): Promise<User> =>
  apiFetch<User>('/users/me', sessionID ? { headers: { session_id: sessionID } } : {}, fetch);

export const getUser = (id: number, fetch: typeof globalThis.fetch = globalThis.fetch): Promise<User> =>
  apiFetch<User>(`/users/user/${id}`, {}, fetch);

export const createComposition = (data: CreateComposition): Promise<Composition> =>
  apiFetch<Composition>('/compositions/', {
    method: 'POST',
    body: JSON.stringify(data),
  });

export const listPublicCompositions = (
  {
    featuredOnly,
    count = 20,
    includeCode = false,
  }: {
    featuredOnly?: boolean;
    count?: number;
    includeCode?: boolean;
  },
  fetch: typeof globalThis.fetch = globalThis.fetch
): Promise<{ comp: Composition; latest: CompositionVersion }[]> => {
  const params = new URLSearchParams();
  if (featuredOnly) {
    params.set('featured_only', 'true');
  }
  if (count) {
    params.set('count', count.toString());
  }
  params.set('include_code', includeCode.toString());

  return apiFetch<{ comp: Composition; latest: CompositionVersion }[]>(
    `/compositions?${params.toString()}`,
    undefined,
    fetch
  );
};

export const listMyCompositions = (): Promise<Composition[]> => apiFetch<Composition[]>('/compositions/my');

export const getComposition = (
  id: number,
  fetch: typeof globalThis.fetch = globalThis.fetch,
  sessionID?: string,
  adminToken?: string,
  baseURL?: string
): Promise<Composition> =>
  apiFetch<Composition>(
    `/compositions/${id}${adminToken ? `?admin_token=${encodeURIComponent(adminToken)}` : ''}`,
    sessionID ? { headers: { session_id: sessionID } } : {},
    fetch,
    undefined,
    baseURL
  );

export interface UpdateCompositionPatch {
  title?: string;
  description?: string;
  is_shared?: boolean;
}

export const updateComposition = (
  id: number,
  fieldMask: string[],
  patch: UpdateCompositionPatch
): Promise<Composition> =>
  apiFetch<Composition>(`/compositions/${id}`, {
    method: 'PATCH',
    body: JSON.stringify({ field_mask: fieldMask, patch }),
  });

export const createCompositionVersion = (
  id: number,
  data: CreateCompositionVersion
): Promise<CompositionVersion> =>
  apiFetch<CompositionVersion>(`/compositions/${id}/versions`, {
    method: 'POST',
    body: JSON.stringify(data),
  });

export const forkComposition = (id: number): Promise<Composition> =>
  apiFetch<Composition>(`/compositions/${id}/fork`, { method: 'POST' });

export const listCompositionVersions = (id: number): Promise<number[]> =>
  apiFetch<number[]>(`/compositions/${id}/versions`);

export const getCompositionLatest = (
  id: number,
  fetch: typeof globalThis.fetch = globalThis.fetch,
  sessionID?: string,
  adminToken?: string,
  baseUrl?: string
): Promise<CompositionVersion> =>
  apiFetch<CompositionVersion>(
    `/compositions/${id}/latest${adminToken ? `?admin_token=${encodeURIComponent(adminToken)}` : ''}`,
    sessionID ? { headers: { session_id: sessionID } } : {},
    fetch,
    undefined,
    baseUrl
  );

export const getCompositionVersion = (
  id: number,
  version: number,
  fetch: typeof globalThis.fetch = globalThis.fetch,
  sessionID?: string,
  adminToken?: string,
  baseUrl?: string
): Promise<CompositionVersion> =>
  apiFetch<CompositionVersion>(
    `/compositions/${id}/version/${version}${adminToken ? `?admin_token=${encodeURIComponent(adminToken)}` : ''}`,
    sessionID ? { headers: { session_id: sessionID } } : {},
    fetch,
    undefined,
    baseUrl
  );

export const deleteComposition = (id: number): Promise<void> =>
  apiFetch<void>(`/compositions/${id}`, { method: 'DELETE' });

export type TextureID = number;

export interface Texture {
  id: TextureID;
  name: string;
  thumbnailUrl: string;
  url: string;
  ownerId: number;
  ownerName: string;
  createdAt: string;
}

export const listTextures = (fetch: typeof globalThis.fetch = globalThis.fetch): Promise<Texture[]> =>
  apiFetch<Texture[]>('/textures', {}, fetch);

export const getTexture = (
  id: TextureID,
  fetch: typeof globalThis.fetch = globalThis.fetch
): Promise<Texture> => apiFetch<Texture>(`/textures/${id}`, {}, fetch);

export const createTexture = (
  name: string,
  file: File,
  is_shared: boolean,
  fetch: typeof globalThis.fetch = globalThis.fetch
): Promise<Texture> => {
  const searchParams = new URLSearchParams();
  searchParams.set('name', name);
  searchParams.set('is_shared', is_shared.toString());

  return apiFetch<Texture>(
    `/textures?${searchParams.toString()}`,
    {
      method: 'POST',
      body: file,
    },
    fetch
  );
};

export const createTextureFromURL = (
  name: string,
  url: string,
  is_shared: boolean,
  fetch: typeof globalThis.fetch = globalThis.fetch
): Promise<Texture> => {
  const searchParams = new URLSearchParams();
  searchParams.set('name', name);
  searchParams.set('is_shared', is_shared.toString());

  return apiFetch<Texture>(
    `/textures/from_url?${searchParams.toString()}`,
    {
      method: 'POST',
      body: JSON.stringify({ url }),
    },
    fetch
  );
};

export const getMultipleTextures = (
  ids: TextureID[],
  fetch: typeof globalThis.fetch = globalThis.fetch,
  adminToken?: string
): Promise<Texture[]> => {
  const searchParams = new URLSearchParams();
  for (const id of ids) {
    searchParams.append('id', id.toString());
  }
  if (adminToken) {
    searchParams.set('admin_token', adminToken);
  }
  return apiFetch<Texture[]>(`/textures/multiple?${searchParams.toString()}`, {}, fetch);
};
