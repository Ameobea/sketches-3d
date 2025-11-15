<script lang="ts">
  import * as OrbitControls from 'three/examples/jsm/controls/OrbitControls.js';
  import { browser } from '$app/environment';

  import Viz from 'src/viz/Viz.svelte';
  import type { PageData } from './$types';
  import {
    processLoadedScene,
    type GeoscriptPlaygroundUserData,
  } from 'src/viz/scenes/geoscriptPlayground/geoscriptPlayground.svelte';
  import { page } from '$app/state';
  import { LoadOrbitControls } from 'src/viz/preloadCache';
  import { ScenesByName } from 'src/viz/scenes';
  import { WorkerManager } from 'src/geoscript/workerManager';

  let { data }: { data: PageData } = $props();

  let renderMode = $derived(page.url.searchParams.get('render') === 'true');

  // pre-load orbit controls because they will be needed later
  LoadOrbitControls.getter = async () => OrbitControls;

  // TODO: we should fetch used textures in the load and include them here
  let userData: GeoscriptPlaygroundUserData = $derived({
    initialComposition: data,
    renderMode,
    me: data.me,
    // also kick off fetching the worker script + initializing the worker as soon as possible
    workerManager: browser ? new WorkerManager() : null,
  });

  const sceneDefOverride = { ...ScenesByName['geoscript'], sceneLoader: () => processLoadedScene };
</script>

<Viz sceneName="geoscript" {userData} {sceneDefOverride} />
