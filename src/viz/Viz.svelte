<script lang="ts">
  import SvelteSeo from 'svelte-seo';
  import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query';
  import { writable, type Writable } from 'svelte/store';

  import { initViz, type VizState } from '.';
  import '../index.css';
  import type { PopupScreenFocus } from './util.ts';
  import PauseMenu from './PauseMenu/PauseMenu.svelte';
  import { type SceneConfig, ScenesByName } from './scenes';
  import DashChargeUI from './UI/DashChargeUI.svelte';
  import { loadVizConfig, type VizConfig } from './conf';
  import { queryClient } from './queryClient';
  import InfiniteInitial from './InitialScreens/InfiniteInitial.svelte';
  export let sceneName: string;

  // svelte-ignore reactive_declaration_non_reactive_property
  $: sceneDef = ScenesByName[sceneName];
  $: metadata = sceneDef.metadata;

  const paused = writable(false);
  const popUpCalled = writable<PopupScreenFocus>({ type: 'pause' });

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
  <div use:initViz={{ paused, popUpCalled, sceneName, vizCb }} />
  {#if $paused && viz}
    {#if $popUpCalled.type === 'pause'}
      <PauseMenu ctx={{ onResume }} {viz} {sceneConfig} liveConfig={liveVizConfig} />
    {/if}
    {#if $popUpCalled.type === 'infinite'}
      <InfiniteInitial infiniteCtx={{ onResume }} {popUpCalled} onSubmit={$popUpCalled.cb} />
    {/if}
  {/if}

  {#if curDashCharges}
    <DashChargeUI curCharges={$curDashCharges ?? 0} />
  {/if}
</QueryClientProvider>
