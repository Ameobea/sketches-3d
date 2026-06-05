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
  } from 'src/viz/scenes/geoscriptPlayground/types';

  interface TransientPayload {
    tree?: TreeDef;
    metadata?: Partial<CompositionVersionMetadata>;
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

  let payloadError = $state<string | null>(null);

  const buildUserData = (): GeoscriptPlaygroundUserData | null => {
    const payload = readPayload();
    if (!payload) {
      payloadError = 'No transient composition payload found on window.__transientCompositionPayload';
      return null;
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
      initialComposition: { comp, version },
      renderMode: true,
      transientAutoFrame: autoFrame,
      me: null,
      workerManager: browser ? new WorkerManager() : null,
    };
  };

  let userData = $derived(buildUserData());

  const { modulePath: _modulePath, ...geoscriptData } = SCENE_REGISTRY['geoscript'];
  const sceneDef = { ...geoscriptData, sceneLoader: () => processLoadedScene };
</script>

{#if payloadError}
  <pre style="color:red;padding:1em;">{payloadError}</pre>
{:else if userData}
  <Viz sceneName="geoscript" {userData} {sceneDef} />
{/if}
