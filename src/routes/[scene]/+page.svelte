<script lang="ts">
  import { browser } from '$app/environment';
  import { onDestroy } from 'svelte';

  import Viz from '../../viz/Viz.svelte';
  import { ScenesByName } from 'src/viz/scenes';
  import { GeoscriptExecutor } from 'src/geoscript/geoscriptExecutor';
  import type { PageData } from './$types';

  export let data: PageData;

  // Kick off the scene's dynamic-import chunk fetch as early as possible (in parallel
  // with Viz.svelte mounting + the rest of the static-import waterfall).  Vite's
  // `__vitePreload` helper emits modulepreloads for all transitive deps when this
  // fires, so the scene-specific chunks (~12 chunks / ~425 KiB for factory) start
  // downloading immediately rather than waiting for `initViz()` deep inside
  // `Viz.svelte`'s action.  Browser caches the import, so initViz's later call
  // resolves against the same fetch.
  if (browser) {
    const loader = ScenesByName[data.sceneName]?.sceneLoader;
    if (loader) {
      Promise.resolve(loader()).catch(() => {});
    }
  }

  let geoscriptExecutor: GeoscriptExecutor | undefined = undefined;
  let executorSceneName: string | null = null;

  $: if (browser) {
    const wantExecutor = !!data.levelDef;
    if (wantExecutor && executorSceneName !== data.sceneName) {
      geoscriptExecutor?.terminate();
      geoscriptExecutor = new GeoscriptExecutor();
      executorSceneName = data.sceneName;
    } else if (!wantExecutor && geoscriptExecutor) {
      geoscriptExecutor.terminate();
      geoscriptExecutor = undefined;
      executorSceneName = null;
    }
  }

  onDestroy(() => {
    geoscriptExecutor?.terminate();
    geoscriptExecutor = undefined;
    executorSceneName = null;
  });
</script>

<svelte:head>
  {#each data.preloadUrls ?? [] as url}
    <link rel="preload" as="fetch" crossorigin="anonymous" href={url} />
  {/each}
</svelte:head>

<Viz sceneName={data.sceneName} userData={data.levelDef ?? undefined} {geoscriptExecutor} />
