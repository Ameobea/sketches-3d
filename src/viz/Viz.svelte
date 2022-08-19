<script lang="ts">
  import SvelteSeo from 'svelte-seo';

  import { initViz } from '.';
  import '../index.css';
  import { ScenesByName } from './scenes';

  export let sceneName: string;
  $: metadata = ScenesByName[sceneName]?.metadata;
</script>

{#if metadata}
  <SvelteSeo {...metadata} />
{/if}

<div use:initViz={sceneName} />
