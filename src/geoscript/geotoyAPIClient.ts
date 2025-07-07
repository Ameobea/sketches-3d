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
    fov?: number; // for PerspectiveCamera
    zoom?: number; // for OrthographicCamera
  };
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
  fetch: typeof globalThis.fetch = globalThis.fetch
): Promise<T> => {
  const res = await fetch(`${INTERNAL_PROXY_GEOTOY_API_BASE_URL}${path}`, {
    ...options,
    credentials: 'include',
    headers: {
      'Content-Type': 'application/json',
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
  }: {
    featuredOnly?: boolean;
    count?: number;
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
  adminToken?: string
): Promise<Composition> =>
  apiFetch<Composition>(
    `/compositions/${id}${adminToken ? `?admin_token=${encodeURIComponent(adminToken)}` : ''}`,
    sessionID ? { headers: { session_id: sessionID } } : {},
    fetch
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
  adminToken?: string
): Promise<CompositionVersion> =>
  apiFetch<CompositionVersion>(
    `/compositions/${id}/latest${adminToken ? `?admin_token=${encodeURIComponent(adminToken)}` : ''}`,
    sessionID ? { headers: { session_id: sessionID } } : {},
    fetch
  );

export const getCompositionVersion = (
  id: number,
  version: number,
  fetch: typeof globalThis.fetch = globalThis.fetch,
  sessionID?: string,
  adminToken?: string
): Promise<CompositionVersion> =>
  apiFetch<CompositionVersion>(
    `/compositions/${id}/version/${version}${adminToken ? `?admin_token=${encodeURIComponent(adminToken)}` : ''}`,
    sessionID ? { headers: { session_id: sessionID } } : {},
    fetch
  );

export const deleteComposition = (id: number): Promise<void> =>
  apiFetch<void>(`/compositions/${id}`, { method: 'DELETE' });
