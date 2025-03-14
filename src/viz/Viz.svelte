<script lang="ts">
  import SvelteSeo from 'svelte-seo';
  import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query';
  import { writable, type Writable } from 'svelte/store';

  import { initViz, type VizState } from '.';
  import '../index.css';

  import PauseMenu from './PauseMenu/PauseMenu.svelte';
  import { type SceneConfig, ScenesByName } from './scenes';
  import DashChargeUI from './UI/DashChargeUI.svelte';
  import { loadVizConfig, type VizConfig } from './conf';
  import { queryClient } from './queryClient';

  export let sceneName: string;
  // svelte-ignore reactive_declaration_non_reactive_property
  $: sceneDef = ScenesByName[sceneName];
  $: metadata = sceneDef.metadata;

  const paused = writable(false);
  const onResume = () => void paused.set(false);

  let viz: VizState | null = null;
  let liveVizConfig = writable<VizConfig>(loadVizConfig());
  let sceneConfig: SceneConfig | null = null;
  const vizCb = (newViz: VizState, newLiveVizConfig: Writable<VizConfig>, newSceneConfig: SceneConfig) => {
    viz = newViz;
    liveVizConfig = newLiveVizConfig;
    sceneConfig = newSceneConfig;
  };

  $: curDashCharges = sceneConfig?.player?.dashConfig?.chargeConfig?.curCharges;
</script>

<QueryClientProvider client={queryClient}>
  {#if metadata}
    <SvelteSeo {...metadata} />
  {/if}

  <!-- svelte-ignore element_invalid_self_closing_tag -->
  <div use:initViz={{ paused, sceneName, vizCb }} />
  {#if $paused && viz}
    <PauseMenu ctx={{ onResume }} {viz} {sceneConfig} liveConfig={liveVizConfig} />
  {/if}

  {#if curDashCharges}
    <DashChargeUI curCharges={$curDashCharges ?? 0} />
  {/if}
</QueryClientProvider>
