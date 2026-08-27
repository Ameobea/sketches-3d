<script lang="ts">
  import * as OrbitControls from 'three/examples/jsm/controls/OrbitControls.js';
  import { browser } from '$app/environment';

  import Viz from 'src/viz/Viz.svelte';
  import {
    processLoadedScene,
    type GeoscriptPlaygroundUserData,
  } from 'src/viz/scenes/geoscriptPlayground/geoscriptPlayground.svelte';
  import { LoadOrbitControls } from 'src/viz/preloadCache';
  import { SCENE_REGISTRY } from 'src/viz/scenes/sceneRegistry';
  import { WorkerManager } from 'src/geoscript/workerManager';
  import {
    buildEmptyDoc,
    defaultTreeEntry,
    withTree,
    type Composition,
    type CompositionDoc,
    type CompositionVersion,
    type CompositionVersionMetadata,
    COMPOSITION_METADATA_VERSION,
    type TransientRenderMetadata,
  } from 'src/geoscript/geotoyAPIClient';
  import { buildDefaultMaterialDefinitions } from 'src/geoscript/materials';
  import { DefaultView, type MaterialOverrideMode } from 'src/geotoy/types';
  import type { EvalRequest } from 'src/geotoy/modes/mesh/evalResult';

  interface TransientPayload {
    tree?: CompositionDoc;
    /**
     * Either a full `CompositionVersionMetadata` (passed straight through — what a DB row
     * already holds), or the flat single-tree form the CLI finds convenient to author.
     */
    metadata?: CompositionVersionMetadata | TransientRenderMetadata;
    materialOverride?: MaterialOverrideMode;
    eval?: EvalRequest;
  }

  LoadOrbitControls.getter = async () => OrbitControls;

  const readPayload = (): TransientPayload | null => {
    if (!browser) return null;
    const w = window as any;
    if (w.__transientCompositionPayload && typeof w.__transientCompositionPayload === 'object') {
      return w.__transientCompositionPayload as TransientPayload;
    }
    return null;
  };

  const buildUserData = (): { userData: GeoscriptPlaygroundUserData | null; error: string | null } => {
    const payload = readPayload();
    if (!payload) {
      return {
        userData: null,
        error: 'No transient composition payload found on window.__transientCompositionPayload',
      };
    }

    let doc: CompositionDoc = payload.tree ?? buildEmptyDoc();
    const entry = defaultTreeEntry(doc);
    // `eval --expr` is appended to the default tree's root source as a trailing expression so
    // it's evaluated as part of the (optimized) run; its value becomes the run's captured last value.
    if (payload.eval?.expr) {
      const root = entry.tree.nodes[entry.tree.rootId];
      if (root) {
        doc = withTree(doc, entry.id, {
          ...entry.tree,
          nodes: {
            ...entry.tree.nodes,
            [entry.tree.rootId]: { ...root, source: `${root.source}\n(${payload.eval.expr})\n` },
          },
        });
      }
    }
    const supplied = payload.metadata;
    // A v1 blob is already exactly what the app expects; only the flat CLI form needs shaping.
    const flat: TransientRenderMetadata =
      supplied && 'version' in supplied ? {} : ((supplied ?? {}) as TransientRenderMetadata);
    const autoFrame = supplied && 'version' in supplied ? false : !flat.view;
    const metadata: CompositionVersionMetadata =
      supplied && 'version' in supplied
        ? supplied
        : {
            version: COMPOSITION_METADATA_VERSION,
            tabs: {
              [entry.id]: {
                kind: 'mesh',
                preludeEjected: flat.preludeEjected ?? false,
                view: flat.view ?? DefaultView,
                environment: flat.environment,
                emissiveBloom: flat.emissiveBloom,
              },
            },
            activeTreeId: entry.id,
            materials: flat.materials ?? buildDefaultMaterialDefinitions(),
          };

    const now = new Date().toISOString();
    const comp: Composition = {
      id: -1,
      author_id: -1,
      author_username: '_transient',
      title: '_transient',
      description: '',
      tags: [],
      created_at: now,
      updated_at: now,
      is_shared: false,
      is_featured: false,
    };
    const version: CompositionVersion = {
      id: -1,
      composition_id: -1,
      tree: doc,
      created_at: now,
      metadata,
    };

    return {
      userData: {
        initialComposition: { comp, version },
        renderMode: true,
        transientAutoFrame: autoFrame,
        renderMaterialOverride: payload.materialOverride,
        failRenderOnError: true,
        evalRequest: payload.eval,
        me: null,
        workerManager: browser ? new WorkerManager() : null,
      },
      error: null,
    };
  };

  const built = $derived(buildUserData());

  const { modulePath: _modulePath, ...geoscriptData } = SCENE_REGISTRY['geoscript'];
  const sceneDef = { ...geoscriptData, sceneLoader: () => processLoadedScene };
</script>

{#if built.error}
  <pre style="color:red;padding:1em;">{built.error}</pre>
{:else if built.userData}
  <Viz sceneName="geoscript" userData={built.userData} {sceneDef} />
{/if}
