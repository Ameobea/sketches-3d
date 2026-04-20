<script lang="ts">
  import { browser } from '$app/environment';
  import { onDestroy } from 'svelte';

  import Viz from '../../viz/Viz.svelte';
  import { GeoscriptExecutor } from 'src/geoscript/geoscriptExecutor';
  import type { PageData } from './$types';

  export let data: PageData;

  // Spawn the geoscript worker as early as possible — at component mount, before
  // Viz mounts and before the GLTF fetch starts — so the worker boot + wasm fetches
  // overlap with renderer setup instead of waterfalling behind it.  Owned here and
  // forwarded to Viz; recreated on scene change, terminated on navigation away.
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
