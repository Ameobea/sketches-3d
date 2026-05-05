<script lang="ts">
  import * as OrbitControls from 'three/examples/jsm/controls/OrbitControls.js';
  import { browser } from '$app/environment';

  import Viz from 'src/viz/Viz.svelte';
  import type { PageData } from './$types';
  import {
    processLoadedScene,
    type GeoscriptPlaygroundUserData,
  } from 'src/viz/scenes/geoscriptPlayground/geoscriptPlayground.svelte';
  import { SCENE_REGISTRY } from 'src/viz/scenes/sceneRegistry';
  import { LoadOrbitControls } from 'src/viz/preloadCache';
  import { WorkerManager } from 'src/geoscript/workerManager';

  // pre-load orbit controls because they will be needed later
  LoadOrbitControls.getter = async () => OrbitControls;

  let { data }: { data: PageData } = $props();

  let userData: GeoscriptPlaygroundUserData = $derived({
    me: data.me,
    renderMode: false,
    initialComposition: null,
    // also kick off fetching the worker script + initializing the worker as soon as possible
    workerManager: browser ? new WorkerManager() : null,
  });

  const { modulePath: _modulePath, ...geoscriptData } = SCENE_REGISTRY['geoscript'];
  const sceneDef = { ...geoscriptData, sceneLoader: () => processLoadedScene };
</script>

<Viz sceneName="geoscript" {userData} {sceneDef} />
