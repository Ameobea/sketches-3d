<script lang="ts" context="module">
  interface PauseMenuCtx {
    onResume: () => void;
  }

  enum Menu {
    Main,
    Graphics,
    Controls,
    Audio,
  }
</script>

<script lang="ts">
  import { derived, writable } from 'svelte/store';

  import { applyAudioSettings, type VizState } from '..';
  import { type AudioSettings, loadVizConfig, type VizConfig } from '../conf';
  import AudioMenu from './AudioMenu.svelte';
  import ControlsMenu from './ControlsMenu.svelte';
  import GraphicsMenu from './GraphicsMenu.svelte';
  import type { SceneConfig } from '../scenes';

  let activeMenu = Menu.Main;

  export let ctx: PauseMenuCtx;
  export let viz: VizState;
  export let sceneConfig: SceneConfig | null;
  $: customControlsEntries = sceneConfig?.customControlsEntries ?? [];

  const startVizConfig = loadVizConfig();
  const newConfig = writable(startVizConfig);

  const saveNewConfig = (newVizConfig: VizConfig) => {
    localStorage.setItem('vizConfig', JSON.stringify(newVizConfig));
    activeMenu = Menu.Main;
  };

  const audioConf = derived(newConfig, $newConfig => $newConfig.audio);
  const onAudioConfChanged = (newAudioConf: AudioSettings) => {
    applyAudioSettings(newAudioConf);
    newConfig.update(conf => ({ ...conf, audio: newAudioConf }));
  };

  const commit = () => saveNewConfig($newConfig);
</script>

<!-- svelte-ignore a11y-click-events-have-key-events -->
<!-- svelte-ignore a11y-no-static-element-interactions -->
<div
  class="backdrop"
  on:click={evt => {
    if (evt.target === evt.currentTarget) {
      commit();
      ctx.onResume();
    }
  }}
>
  <div class="root">
    <h2>PAUSED</h2>

    <div class="menu-items-stack">
      {#if activeMenu === Menu.Main}
        <button
          on:click={() => {
            commit();
            ctx.onResume();
          }}
        >
          Resume
        </button>
        <button
          on:click={() => {
            activeMenu = Menu.Graphics;
          }}
        >
          Graphics
        </button>
        <div class="slider-input">
          <label for="global-volume-slider">Global Volume</label>
          <input
            type="range"
            id="global-volume-slider"
            name="volume"
            min="0"
            max="1"
            step="0.01"
            value={$audioConf.globalVolume}
            on:input={evt => {
              onAudioConfChanged({
                ...$audioConf,
                globalVolume: +evt.currentTarget.value,
              });
            }}
          />
        </div>
        <button
          on:click={() => {
            activeMenu = Menu.Audio;
          }}
        >
          Audio
        </button>
        <button
          on:click={() => {
            activeMenu = Menu.Controls;
          }}
        >
          Controls
        </button>
      {:else if activeMenu === Menu.Graphics}
        <GraphicsMenu
          onBack={() => {
            activeMenu = Menu.Main;
          }}
          onChange={saveNewConfig}
          {startVizConfig}
          {viz}
        />
      {:else if activeMenu === Menu.Audio}
        <AudioMenu
          onBack={() => {
            activeMenu = Menu.Main;
          }}
          conf={audioConf}
          onChange={onAudioConfChanged}
        />
      {:else if activeMenu === Menu.Controls}
        <ControlsMenu customEntries={customControlsEntries} />
        <button
          on:click={() => {
            activeMenu = Menu.Main;
          }}
        >
          Back
        </button>
      {/if}
    </div>
  </div>
</div>

<style lang="css">
  .backdrop {
    display: flex;
    justify-content: center;
    align-items: center;
    position: fixed;
    top: 0;
    left: 0;
    z-index: 100;
    width: 100vw;
    height: 100vh;
    background-color: rgba(0, 0, 0, 0.8);
    padding-top: calc(max(20px, 4vh));
    padding-left: calc(max(40px, 8vw));
    padding-right: calc(max(40px, 8vw));
    padding-bottom: calc(max(20px, 4vh));
    min-width: 800px;
    min-height: 600px;
  }

  .root {
    display: flex;
    flex-direction: column;
    align-items: center;
    height: 100%;
    background-color: rgba(0, 0, 0, 0.8);
    border: 1px solid #111;
    font-family: 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
    max-width: 1200px;
    width: 100%;
  }

  h2 {
    font-size: 48px;
    font-weight: 500;
    position: absolute;
    top: 50px;
    letter-spacing: 1.8px;
  }

  :global(.slider-input label) {
    font-size: 14px;
    font-weight: 500;
    margin-top: 2px;
    margin-bottom: 5px;
    text-align: center;
    display: block;
    width: 100%;
  }

  :global(.slider-input input[type='range']) {
    display: block;
    width: 100%;
    margin-top: 0px;
    margin-bottom: 6px;
  }

  :global(.menu-items-stack) {
    display: flex;
    flex-direction: column;
    gap: 20px;
    width: 400px;
    margin-top: auto;
    margin-bottom: auto;
  }

  :global(.menu-items-stack > *) {
    height: 60px;
    font-size: 24px;
    padding: 10px;
    border: 1px solid #ddd;
    background-color: rgba(0, 0, 0, 0.3);
    color: #eee;
    text-transform: uppercase;
    outline: none;
  }

  :global(.menu-items-stack > button, .menu-items-stack input) {
    cursor: pointer;
  }

  :global(.menu-items-stack > button:hover) {
    background-color: rgba(18, 18, 18, 0.4);
  }

  :global(.menu-items-stack > button:disabled) {
    background-color: rgba(0, 0, 0, 0.3) !important;
  }
</style>
