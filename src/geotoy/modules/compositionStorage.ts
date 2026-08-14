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
  COMPOSITION_METADATA_VERSION,
  defaultTabMetadata,
  readVersionMetadata,
  type CompositionVersionMetadata,
  type MeshTabView,
  type TabMetadata,
} from 'src/geoscript/geotoyAPIClient';
import type { GeoscriptPlaygroundUserData } from 'src/viz/scenes/geoscriptPlayground/geoscriptPlayground.svelte';
import { DefaultCameraTarget } from 'src/geotoy/types';
import { buildDefaultMaterialDefinitions, type MaterialDefinitions } from 'src/geoscript/materials';
import type { Viz } from 'src/viz';
import { OrthographicCamera, PerspectiveCamera } from 'three';

const DefaultCode = 'box(8) | (box(8) + vec3(4, 4, -4)) | render';

export interface PlaygroundState {
  doc: CompositionDoc;
  activeTreeId: string;
  materials: MaterialDefinitions;
  /** Per-tab view/prelude/environment, draft merged over server. Keyed by tree id. */
  tabMeta: Record<string, TabMetadata>;
  lastRunWasSuccessful: boolean;
}

// v2-container draft keys; the prefix bump deliberately orphans all v1 drafts.
const KEY_DOC = 'geotoy2:doc';
const KEY_MATERIALS = 'geotoy2:materials';
const KEY_TAB_META = 'geotoy2:tabMeta';
const KEY_ACTIVE_TREE = 'geotoy2:activeTreeId';
const KEY_LAST_RUN_COMPLETED = 'geotoy2:lastRunCompleted';

const getLocalStorageKeySuffix = (userData: GeoscriptPlaygroundUserData | undefined): string => {
  const initComposition = userData?.initialComposition;
  if (!initComposition) {
    return '';
  }
  return `-${initComposition.comp.id}-${initComposition.version.id}`;
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

const parseTabMetaOrNull = (raw: string | null): Record<string, TabMetadata> | null => {
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === 'object' ? (parsed as Record<string, TabMetadata>) : null;
  } catch (err) {
    console.warn('Error parsing saved tab metadata:', err);
    return null;
  }
};

/**
 * Per-tab metadata for every live tree: draft over server, defaulted for trees neither knows
 * about, and pruned to the doc. An entry whose `kind` disagrees with the tree's is discarded —
 * `TreeEntry.kind` is the authority.
 */
const resolveTabMeta = (
  doc: CompositionDoc,
  serverTabs: Record<string, TabMetadata> | undefined,
  draftTabs: Record<string, TabMetadata> | null
): Record<string, TabMetadata> => {
  const merged = { ...serverTabs, ...draftTabs };
  return Object.fromEntries(
    doc.trees.map(entry => {
      const found = merged[entry.id];
      return [entry.id, found?.kind === entry.kind ? found : defaultTabMetadata(entry.kind)];
    })
  );
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
  const savedTabMeta = parseTabMetaOrNull(localStorage.getItem(`${KEY_TAB_META}${suffix}`));

  const lastRunWasSuccessful = localStorage.getItem(`${KEY_LAST_RUN_COMPLETED}${suffix}`) !== 'false';

  const serverMeta = readVersionMetadata(userData?.initialComposition?.version.metadata);
  const serverDoc = userData?.initialComposition?.version.tree;

  const doc: CompositionDoc = savedDoc ?? serverDoc ?? buildDefaultDoc(DefaultCode);
  const savedActiveTreeId = localStorage.getItem(`${KEY_ACTIVE_TREE}${suffix}`);
  const activeTreeId = resolveActiveTreeId(doc, savedActiveTreeId ?? serverMeta?.activeTreeId);

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

  const tabMeta = resolveTabMeta(doc, serverMeta?.tabs, savedTabMeta);

  return { doc, activeTreeId, materials, tabMeta, lastRunWasSuccessful };
};

export const getView = (viz: Viz): MeshTabView => ({
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
  const docJson = JSON.stringify(state.doc);
  const materialsJson = JSON.stringify(state.materials);
  // Drafts must stay text-only (no textures/runtime assets) and far below localStorage limits.
  const bytes = docJson.length + materialsJson.length;
  if (bytes > 1_000_000) {
    console.warn(`geotoy draft is ${(bytes / 1e6).toFixed(2)}MB; drafts are expected to stay text-only`);
  }
  localStorage.setItem(`${KEY_DOC}${suffix}`, docJson);
  localStorage.setItem(`${KEY_MATERIALS}${suffix}`, materialsJson);
  // Written whole: the caller derives it from the live tab set, so a deleted tab's entry
  // disappears rather than lingering in a merge.
  localStorage.setItem(`${KEY_TAB_META}${suffix}`, JSON.stringify(state.tabMeta));
  localStorage.setItem(`${KEY_ACTIVE_TREE}${suffix}`, state.activeTreeId);
};

/** The version snapshot; `tabMeta` is supplied whole by the caller, already live-captured. */
export const buildCompositionVersionMetadata = (
  activeTreeId: string,
  materials: MaterialDefinitions,
  tabMeta: Record<string, TabMetadata>
): CompositionVersionMetadata => ({
  version: COMPOSITION_METADATA_VERSION,
  tabs: tabMeta,
  activeTreeId,
  materials,
});

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
  materials: MaterialDefinitions,
  tabMeta: Record<string, TabMetadata>,
  { title, description, isShared, tags }: CompositionMeta,
  userData?: GeoscriptPlaygroundUserData
): Promise<{ type: 'ok' } | { type: 'error'; msg: string }> => {
  try {
    const metadata = buildCompositionVersionMetadata(activeTreeId, materials, tabMeta);
    await Promise.all([
      createCompositionVersion(comp.id, { tree: currentDoc, metadata }),
      updateComposition(comp.id, ['title', 'description', 'is_shared', 'tags'], {
        title,
        description,
        is_shared: isShared,
        tags,
      }),
    ]);
    saveState({ doc: currentDoc, activeTreeId, materials, tabMeta }, userData);
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
export const getServerState = (userData: GeoscriptPlaygroundUserData | undefined): PlaygroundState => {
  const serverMeta = readVersionMetadata(userData?.initialComposition?.version.metadata);
  const doc = userData?.initialComposition?.version.tree ?? buildDefaultDoc(DefaultCode);
  return {
    doc,
    activeTreeId: resolveActiveTreeId(doc, serverMeta?.activeTreeId),
    materials: serverMeta?.materials || buildDefaultMaterialDefinitions(),
    tabMeta: resolveTabMeta(doc, serverMeta?.tabs, null),
    lastRunWasSuccessful: true,
  };
};

export const clearSavedState = (userData: GeoscriptPlaygroundUserData | undefined) => {
  const suffix = getLocalStorageKeySuffix(userData);
  for (const key of [KEY_DOC, KEY_MATERIALS, KEY_TAB_META, KEY_ACTIVE_TREE]) {
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
