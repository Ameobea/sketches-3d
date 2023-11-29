<script lang="ts">
  import SvelteSeo from 'svelte-seo';

  import { initViz, type VizState } from '.';
  import '../index.css';
  import { writable } from 'svelte/store';

  import PauseMenu from './PauseMenu/PauseMenu.svelte';
  import { type SceneConfig, ScenesByName } from './scenes';
  import DashChargeUI from './UI/DashChargeUI.svelte';

  export let sceneName: string;
  $: sceneDef = ScenesByName[sceneName];
  $: metadata = sceneDef.metadata;

  const paused = writable(false);
  const onResume = () => void paused.set(false);

  let viz: VizState | null = null;
  let sceneConfig: SceneConfig | null = null;
  const vizCb = (newViz: VizState, newSceneConfig: SceneConfig) => {
    viz = newViz;
    sceneConfig = newSceneConfig;
  };

  $: curDashCharges = sceneConfig?.player?.dashConfig?.chargeConfig?.curCharges;
</script>

{#if metadata}
  <SvelteSeo {...metadata} />
{/if}

<div use:initViz={{ paused, sceneName, vizCb }} />
{#if $paused && viz}
  <PauseMenu ctx={{ onResume }} {viz} />
{/if}

{#if curDashCharges}
  <DashChargeUI curCharges={$curDashCharges ?? 0} />
{/if}
