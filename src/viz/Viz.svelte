<script lang="ts">
  import SvelteSeo from 'svelte-seo';

  import { initViz } from '.';
  import '../index.css';
  import { writable } from 'svelte/store';

  import PauseMenu from './PauseMenu/PauseMenu.svelte';
  import { ScenesByName } from './scenes';

  export let sceneName: string;
  $: metadata = ScenesByName[sceneName]?.metadata;

  const paused = writable(false);
  const globalVolume = writable(0.8);

  const onResume = () => void paused.set(false);
</script>

{#if metadata}
  <SvelteSeo {...metadata} />
{/if}

<div use:initViz={{ paused, sceneName }} />
{#if $paused}
  <PauseMenu ctx={{ onResume, globalVolume }} />
{/if}
