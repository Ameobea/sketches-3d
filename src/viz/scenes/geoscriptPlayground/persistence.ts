import {
  APIError,
  buildDefaultDoc,
  createCompositionVersion,
  defaultTreeEntry,
  isCompositionDocV2,
  ROOT_NODE_NAME,
  updateComposition,
  type Composition,
  type CompositionDoc,
  type CompositionVersionMetadata,
  type EnvironmentConfig,
  type TreeDef,
  type ViewState,
} from 'src/geoscript/geotoyAPIClient';
import type { GeoscriptPlaygroundUserData } from './geoscriptPlayground.svelte';
import { DefaultCameraFOV, DefaultCameraPos, DefaultCameraTarget, DefaultCameraZoom } from './types';
import { buildDefaultMaterialDefinitions, type MaterialDefinitions } from 'src/geoscript/materials';
import type { Viz } from 'src/viz';
import type { OrbitControls } from 'three/examples/jsm/Addons.js';
import { OrthographicCamera, PerspectiveCamera } from 'three';

const DefaultCode = 'box(8) | (box(8) + vec3(4, 4, -4)) | render';

export interface PlaygroundState {
  doc: CompositionDoc;
  /** Active tree core within `doc` (currently always the default mesh tree). */
  tree: TreeDef;
  activeTreeId: string;
  materials: MaterialDefinitions;
  view: ViewState;
  lastRunWasSuccessful: boolean;
  preludeEjected: boolean;
  environment?: EnvironmentConfig;
}

// v2-container draft keys; the prefix bump deliberately orphans all v1 drafts.
const KEY_DOC = 'geotoy2:doc';
const KEY_MATERIALS = 'geotoy2:materials';
const KEY_VIEWS = 'geotoy2:views';
const KEY_ACTIVE_TREE = 'geotoy2:activeTreeId';
const KEY_PRELUDE_EJECTED = 'geotoy2:preludeEjected';
const KEY_ENVIRONMENT = 'geotoy2:environment';
const KEY_LAST_RUN_COMPLETED = 'geotoy2:lastRunCompleted';

const getLocalStorageKeySuffix = (userData: GeoscriptPlaygroundUserData | undefined): string => {
  const initComposition = userData?.initialComposition;
  if (!initComposition) {
    return '';
  }
  return `-${initComposition.comp.id}-${initComposition.version.id}`;
};

const DefaultView: ViewState = {
  cameraPosition: [DefaultCameraPos.x, DefaultCameraPos.y, DefaultCameraPos.z],
  target: [DefaultCameraTarget.x, DefaultCameraTarget.y, DefaultCameraTarget.z],
  fov: DefaultCameraFOV,
  zoom: DefaultCameraZoom,
  projection: 'perspective',
};

const parseDocOrNull = (raw: string | null): CompositionDoc | null => {
  if (!raw) return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch (err) {
    console.warn('Error parsing saved composition doc:', err);
    return null;
  }
  if (!isCompositionDocV2(parsed)) {
    console.warn('Discarding saved composition doc: not a v2 container', parsed);
    return null;
  }
  for (const entry of parsed.trees) {
    const rootNode = entry.tree.nodes[entry.tree.rootId];
    if (!rootNode || rootNode.name !== ROOT_NODE_NAME) {
      console.warn(`Discarding saved composition doc: tree "${entry.name}" has no \`_root\` node`, parsed);
      return null;
    }
  }
  return parsed;
};

const parseViewsOrNull = (raw: string | null): Record<string, ViewState> | null => {
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === 'object' ? (parsed as Record<string, ViewState>) : null;
  } catch (err) {
    console.warn('Error parsing saved view metadata:', err);
    return null;
  }
};

/** Server-side doc from the loaded composition, or null if there's no initial composition. */
export const getServerDoc = (userData: GeoscriptPlaygroundUserData | undefined): CompositionDoc | null =>
  userData?.initialComposition?.version.tree ?? null;

/** Pre-unification `physical`/`basic` drafts can't be read by the shared build path; discard them. */
const isLegacyMaterials = (m: MaterialDefinitions): boolean =>
  Object.values(m?.materials ?? {}).some(
    d => (d as { type?: string }).type === 'physical' || (d as { type?: string }).type === 'basic'
  );

/** Active-tree id for a doc: the metadata's pick when it still exists, else the default entry. */
const resolveActiveTreeId = (doc: CompositionDoc, metadataActiveId: string | undefined): string =>
  metadataActiveId && doc.trees.some(t => t.id === metadataActiveId)
    ? metadataActiveId
    : defaultTreeEntry(doc).id;

export const loadState = (userData: GeoscriptPlaygroundUserData | undefined): PlaygroundState => {
  const suffix = getLocalStorageKeySuffix(userData);

  const savedDoc = parseDocOrNull(localStorage.getItem(`${KEY_DOC}${suffix}`));
  const savedMaterialsRaw = localStorage.getItem(`${KEY_MATERIALS}${suffix}`);
  const savedViews = parseViewsOrNull(localStorage.getItem(`${KEY_VIEWS}${suffix}`));
  const savedPreludeEjected = localStorage.getItem(`${KEY_PRELUDE_EJECTED}${suffix}`);
  const savedEnvironment = localStorage.getItem(`${KEY_ENVIRONMENT}${suffix}`);

  const lastRunWasSuccessful = localStorage.getItem(`${KEY_LAST_RUN_COMPLETED}${suffix}`) !== 'false';

  const serverMeta = userData?.initialComposition?.version.metadata;
  const serverDoc = userData?.initialComposition?.version.tree;

  const doc: CompositionDoc = savedDoc ?? serverDoc ?? buildDefaultDoc(DefaultCode);
  const savedActiveTreeId = localStorage.getItem(`${KEY_ACTIVE_TREE}${suffix}`);
  const activeTreeId = resolveActiveTreeId(doc, savedActiveTreeId ?? serverMeta?.activeTreeId);
  const tree = doc.trees.find(t => t.id === activeTreeId)!.tree;

  let materials: MaterialDefinitions;
  if (savedMaterialsRaw) {
    try {
      const parsed = JSON.parse(savedMaterialsRaw);
      materials = isLegacyMaterials(parsed)
        ? (serverMeta?.materials ?? buildDefaultMaterialDefinitions())
        : parsed;
    } catch (err) {
      console.warn('Error parsing saved material definitions:', err);
      materials = serverMeta?.materials ?? buildDefaultMaterialDefinitions();
    }
  } else {
    materials = serverMeta?.materials ?? buildDefaultMaterialDefinitions();
  }

  const view = savedViews?.[activeTreeId] ?? serverMeta?.views?.[activeTreeId] ?? DefaultView;

  const preludeEjected = savedPreludeEjected
    ? savedPreludeEjected === 'true'
    : (serverMeta?.preludeEjected ?? false);

  let environment: EnvironmentConfig | undefined = serverMeta?.environment;
  if (savedEnvironment !== null) {
    try {
      environment = savedEnvironment === '' ? undefined : JSON.parse(savedEnvironment);
    } catch (err) {
      console.warn('Error parsing saved environment metadata:', err);
    }
  }

  return { doc, tree, activeTreeId, materials, view, lastRunWasSuccessful, preludeEjected, environment };
};

export const getView = (viz: Viz): ViewState => ({
  cameraPosition: viz.camera.position.toArray(),
  target: viz.orbitControls?.target.toArray() || DefaultCameraTarget.toArray(),
  fov: viz.camera instanceof PerspectiveCamera ? viz.camera.fov : undefined,
  zoom: viz.camera.zoom,
  projection: viz.camera instanceof OrthographicCamera ? 'orthographic' : 'perspective',
});

export const saveState = (
  state: Omit<PlaygroundState, 'lastRunWasSuccessful' | 'tree'>,
  userData: GeoscriptPlaygroundUserData | undefined
) => {
  const suffix = getLocalStorageKeySuffix(userData);
  const docJson = JSON.stringify(state.doc);
  const materialsJson = JSON.stringify(state.materials);
  // Drafts must stay text-only (no textures/runtime assets) and far below localStorage limits.
  const bytes = docJson.length + materialsJson.length;
  if (bytes > 1_000_000) {
    console.warn(`geotoy draft is ${(bytes / 1e6).toFixed(2)}MB; drafts are expected to stay text-only`);
  }
  localStorage.setItem(`${KEY_DOC}${suffix}`, docJson);
  localStorage.setItem(`${KEY_MATERIALS}${suffix}`, materialsJson);
  const views = parseViewsOrNull(localStorage.getItem(`${KEY_VIEWS}${suffix}`)) ?? {};
  views[state.activeTreeId] = state.view;
  localStorage.setItem(`${KEY_VIEWS}${suffix}`, JSON.stringify(views));
  localStorage.setItem(`${KEY_ACTIVE_TREE}${suffix}`, state.activeTreeId);
  localStorage.setItem(`${KEY_PRELUDE_EJECTED}${suffix}`, state.preludeEjected ? 'true' : 'false');
  // Persist '' to mean "explicitly no environment" so it overrides a server default.
  localStorage.setItem(
    `${KEY_ENVIRONMENT}${suffix}`,
    state.environment ? JSON.stringify(state.environment) : ''
  );
};

export const buildCompositionVersionMetadata = (
  viz: Viz,
  activeTreeId: string,
  materials: MaterialDefinitions,
  preludeEjected: boolean,
  environment: EnvironmentConfig | undefined,
  /** Prior per-tree views to merge under, so saving one tree can't drop the others'. */
  baseViews?: Record<string, ViewState>
): { type: 'ok'; metadata: CompositionVersionMetadata } | { type: 'error'; msg: string } => {
  const controls: OrbitControls | null = viz.orbitControls;
  if (!controls) {
    return { type: 'error', msg: 'missing orbit controls; app not yet initialized?' };
  }
  const view: ViewState = {
    cameraPosition: [viz.camera.position.x, viz.camera.position.y, viz.camera.position.z],
    target: [controls.target.x, controls.target.y, controls.target.z],
    projection: viz.camera instanceof OrthographicCamera ? 'orthographic' : 'perspective',
  };
  if (viz.camera instanceof PerspectiveCamera) {
    view.fov = viz.camera.fov;
  }
  if (viz.camera instanceof OrthographicCamera) {
    view.zoom = viz.camera.zoom;
  }
  const metadata: CompositionVersionMetadata = {
    views: { ...baseViews, [activeTreeId]: view },
    activeTreeId,
    materials,
    preludeEjected,
    environment,
  };

  return { type: 'ok', metadata };
};

export interface CompositionMeta {
  title: string;
  description: string;
  isShared: boolean;
  tags: string[];
}

export const saveNewVersion = async (
  comp: Composition,
  currentDoc: CompositionDoc,
  activeTreeId: string,
  viz: Viz,
  materials: MaterialDefinitions,
  preludeEjected: boolean,
  environment: EnvironmentConfig | undefined,
  { title, description, isShared, tags }: CompositionMeta,
  userData?: GeoscriptPlaygroundUserData
): Promise<{ type: 'ok' } | { type: 'error'; msg: string }> => {
  try {
    const metadataRes = buildCompositionVersionMetadata(
      viz,
      activeTreeId,
      materials,
      preludeEjected,
      environment,
      userData?.initialComposition?.version.metadata?.views
    );
    if (metadataRes.type === 'error') {
      return metadataRes;
    }
    const metadata = metadataRes.metadata;

    await Promise.all([
      createCompositionVersion(comp.id, { tree: currentDoc, metadata }),
      updateComposition(comp.id, ['title', 'description', 'is_shared', 'tags'], {
        title,
        description,
        is_shared: isShared,
        tags,
      }),
    ]);
    saveState(
      {
        doc: currentDoc,
        activeTreeId,
        materials,
        view: metadata.views![activeTreeId],
        preludeEjected,
        environment,
      },
      userData
    );
    return { type: 'ok' };
  } catch (error) {
    console.error('Error saving changes:', error);
    if (error instanceof APIError) {
      return { type: 'error', msg: error.message };
    } else {
      return { type: 'error', msg: `${error}` };
    }
  }
};

/**
 * Not an efficient function; shouldn't be called frequently.
 */
export const getIsDirty = (userData: GeoscriptPlaygroundUserData | undefined): boolean => {
  const suffix = getLocalStorageKeySuffix(userData);
  const savedDocRaw = localStorage.getItem(`${KEY_DOC}${suffix}`);
  const savedMaterialsRaw = localStorage.getItem(`${KEY_MATERIALS}${suffix}`);
  const savedPreludeEjected = localStorage.getItem(`${KEY_PRELUDE_EJECTED}${suffix}`);
  const savedEnvironment = localStorage.getItem(`${KEY_ENVIRONMENT}${suffix}`);

  const serverDoc = userData?.initialComposition?.version.tree;
  const serverDocJson = serverDoc ? JSON.stringify(serverDoc) : null;
  const serverMaterials =
    userData?.initialComposition?.version.metadata?.materials || buildDefaultMaterialDefinitions();
  const serverPreludeEjected = userData?.initialComposition?.version.metadata?.preludeEjected || false;
  const serverEnvironment = userData?.initialComposition?.version.metadata?.environment;
  const serverEnvironmentJson = serverEnvironment ? JSON.stringify(serverEnvironment) : '';

  return (
    (savedDocRaw !== null && serverDocJson !== null ? savedDocRaw !== serverDocJson : savedDocRaw !== null) ||
    (savedMaterialsRaw ? savedMaterialsRaw !== JSON.stringify(serverMaterials) : false) ||
    (savedPreludeEjected !== null ? (savedPreludeEjected === 'true') !== serverPreludeEjected : false) ||
    (savedEnvironment !== null ? savedEnvironment !== serverEnvironmentJson : false)
  );
};

export const getServerState = (userData: GeoscriptPlaygroundUserData | undefined): PlaygroundState => {
  const serverMeta = userData?.initialComposition?.version.metadata;
  const doc = userData?.initialComposition?.version.tree ?? buildDefaultDoc(DefaultCode);
  const activeTreeId = resolveActiveTreeId(doc, serverMeta?.activeTreeId);
  const tree = doc.trees.find(t => t.id === activeTreeId)!.tree;
  const materials = serverMeta?.materials || buildDefaultMaterialDefinitions();
  const view = serverMeta?.views?.[activeTreeId] || DefaultView;

  return {
    doc,
    tree,
    activeTreeId,
    materials,
    view,
    lastRunWasSuccessful: true,
    preludeEjected: serverMeta?.preludeEjected || false,
    environment: serverMeta?.environment,
  };
};

export const clearSavedState = (userData: GeoscriptPlaygroundUserData | undefined) => {
  const suffix = getLocalStorageKeySuffix(userData);
  for (const key of [
    KEY_DOC,
    KEY_MATERIALS,
    KEY_VIEWS,
    KEY_ACTIVE_TREE,
    KEY_PRELUDE_EJECTED,
    KEY_ENVIRONMENT,
  ]) {
    localStorage.removeItem(`${key}${suffix}`);
  }
};

export const setLastRunWasSuccessful = (
  wasSuccessful: boolean,
  userData: GeoscriptPlaygroundUserData | undefined
) => {
  const suffix = getLocalStorageKeySuffix(userData);
  localStorage[`${KEY_LAST_RUN_COMPLETED}${suffix}`] = wasSuccessful ? 'true' : 'false';
};
