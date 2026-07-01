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
    buildEmptyTree,
    type Composition,
    type CompositionVersion,
    type CompositionVersionMetadata,
    type TreeDef,
  } from 'src/geoscript/geotoyAPIClient';
  import { buildDefaultMaterialDefinitions } from 'src/geoscript/materials';
  import {
    DefaultCameraFOV,
    DefaultCameraPos,
    DefaultCameraTarget,
    DefaultCameraZoom,
    type MaterialOverrideMode,
  } from 'src/viz/scenes/geoscriptPlayground/types';

  interface TransientPayload {
    tree?: TreeDef;
    metadata?: Partial<CompositionVersionMetadata>;
    materialOverride?: MaterialOverrideMode;
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

    const tree: TreeDef = payload.tree ?? buildEmptyTree();
    const meta = payload.metadata ?? {};
    const autoFrame = !meta.view;
    const metadata: CompositionVersionMetadata = {
      view: meta.view ?? {
        cameraPosition: [DefaultCameraPos.x, DefaultCameraPos.y, DefaultCameraPos.z],
        target: [DefaultCameraTarget.x, DefaultCameraTarget.y, DefaultCameraTarget.z],
        fov: DefaultCameraFOV,
        zoom: DefaultCameraZoom,
      },
      materials: meta.materials ?? buildDefaultMaterialDefinitions(),
      preludeEjected: meta.preludeEjected ?? false,
      environment: meta.environment,
    };

    const now = new Date().toISOString();
    const comp: Composition = {
      id: -1,
      author_id: -1,
      author_username: '_transient',
      title: '_transient',
      description: '',
      created_at: now,
      updated_at: now,
      is_shared: false,
      is_featured: false,
    };
    const version: CompositionVersion = {
      id: -1,
      composition_id: -1,
      tree,
      created_at: now,
      metadata,
    };

    return {
      userData: {
        initialComposition: { comp, version },
        renderMode: true,
        transientAutoFrame: autoFrame,
        renderMaterialOverride: payload.materialOverride,
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
