<script lang="ts">
  import Viz from '../../viz/Viz.svelte';
  import type { PageData } from './$types';

  export let data: PageData;
</script>

<svelte:head>
  {#each data.preloadUrls ?? [] as url}
    <link rel="preload" as="fetch" crossorigin="anonymous" href={url} />
  {/each}
</svelte:head>

<Viz sceneName={data.sceneName} userData={data.levelDef ?? undefined} />
