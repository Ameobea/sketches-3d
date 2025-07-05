const BASE_URL = 'http://localhost:5810';

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

export interface CompositionVersion {
  id: number;
  composition_id: number;
  source_code: string;
  created_at: string;
}

export interface CreateComposition {
  title: string;
  description: string;
  source_code: string;
}

export interface CreateCompositionVersion {
  source_code: string;
}

const apiFetch = async <T>(
  path: string,
  options: RequestInit = {},
  fetch: typeof globalThis.fetch = globalThis.fetch
): Promise<T> => {
  const res = await fetch(`${BASE_URL}${path}`, {
    ...options,
    credentials: 'include',
    headers: {
      'Content-Type': 'application/json',
      ...(options.headers || {}),
    },
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`API error: ${res.status} ${text}`);
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

export const me = (): Promise<User> => apiFetch<User>('/users/me');

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
): Promise<Composition[]> => {
  const params = new URLSearchParams();
  if (featuredOnly) {
    params.set('featured_only', 'true');
  }
  if (count) {
    params.set('count', count.toString());
  }

  return apiFetch<Composition[]>(`/compositions?${params.toString()}`, undefined, fetch);
};

export const listMyCompositions = (): Promise<Composition[]> => apiFetch<Composition[]>('/compositions/my');

export const getComposition = (id: number): Promise<Composition> =>
  apiFetch<Composition>(`/compositions/${id}`);

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

export const getCompositionLatest = (id: number): Promise<CompositionVersion> =>
  apiFetch<CompositionVersion>(`/compositions/${id}/latest`);

export const getCompositionVersion = (id: number, version: number): Promise<CompositionVersion> =>
  apiFetch<CompositionVersion>(`/compositions/${id}/version/${version}`);

export const deleteComposition = (id: number): Promise<void> =>
  apiFetch<void>(`/compositions/${id}`, { method: 'DELETE' });
