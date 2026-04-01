<script lang="ts">
  import { writable, type Writable } from 'svelte/store';

  import { initViz, type Viz } from '.';
  import '../index.css';
  import type { PopupScreenFocus } from './util/util.ts';
  import PauseMenu from './PauseMenu/PauseMenu.svelte';
  import { type SceneConfig, type SceneDef, ScenesByName } from './scenes';
  import DashChargeUI from './UI/DashChargeUI.svelte';
  import { loadVizConfig, type VizConfig } from './conf';
  import InfiniteInitial from './InitialScreens/InfiniteInitial.svelte';
  import { rwritable } from './util/TransparentWritable';

  export let sceneName: string;
  export let userData: any = undefined;
  export let sceneDefOverride: SceneDef | undefined = undefined;

  $: sceneDef = sceneDefOverride ?? ScenesByName[sceneName];
  $: metadata = sceneDef?.metadata;

  const paused = rwritable(false);
  const popUpCalled = rwritable<PopupScreenFocus>({ type: 'pause' });

  const onResume = () => void paused.set(false);

  let viz: Viz | null = null;
  let liveVizConfig = writable<VizConfig>(loadVizConfig());
  let sceneConfig: SceneConfig | null = null;
  const vizCb = (newViz: Viz, newLiveVizConfig: Writable<VizConfig>, newSceneConfig: SceneConfig) => {
    viz = newViz;
    liveVizConfig = newLiveVizConfig;
    sceneConfig = newSceneConfig;
  };

  $: curDashCharges = sceneConfig?.player?.dashConfig?.chargeConfig?.curCharges;

  // When sceneName changes (same [scene] catch-all route, different scene), reset stale references
  // so the UI doesn't hold onto the old destroyed viz during the transition.
  $: {
    void sceneName;
    viz = null;
    sceneConfig = null;
  }
</script>

<svelte:head>
  {#if metadata}
    <title>{metadata.title}</title>
    {#if metadata.description}
      <meta name="description" content={metadata.description} />
    {/if}
    {#if metadata.openGraph?.title ?? metadata.title}
      <meta property="og:title" content={metadata.openGraph?.title ?? metadata.title} />
    {/if}
    {#if metadata.openGraph?.description ?? metadata.description}
      <meta property="og:description" content={metadata.openGraph?.description ?? metadata.description} />
    {/if}
    {#if metadata.openGraph?.images?.[0]}
      <meta property="og:image" content={metadata.openGraph.images[0].url} />
      {#if metadata.openGraph.images[0].alt}
        <meta property="og:image:alt" content={metadata.openGraph.images[0].alt} />
      {/if}
    {/if}
  {/if}
</svelte:head>

<!-- {#key} forces the action to destroy+reinit when sceneName changes on the same [scene] route -->
{#key sceneName}
  <!-- svelte-ignore element_invalid_self_closing_tag -->
  <div
    use:initViz={{ paused, popUpCalled, sceneName, vizCb, userData, sceneDefOverride }}
    id="viz-container"
  />
{/key}
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
