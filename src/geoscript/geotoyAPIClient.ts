import type { MaterialDescriptor, MaterialDefinitions, MaterialDef } from './materials';

// Lazily accessed so node-test imports don't blow up on `import.meta.env`.
let _geotoyAPIBaseURL: string | undefined;
export const getGeotoyAPIBaseURL = (): string => {
  if (_geotoyAPIBaseURL === undefined) {
    _geotoyAPIBaseURL = import.meta.env.VITE_GEOSCRIPT_API_URL || 'http://localhost:5810';
  }
  return _geotoyAPIBaseURL!;
};

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
  tags: string[];
}

/** Scene-wide image-based lighting (IBL). */
export type EnvironmentConfig =
  | {
      kind: 'gradient';
      skyColor: number;
      horizonColor: number;
      groundColor: number;
      intensity?: number;
      setBackground?: boolean;
    }
  | {
      kind: 'equirect';
      /** Texture-library id of an equirectangular image, PMREM-prefiltered for IBL. */
      textureId: TextureID;
      intensity?: number;
      setBackground?: boolean;
    };

export interface MeshTabView {
  cameraPosition: [number, number, number];
  target: [number, number, number];
  fov?: number; // for `PerspectiveCamera`
  zoom?: number; // for `OrthographicCamera`
  projection: 'perspective' | 'orthographic';
}

export type TextureChannel = 'rgb' | 'r' | 'g' | 'b' | 'a';

export interface TextureTabView {
  /** UV-space point at the viewport center. */
  center: [number, number];
  /** Screen px per texel of the selected output. */
  zoom: number;
  /** Selected output name; falls back to the first output when absent or stale. */
  output?: string;
  channel?: TextureChannel;
  tiled?: boolean;
  /** Explicit sRGB-display override; unset defers to the output's usage (albedo → on). */
  srgb?: boolean;
}

/** Capture and restore are paired through the same mode, so each mode narrows by its kind. */
export type TabView = MeshTabView | TextureTabView;

/** One `render_texture` output as observed on the tab's last completed run. The persisted
 *  list lets the material editor offer a never-yet-run tab's outputs on a fresh load; it
 *  re-syncs on every run of the tab. */
export interface TextureOutputMeta {
  name: string;
  usage?: string;
}

/** UI-owned GPU materialization params for one texture output. These have no geoscript
 *  source-code surface: the editor writes them here, they're injected into each run and
 *  baked onto the rendered handle, and consumers (material upload, HUD) read them off the
 *  run output. Omitted fields mean "consumer default". `wrap` is deliberately absent — it
 *  affects synthesis-side sampling, so it stays code-owned on `texture()`. */
export interface TextureOutputGpuParams {
  minFilter?: string;
  magFilter?: string;
  format?: string;
}

/**
 * Per-tab state that isn't tree content. Tagged by the tab's kind so mesh-only state (camera,
 * scene environment) can't exist on a texture tab. `kind` duplicates `TreeEntry.kind`
 * deliberately: the blob is self-describing, and writes derive it from the live tab so the two
 * can't diverge — a mismatched read trusts `TreeEntry.kind` and ignores the entry.
 */
export type TabMetadata =
  | {
      kind: 'mesh';
      preludeEjected: boolean;
      view?: MeshTabView;
      environment?: EnvironmentConfig;
    }
  | {
      kind: 'texture';
      preludeEjected: boolean;
      view?: TextureTabView;
      textureOutputs?: TextureOutputMeta[];
      /** Keyed by output name. Unlike `textureOutputs` (a derived index), these are user
       *  content: edits dirty the composition and re-run the composition. */
      textureParams?: Record<string, TextureOutputGpuParams>;
    };

/** Bumped only by a migration; an unexpected value is a hard load error, never a fallback. */
export const COMPOSITION_METADATA_VERSION = 1;

export interface CompositionVersionMetadata {
  version: typeof COMPOSITION_METADATA_VERSION;
  /** Keyed by tree id; rebuilt from the live tab set on every write, so it never holds an
   *  entry for a tree that no longer exists. */
  tabs: Record<string, TabMetadata>;
  activeTreeId: string;
  /** Composition-wide: one palette, referenced by name from any tree, one editor. */
  materials?: MaterialDefinitions;
}

/**
 * Metadata for a loaded version, or `undefined` when the row carries none (the column defaults
 * to `{}`). Throws on metadata that exists but isn't this version — an unmigrated or future
 * row must not be silently reinterpreted, and every consumer needs the same answer.
 */
export const readVersionMetadata = (
  metadata: CompositionVersionMetadata | undefined
): CompositionVersionMetadata | undefined => {
  if (!metadata || Object.keys(metadata).length === 0) return undefined;
  if (metadata.version !== COMPOSITION_METADATA_VERSION) {
    throw new Error(
      `composition metadata is version ${metadata.version}, expected ${COMPOSITION_METADATA_VERSION}; ` +
        'the database migration has not been applied'
    );
  }
  return metadata;
};

/**
 * Flat single-tree metadata for transient renders. Not the stored shape — it exists so a CLI
 * caller can say "here's some geoscript, maybe a camera" without spelling out a `tabs` record.
 */
export interface TransientRenderMetadata {
  view?: MeshTabView;
  materials?: MaterialDefinitions;
  preludeEjected?: boolean;
  environment?: EnvironmentConfig;
}

export const defaultTabMetadata = (kind: TreeKind): TabMetadata =>
  kind === 'mesh' ? { kind: 'mesh', preludeEjected: false } : { kind: 'texture', preludeEjected: false };

export interface Transform3 {
  pos: [number, number, number];
  rot: [number, number, number];
  scale: [number, number, number];
}

/** A node placement: a transform plus a short id stable across edits (gizmo target +
 *  undo address instances by id, not array index). Id uniqueness is scoped per node. */
export interface Instance extends Transform3 {
  id: string;
}

export interface GizmoValue {
  kind: 'vec3' | 'transform';
  mode: 'delta' | 'absolute';
  value: [number, number, number] | Transform3;
}

export interface RampStopJson {
  pos: number;
  /** 1 element for scalar ramps, 3 (linear RGB) for color ramps. */
  value: number[];
  ease: 'linear' | 'smooth' | 'smoother' | 'step';
}

/** Serialized `input_ramp`/`input_color_ramp` spec; mirrors the wasm-side `RampSpecWire`. */
export interface RampSpecJson {
  scalar: boolean;
  stops: RampStopJson[];
  extend: 'clamp' | 'repeat' | 'mirror';
  space: 'linear' | 'oklab' | 'oklch' | 'srgb';
}

/** An `input_*(...)` control value keyed by handleId; sparse. Written by the control panel. */
export interface ControlValue {
  kind: 'float' | 'int' | 'bool' | 'color' | 'select' | 'spline' | 'ramp';
  value: number | boolean | [number, number, number] | string | [number, number, number][] | RampSpecJson;
}

export interface NodeDef {
  id: string;
  name: string;
  source: string;
  /** Per-node placements; length >= 1. The single-copy case is `instances.length === 1`. */
  instances: Instance[];
  /** Gizmo values keyed by handleId; sparse. Populated by the gizmo runtime (M3). */
  handles?: Record<string, GizmoValue>;
  /** Control-panel input values keyed by handleId; sparse. */
  controls?: Record<string, ControlValue>;
  children: string[];
  disabled?: boolean;
}

export interface TreeDef {
  version: 1;
  /** Id of the always-present `_root` compositor node. */
  rootId: string;
  globalsSource: string;
  nodes: Record<string, NodeDef>;
}

export const isTreeDefV1 = (raw: unknown): raw is TreeDef => {
  const t = raw as TreeDef | null;
  return !!t && t.version === 1 && typeof t.rootId === 'string' && !!t.nodes && typeof t.nodes === 'object';
};

export type TreeKind = 'mesh' | 'texture';

/**
 * Charset for node names *and* tree ids. Load-bearing: ids enter the geoscript module
 * namespace as `<tabId>:<nodeName>`, so neither half may contain `:`.
 */
export const NAME_RE = /^[a-zA-Z_][a-zA-Z0-9_]*$/;

export interface TreeEntry {
  id: string;
  kind: TreeKind;
  name: string;
  tree: TreeDef;
}

/** v2 container: a composition version holds 1+ typed trees, each core an intact v1 `TreeDef`. */
export interface CompositionDoc {
  version: 2;
  trees: TreeEntry[];
}

export const isCompositionDocV2 = (raw: unknown): raw is CompositionDoc => {
  const d = raw as CompositionDoc | null;
  return (
    !!d &&
    d.version === 2 &&
    Array.isArray(d.trees) &&
    d.trees.length > 0 &&
    d.trees.every(
      t =>
        !!t &&
        typeof t.id === 'string' &&
        // Enforced here rather than only where ids are generated: an id with a `:` would
        // silently break every import in the composition with an `Unknown module` cascade.
        NAME_RE.test(t.id) &&
        // An unknown `kind` resolves to no mode at all.
        (t.kind === 'mesh' || t.kind === 'texture') &&
        typeof t.name === 'string' &&
        isTreeDefV1(t.tree)
    )
  );
};

/** The entry a consumer binds to when it doesn't name one: first tree of `kind`, else first tree. */
export const defaultTreeEntry = (doc: CompositionDoc, kind: TreeKind = 'mesh'): TreeEntry =>
  doc.trees.find(t => t.kind === kind) ?? doc.trees[0];

export const defaultTree = (doc: CompositionDoc, kind: TreeKind = 'mesh'): TreeDef =>
  defaultTreeEntry(doc, kind).tree;

export const wrapTree = (tree: TreeDef): CompositionDoc => ({
  version: 2,
  trees: [{ id: 'main', kind: 'mesh', name: 'main', tree }],
});

export const withTree = (doc: CompositionDoc, treeId: string, tree: TreeDef): CompositionDoc => ({
  ...doc,
  trees: doc.trees.map(t => (t.id === treeId ? { ...t, tree } : t)),
});

/** The reserved name of the always-present root compositor node. */
export const ROOT_NODE_NAME = '_root';

export interface CompositionVersion {
  id: number;
  composition_id: number;
  tree: CompositionDoc;
  created_at: string;
  metadata: CompositionVersionMetadata;
  thumbnail_url?: string | null;
}

export interface CreateComposition {
  title: string;
  description: string;
  tree: CompositionDoc;
  is_shared: boolean;
  metadata: CompositionVersionMetadata;
  tags?: string[];
}

export interface CreateCompositionVersion {
  tree: CompositionDoc;
  metadata: CompositionVersionMetadata;
}

export const buildIdentityTransform = (): Transform3 => ({
  pos: [0, 0, 0],
  rot: [0, 0, 0],
  scale: [1, 1, 1],
});

export const cloneTransform3 = (t: Transform3): Transform3 => ({
  pos: [t.pos[0], t.pos[1], t.pos[2]],
  rot: [t.rot[0], t.rot[1], t.rot[2]],
  scale: [t.scale[0], t.scale[1], t.scale[2]],
});

const randHex8 = (): string => {
  const b = new Uint8Array(4);
  crypto.getRandomValues(b);
  let s = '';
  for (const x of b) s += x.toString(16).padStart(2, '0');
  return s;
};

/** A short (8 hex char) id unique among `existing`. Per-node scope keeps collisions
 *  vanishingly unlikely; the loop is belt-and-suspenders. */
export const newInstanceId = (existing?: Iterable<string>): string => {
  const taken = existing instanceof Set ? existing : new Set(existing);
  let id = randHex8();
  while (taken.has(id)) id = randHex8();
  return id;
};

export const buildInstance = (transform?: Transform3, existingIds?: Iterable<string>): Instance => ({
  ...cloneTransform3(transform ?? buildIdentityTransform()),
  id: newInstanceId(existingIds),
});

/**
 * Build a fresh tree containing only `_root` with the given source. Used for legacy
 * single-source compositions and as the empty-state default.
 */
export const buildLegacyRootTree = (source: string): TreeDef => {
  const id = crypto.randomUUID();
  return {
    version: 1,
    rootId: id,
    globalsSource: '',
    nodes: {
      [id]: {
        id,
        name: ROOT_NODE_NAME,
        source,
        instances: [buildInstance()],
        children: [],
      },
    },
  };
};

export const buildEmptyTree = (): TreeDef => buildLegacyRootTree('');

export const buildDefaultDoc = (source: string): CompositionDoc => wrapTree(buildLegacyRootTree(source));

export const buildEmptyDoc = (): CompositionDoc => wrapTree(buildEmptyTree());

/** Root-node source of the container's default tree. */
export const getRootNodeSource = (doc: CompositionDoc): string => {
  const tree = defaultTree(doc);
  return tree.nodes[tree.rootId]?.source ?? '';
};

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

export interface CompositionAndVersion {
  comp: Composition;
  latest: CompositionVersion;
}

export const listPublicCompositions = (
  {
    featuredOnly,
    count = 20,
    offset = 0,
    includeCode = false,
    userID,
  }: {
    featuredOnly?: boolean;
    count?: number;
    offset?: number;
    includeCode?: boolean;
    userID?: number;
  },
  fetch: typeof globalThis.fetch = globalThis.fetch
): Promise<CompositionAndVersion[]> => {
  const params = new URLSearchParams();
  if (featuredOnly) {
    params.set('featured_only', 'true');
  }
  if (count) {
    params.set('count', count.toString());
  }
  if (offset) {
    params.set('offset', offset.toString());
  }
  params.set('include_code', includeCode.toString());
  if (userID) {
    params.set('user_id', userID.toString());
  }

  return apiFetch<CompositionAndVersion[]>(`/compositions?${params.toString()}`, undefined, fetch);
};

export const listMyCompositions = (
  sessionID: string,
  fetch?: typeof globalThis.fetch
): Promise<CompositionAndVersion[]> =>
  apiFetch<CompositionAndVersion[]>('/compositions/my', { headers: { session_id: sessionID } }, fetch);

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

export const getCompositionHistory = (
  id: number,
  fetch: typeof globalThis.fetch = globalThis.fetch,
  sessionID?: string,
  adminToken?: string
): Promise<CompositionVersion[]> =>
  apiFetch<CompositionVersion[]>(
    `/compositions/${id}/history${adminToken ? `?admin_token=${encodeURIComponent(adminToken)}` : ''}`,
    sessionID ? { headers: { session_id: sessionID } } : {},
    fetch
  );

export interface UpdateCompositionPatch {
  title?: string;
  description?: string;
  is_shared?: boolean;
  tags?: string[];
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

export const forkComposition = (
  id: number
): Promise<{ composition: Composition; version: CompositionVersion }> =>
  apiFetch<{ composition: Composition; version: CompositionVersion }>(`/compositions/${id}/fork`, {
    method: 'POST',
  });

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

export interface TextureDescriptor {
  id: TextureID;
  name: string;
  description: string;
  thumbnailUrl: string;
  url: string;
  /** Where the texture was downloaded from, if it was created via `createTextureFromURL`. */
  sourceUrl: string | null;
  ownerId: number;
  ownerName: string;
  createdAt: string;
  isShared: boolean;
  tags: string[];
}

/** Metadata common to both texture-creation paths; sent as query params. */
export interface CreateTextureMeta {
  name: string;
  description?: string;
  isShared: boolean;
  tags?: string[];
}

const createTextureParams = ({ name, description, isShared, tags }: CreateTextureMeta): string => {
  const searchParams = new URLSearchParams();
  searchParams.set('name', name);
  searchParams.set('is_shared', isShared.toString());
  if (description) {
    searchParams.set('description', description);
  }
  for (const tag of tags ?? []) {
    searchParams.append('tag', tag);
  }
  return searchParams.toString();
};

export const listTextures = (
  fetch: typeof globalThis.fetch = globalThis.fetch
): Promise<TextureDescriptor[]> => apiFetch<TextureDescriptor[]>('/textures', {}, fetch);

export const getTexture = (
  id: TextureID,
  fetch: typeof globalThis.fetch = globalThis.fetch
): Promise<TextureDescriptor> => apiFetch<TextureDescriptor>(`/textures/${id}`, {}, fetch);

export const createTexture = (
  meta: CreateTextureMeta,
  file: File,
  fetch: typeof globalThis.fetch = globalThis.fetch
): Promise<TextureDescriptor> =>
  apiFetch<TextureDescriptor>(
    `/textures?${createTextureParams(meta)}`,
    {
      method: 'POST',
      body: file,
    },
    fetch
  );

export const createTextureFromURL = (
  meta: CreateTextureMeta,
  url: string,
  fetch: typeof globalThis.fetch = globalThis.fetch
): Promise<TextureDescriptor> =>
  apiFetch<TextureDescriptor>(
    `/textures/from_url?${createTextureParams(meta)}`,
    {
      method: 'POST',
      body: JSON.stringify({ url }),
    },
    fetch
  );

export const updateTexture = (
  id: TextureID,
  patch: Partial<{ name: string; description: string; isShared: boolean; tags: string[] }>,
  fetch: typeof globalThis.fetch = globalThis.fetch
): Promise<TextureDescriptor> =>
  apiFetch<TextureDescriptor>(
    `/textures/${id}`,
    {
      method: 'PATCH',
      body: JSON.stringify(patch),
    },
    fetch
  );

export const deleteTexture = (
  id: TextureID,
  fetch: typeof globalThis.fetch = globalThis.fetch
): Promise<void> => apiFetch<void>(`/textures/${id}`, { method: 'DELETE' }, fetch);

export const getMultipleTextures = (
  ids: TextureID[],
  fetch: typeof globalThis.fetch = globalThis.fetch,
  adminToken?: string,
  baseUrl?: string
): Promise<TextureDescriptor[]> => {
  const searchParams = new URLSearchParams();
  for (const id of ids) {
    searchParams.append('id', id.toString());
  }
  if (adminToken) {
    searchParams.set('admin_token', adminToken);
  }
  return apiFetch<TextureDescriptor[]>(
    `/textures/multiple?${searchParams.toString()}`,
    {},
    fetch,
    false,
    baseUrl
  );
};

export const listMaterials = (
  fetch: typeof globalThis.fetch = globalThis.fetch
): Promise<MaterialDescriptor[]> => apiFetch<MaterialDescriptor[]>('/materials', {}, fetch);

export const createMaterial = (
  def: MaterialDef,
  isShared: boolean,
  { description, tags }: { description?: string; tags?: string[] } = {},
  fetch: typeof globalThis.fetch = globalThis.fetch
): Promise<MaterialDescriptor> =>
  apiFetch<MaterialDescriptor>(
    '/materials',
    {
      method: 'POST',
      body: JSON.stringify({
        name: def.name,
        description,
        materialDefinition: def,
        isShared,
        tags,
      }),
      headers: { 'Content-Type': 'application/json' },
    },
    fetch
  );

export const getMaterial = (
  id: number,
  fetch: typeof globalThis.fetch = globalThis.fetch,
  adminToken?: string,
  baseUrl?: string
): Promise<MaterialDescriptor> =>
  apiFetch<MaterialDescriptor>(
    `/materials/${id}${adminToken ? `?admin_token=${encodeURIComponent(adminToken)}` : ''}`,
    {},
    fetch,
    false,
    baseUrl
  );

export const updateMaterial = (
  id: number,
  body: Partial<{
    name: string;
    description: string;
    materialDefinition: MaterialDef;
    isShared: boolean;
    tags: string[];
  }>,
  fetch: typeof globalThis.fetch = globalThis.fetch
): Promise<MaterialDescriptor> =>
  apiFetch<MaterialDescriptor>(
    `/materials/${id}`,
    {
      method: 'PUT',
      body: JSON.stringify(body),
      headers: { 'Content-Type': 'application/json' },
    },
    fetch
  );

export const deleteMaterial = (
  id: number,
  fetch: typeof globalThis.fetch = globalThis.fetch
): Promise<void> => apiFetch<void>(`/materials/${id}`, { method: 'DELETE' }, fetch);

export const unwrapUVs = async (
  vertices: Float32Array,
  indices: Uint32Array,
  fetch: typeof globalThis.fetch = globalThis.fetch
): Promise<{ uvs: Float32Array; verts: Float32Array; indices: Uint32Array }> => {
  const encodeRequestBody = (vertices: Float32Array, indices: Uint32Array): ArrayBuffer => {
    const vertexCount = vertices.length / 3;
    const indexCount = indices.length;
    const buffer = new ArrayBuffer(8 + vertexCount * 12 + indexCount * 4);
    const dataView = new DataView(buffer);
    dataView.setUint32(0, vertexCount, true);
    dataView.setUint32(4, indexCount, true);

    const vertexArray = new Float32Array(buffer, 8, vertexCount * 3);
    vertexArray.set(vertices);

    const indexArray = new Uint32Array(buffer, 8 + vertexCount * 12, indexCount);
    indexArray.set(indices);

    return buffer;
  };

  const body = encodeRequestBody(vertices, indices);
  const headers = new Headers();
  headers.set('Content-Type', 'application/octet-stream');
  const response = await fetch('/uv_unwrap', { method: 'POST', body, headers });

  if (!response.ok) {
    throw new APIError(response.status, await response.text());
  }

  const decodeUVUnwrapResponse = (
    encodedResponse: ArrayBuffer
  ): { uvs: Float32Array; verts: Float32Array; indices: Uint32Array } => {
    const dataView = new DataView(encodedResponse);
    const vertexCount = dataView.getUint32(0, true);
    const indexCount = dataView.getUint32(4, true);

    if (encodedResponse.byteLength !== 8 + vertexCount * 2 * 4 + vertexCount * 3 * 4 + indexCount * 4) {
      throw new APIError(
        400,
        `Invalid response size; expected ${8 + vertexCount * 2 * 4 + vertexCount * 3 * 4 + indexCount * 4} bytes, got ${encodedResponse.byteLength} bytes`
      );
    }

    const uvs = new Float32Array(encodedResponse, 8, vertexCount * 2);
    const verts = new Float32Array(encodedResponse, 8 + vertexCount * 2 * 4, vertexCount * 3);
    const indices = new Uint32Array(encodedResponse, 8 + vertexCount * (2 + 3) * 4, indexCount);

    return { uvs, verts, indices };
  };

  const responseBuffer = await response.arrayBuffer();
  return decodeUVUnwrapResponse(responseBuffer);
};
