import {
  APIError,
  buildLegacyRootTree,
  createCompositionVersion,
  ROOT_NODE_NAME,
  updateComposition,
  type Composition,
  type CompositionVersionMetadata,
  type EnvironmentConfig,
  type TreeDef,
} from 'src/geoscript/geotoyAPIClient';
import type { GeoscriptPlaygroundUserData } from './geoscriptPlayground.svelte';
import { DefaultCameraFOV, DefaultCameraPos, DefaultCameraTarget, DefaultCameraZoom } from './types';
import { buildDefaultMaterialDefinitions, type MaterialDefinitions } from 'src/geoscript/materials';
import type { Viz } from 'src/viz';
import type { OrbitControls } from 'three/examples/jsm/Addons.js';
import { OrthographicCamera, PerspectiveCamera } from 'three';

const DefaultCode = 'box(8) | (box(8) + vec3(4, 4, -4)) | render';

export interface PlaygroundState {
  tree: TreeDef;
  materials: MaterialDefinitions;
  view: CompositionVersionMetadata['view'];
  lastRunWasSuccessful: boolean;
  preludeEjected: boolean;
  environment?: EnvironmentConfig;
}

const getLocalStorageKeySuffix = (userData: GeoscriptPlaygroundUserData | undefined): string => {
  const initComposition = userData?.initialComposition;
  if (!initComposition) {
    return '';
  }
  return `-${initComposition.comp.id}-${initComposition.version.id}`;
};

const getTreeKey = (userData: GeoscriptPlaygroundUserData | undefined): string =>
  `lastGeoscriptPlaygroundTree${getLocalStorageKeySuffix(userData)}`;

const DefaultView: CompositionVersionMetadata['view'] = {
  cameraPosition: [DefaultCameraPos.x, DefaultCameraPos.y, DefaultCameraPos.z],
  target: [DefaultCameraTarget.x, DefaultCameraTarget.y, DefaultCameraTarget.z],
  fov: DefaultCameraFOV,
  zoom: DefaultCameraZoom,
  projection: 'perspective',
};

const parseTreeOrNull = (raw: string | null): TreeDef | null => {
  if (!raw) return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch (err) {
    console.warn('Error parsing saved tree:', err);
    return null;
  }
  if (!parsed || typeof parsed !== 'object') return null;
  const t = parsed as Partial<TreeDef>;
  // Drafts predating the v1 instances migration are dropped, not upgraded.
  if (t.version !== 1) {
    console.warn('Discarding saved tree: not version 1', t);
    return null;
  }
  if (typeof t.rootId !== 'string' || t.rootId.length === 0) {
    console.warn('Discarding saved tree: missing/invalid rootId', t);
    return null;
  }
  if (!t.nodes || typeof t.nodes !== 'object') {
    console.warn('Discarding saved tree: missing nodes', t);
    return null;
  }
  const rootNode = t.nodes[t.rootId];
  if (!rootNode || rootNode.name !== ROOT_NODE_NAME) {
    console.warn('Discarding saved tree: rootId does not resolve to a `_root` node', t);
    return null;
  }
  return parsed as TreeDef;
};

/** Locally-saved draft `TreeDef`, or null if absent / unparseable. */
export const loadTreeFromLocal = (userData: GeoscriptPlaygroundUserData | undefined): TreeDef | null =>
  parseTreeOrNull(localStorage.getItem(getTreeKey(userData)));

export const saveTreeToLocal = (tree: TreeDef, userData: GeoscriptPlaygroundUserData | undefined): void => {
  localStorage.setItem(getTreeKey(userData), JSON.stringify(tree));
};

/** Server-side tree from the loaded composition, or null if there's no initial composition. */
export const getServerTree = (userData: GeoscriptPlaygroundUserData | undefined): TreeDef | null =>
  userData?.initialComposition?.version.tree ?? null;

/** Pre-unification `physical`/`basic` drafts can't be read by the shared build path; discard them. */
const isLegacyMaterials = (m: MaterialDefinitions): boolean =>
  Object.values(m?.materials ?? {}).some(
    d => (d as { type?: string }).type === 'physical' || (d as { type?: string }).type === 'basic'
  );

export const loadState = (userData: GeoscriptPlaygroundUserData | undefined): PlaygroundState => {
  const suffix = getLocalStorageKeySuffix(userData);

  const savedTree = parseTreeOrNull(localStorage.getItem(`lastGeoscriptPlaygroundTree${suffix}`));
  const savedMaterialsRaw = localStorage.getItem(`geoscriptPlaygroundMaterials${suffix}`);
  const savedView = localStorage.getItem(`geoscriptPlaygroundView${suffix}`);
  const savedPreludeEjected = localStorage.getItem(`geoscriptPlaygroundPreludeEjected${suffix}`);
  const savedEnvironment = localStorage.getItem(`geoscriptPlaygroundEnvironment${suffix}`);

  const lastRunWasSuccessful = localStorage.getItem(`lastGeoscriptRunCompleted${suffix}`) !== 'false';

  const serverTree = userData?.initialComposition?.version.tree;
  const serverMaterials = userData?.initialComposition?.version.metadata?.materials;
  const serverView = userData?.initialComposition?.version.metadata?.view;
  const serverPreludeEjected = userData?.initialComposition?.version.metadata?.preludeEjected;
  const serverEnvironment = userData?.initialComposition?.version.metadata?.environment;

  const tree: TreeDef = savedTree ?? serverTree ?? buildLegacyRootTree(DefaultCode);

  let materials: MaterialDefinitions;
  if (savedMaterialsRaw) {
    try {
      const parsed = JSON.parse(savedMaterialsRaw);
      materials = isLegacyMaterials(parsed) ? (serverMaterials ?? buildDefaultMaterialDefinitions()) : parsed;
    } catch (err) {
      console.warn('Error parsing saved material definitions:', err);
      materials = serverMaterials ?? buildDefaultMaterialDefinitions();
    }
  } else {
    materials = serverMaterials ?? buildDefaultMaterialDefinitions();
  }

  let view: CompositionVersionMetadata['view'] = serverView ?? DefaultView;
  if (savedView) {
    try {
      view = JSON.parse(savedView);
    } catch (err) {
      console.warn('Error parsing saved view metadata:', err);
    }
  }

  const preludeEjected = savedPreludeEjected
    ? savedPreludeEjected === 'true'
    : (serverPreludeEjected ?? false);

  let environment: EnvironmentConfig | undefined = serverEnvironment;
  if (savedEnvironment !== null) {
    try {
      environment = savedEnvironment === '' ? undefined : JSON.parse(savedEnvironment);
    } catch (err) {
      console.warn('Error parsing saved environment metadata:', err);
    }
  }

  return { tree, materials, view, lastRunWasSuccessful, preludeEjected, environment };
};

export const getView = (viz: Viz): CompositionVersionMetadata['view'] => ({
  cameraPosition: viz.camera.position.toArray(),
  target: viz.orbitControls?.target.toArray() || DefaultCameraTarget.toArray(),
  fov: viz.camera instanceof PerspectiveCamera ? viz.camera.fov : undefined,
  zoom: viz.camera.zoom,
  projection: viz.camera instanceof OrthographicCamera ? 'orthographic' : 'perspective',
});

export const saveState = (
  state: Omit<PlaygroundState, 'lastRunWasSuccessful'>,
  userData: GeoscriptPlaygroundUserData | undefined
) => {
  const suffix = getLocalStorageKeySuffix(userData);
  localStorage.setItem(`lastGeoscriptPlaygroundTree${suffix}`, JSON.stringify(state.tree));
  localStorage.setItem(`geoscriptPlaygroundMaterials${suffix}`, JSON.stringify(state.materials));
  localStorage.setItem(`geoscriptPlaygroundView${suffix}`, JSON.stringify(state.view));
  localStorage.setItem(`geoscriptPlaygroundPreludeEjected${suffix}`, state.preludeEjected ? 'true' : 'false');
  // Persist '' to mean "explicitly no environment" so it overrides a server default.
  localStorage.setItem(
    `geoscriptPlaygroundEnvironment${suffix}`,
    state.environment ? JSON.stringify(state.environment) : ''
  );
};

export const buildCompositionVersionMetadata = (
  viz: Viz,
  materials: MaterialDefinitions,
  preludeEjected: boolean,
  environment: EnvironmentConfig | undefined
): { type: 'ok'; metadata: CompositionVersionMetadata } | { type: 'error'; msg: string } => {
  const controls: OrbitControls | null = viz.orbitControls;
  if (!controls) {
    return { type: 'error', msg: 'missing orbit controls; app not yet initialized?' };
  }
  const view: CompositionVersionMetadata['view'] = {
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
    view,
    materials,
    preludeEjected,
    environment,
  };

  return { type: 'ok', metadata };
};

export const saveNewVersion = async (
  comp: Composition,
  currentTree: TreeDef,
  viz: Viz,
  materials: MaterialDefinitions,
  preludeEjected: boolean,
  environment: EnvironmentConfig | undefined,
  title: string,
  description: string,
  isShared: boolean,
  userData?: GeoscriptPlaygroundUserData
): Promise<{ type: 'ok' } | { type: 'error'; msg: string }> => {
  try {
    const metadataRes = buildCompositionVersionMetadata(viz, materials, preludeEjected, environment);
    if (metadataRes.type === 'error') {
      return metadataRes;
    }
    const metadata = metadataRes.metadata;

    await Promise.all([
      createCompositionVersion(comp.id, { tree: currentTree, metadata }),
      updateComposition(comp.id, ['title', 'description', 'is_shared'], {
        title,
        description,
        is_shared: isShared,
      }),
    ]);
    saveState(
      {
        tree: currentTree,
        materials,
        view: metadata.view,
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
  const savedTreeRaw = localStorage.getItem(`lastGeoscriptPlaygroundTree${suffix}`);
  const savedMaterialsRaw = localStorage.getItem(`geoscriptPlaygroundMaterials${suffix}`);
  const savedPreludeEjected = localStorage.getItem(`geoscriptPlaygroundPreludeEjected${suffix}`);
  const savedEnvironment = localStorage.getItem(`geoscriptPlaygroundEnvironment${suffix}`);

  const serverTree = userData?.initialComposition?.version.tree;
  const serverTreeJson = serverTree ? JSON.stringify(serverTree) : null;
  const serverMaterials =
    userData?.initialComposition?.version.metadata?.materials || buildDefaultMaterialDefinitions();
  const serverPreludeEjected = userData?.initialComposition?.version.metadata?.preludeEjected || false;
  const serverEnvironment = userData?.initialComposition?.version.metadata?.environment;
  const serverEnvironmentJson = serverEnvironment ? JSON.stringify(serverEnvironment) : '';

  return (
    (savedTreeRaw !== null && serverTreeJson !== null
      ? savedTreeRaw !== serverTreeJson
      : savedTreeRaw !== null) ||
    (savedMaterialsRaw ? savedMaterialsRaw !== JSON.stringify(serverMaterials) : false) ||
    (savedPreludeEjected !== null ? (savedPreludeEjected === 'true') !== serverPreludeEjected : false) ||
    (savedEnvironment !== null ? savedEnvironment !== serverEnvironmentJson : false)
  );
};

export const getServerState = (userData: GeoscriptPlaygroundUserData | undefined): PlaygroundState => {
  const serverTree = userData?.initialComposition?.version.tree ?? buildLegacyRootTree(DefaultCode);
  const serverMaterials =
    userData?.initialComposition?.version.metadata?.materials || buildDefaultMaterialDefinitions();
  const serverView = userData?.initialComposition?.version.metadata?.view || DefaultView;
  const serverPreludeEjected = userData?.initialComposition?.version.metadata?.preludeEjected || false;
  const serverEnvironment = userData?.initialComposition?.version.metadata?.environment;

  return {
    tree: serverTree,
    materials: serverMaterials,
    view: serverView,
    lastRunWasSuccessful: true,
    preludeEjected: serverPreludeEjected,
    environment: serverEnvironment,
  };
};

export const clearSavedState = (userData: GeoscriptPlaygroundUserData | undefined) => {
  const suffix = getLocalStorageKeySuffix(userData);
  localStorage.removeItem(`lastGeoscriptPlaygroundTree${suffix}`);
  localStorage.removeItem(`geoscriptPlaygroundMaterials${suffix}`);
  localStorage.removeItem(`geoscriptPlaygroundView${suffix}`);
  localStorage.removeItem(`geoscriptPlaygroundPreludeEjected${suffix}`);
  localStorage.removeItem(`geoscriptPlaygroundEnvironment${suffix}`);
};

export const setLastRunWasSuccessful = (
  wasSuccessful: boolean,
  userData: GeoscriptPlaygroundUserData | undefined
) => {
  const suffix = getLocalStorageKeySuffix(userData);
  localStorage[`lastGeoscriptRunCompleted${suffix}`] = wasSuccessful ? 'true' : 'false';
};
