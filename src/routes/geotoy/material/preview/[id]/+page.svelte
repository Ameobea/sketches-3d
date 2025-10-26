<script lang="ts">
  import * as Comlink from 'comlink';
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
  import type { GeoscriptWorkerMethods } from 'src/geoscript/geoscriptWorker.worker';
  import GeoscriptWorker from 'src/geoscript/geoscriptWorker.worker?worker';
  import { ScenesByName } from 'src/viz/scenes';

  let { data }: { data: PageData } = $props();

  let renderMode = $derived(page.url.searchParams.get('render') === 'true');

  // pre-load orbit controls because they will be needed later
  LoadOrbitControls.getter = async () => OrbitControls;

  let userData: GeoscriptPlaygroundUserData = $derived({
    initialComposition: { comp: data.comp, version: data.version },
    renderMode,
    me: null,
    // also kick off fetching the worker script + initializing the worker as soon as possible
    geoscriptWorker: browser ? Comlink.wrap<GeoscriptWorkerMethods>(new GeoscriptWorker()) : null,
  });

  const sceneDefOverride = { ...ScenesByName['geoscript'], sceneLoader: () => processLoadedScene };
</script>

<Viz sceneName="geoscript" {userData} {sceneDefOverride} />
