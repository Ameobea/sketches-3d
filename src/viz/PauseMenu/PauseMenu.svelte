<script lang="ts" context="module">
  interface PauseMenuCtx {
    onResume: () => void;
  }

  enum Menu {
    Main,
    Graphics,
    Gameplay,
    Controls,
    Audio,
    Login,
  }
</script>

<script lang="ts">
  import { derived, type Writable } from 'svelte/store';

  import { applyAudioSettings, type VizState } from '..';
  import { type AudioSettings, loadVizConfig, type VizConfig } from '../conf';
  import AudioMenu from './AudioMenu.svelte';
  import ControlsMenu from './ControlsMenu.svelte';
  import GraphicsMenu from './GraphicsMenu.svelte';
  import type { SceneConfig } from '../scenes';
  import GameplayMenu from './GameplayMenu.svelte';
  import LoginMenu from './LoginMenu.svelte';

  let activeMenu = Menu.Main;

  export let ctx: PauseMenuCtx;
  export let viz: VizState;
  export let liveConfig: Writable<VizConfig>;
  export let sceneConfig: SceneConfig | null;
  $: customControlsEntries = sceneConfig?.customControlsEntries ?? [];

  const startVizConfig = loadVizConfig();

  const saveNewConfig = (newVizConfig: VizConfig) => {
    liveConfig.set(newVizConfig);
    localStorage.setItem('vizConfig', JSON.stringify(newVizConfig));
    activeMenu = Menu.Main;
  };

  const audioConf = derived(liveConfig, $newConfig => $newConfig.audio);
  const onAudioConfChanged = (newAudioConf: AudioSettings) => {
    applyAudioSettings(newAudioConf);
    liveConfig.update(conf => ({ ...conf, audio: newAudioConf }));
  };

  const controlsConf = derived(liveConfig, $newConfig => $newConfig.controls);
  const onControlsConfChanged = (newControlsConf: any) => {
    liveConfig.update(conf => ({ ...conf, controls: newControlsConf }));
  };

  const commit = () => saveNewConfig($liveConfig);
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
  <div class="pause-menu">
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
        <button
          on:click={() => {
            activeMenu = Menu.Gameplay;
          }}
        >
          Gameplay
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
        <button
          on:click={() => {
            activeMenu = Menu.Login;
          }}
        >
          Login / Register
        </button>
      {:else if activeMenu === Menu.Graphics}
        {#if sceneConfig}
          <GraphicsMenu
            onBack={() => {
              activeMenu = Menu.Main;
            }}
            onChange={saveNewConfig}
            {startVizConfig}
            {sceneConfig}
            {viz}
          />
        {:else}
          Loading...
        {/if}
      {:else if activeMenu === Menu.Audio}
        <AudioMenu
          {viz}
          onBack={() => {
            activeMenu = Menu.Main;
          }}
          conf={audioConf}
          onChange={onAudioConfChanged}
        />
      {:else if activeMenu === Menu.Controls}
        <ControlsMenu
          conf={controlsConf}
          onChange={onControlsConfChanged}
          customEntries={customControlsEntries}
        />
        <button
          on:click={() => {
            activeMenu = Menu.Main;
          }}
        >
          Back
        </button>
      {:else if activeMenu === Menu.Gameplay}
        <GameplayMenu
          onBack={() => {
            activeMenu = Menu.Main;
          }}
          onChange={saveNewConfig}
          {startVizConfig}
          {viz}
        />
      {:else if activeMenu === Menu.Login}
        <LoginMenu
          onBack={() => {
            activeMenu = Menu.Main;
          }}
        />
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

  .pause-menu {
    display: flex;
    flex-direction: column;
    align-items: center;
    height: 100%;
    background-color: rgba(0, 0, 0, 0.8);
    border: 1px solid #111;
    font-family: 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
    max-width: 1200px;
    width: 100%;
    color: #eee;
  }

  :global(.pause-menu h3) {
    text-align: center;
    margin-top: 2px;
    margin-bottom: 16px;
    font-family: 'Roboto Mono', 'Courier New', Courier, monospace;
    text-transform: uppercase;
    margin-bottom: 4px;
    margin-top: 4px;
    font-size: 24px;
    font-weight: 500;
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

  :global(
      .menu-items-stack > div,
      .menu-items-stack > button,
      .menu-items-stack > input,
      .menu-items-stack-item
    ) {
    height: 60px;
    font-size: 24px;
    padding: 10px;
    border: 1px solid #ddd;
    background-color: rgba(0, 0, 0, 0.3);
    color: #eee;
    text-transform: uppercase;
    outline: none;
  }

  :global(.menu-items-stack button, .menu-items-stack input) {
    cursor: pointer;
  }

  :global(.menu-items-stack input[type='text'], .menu-items-stack input[type='password']) {
    cursor: unset;
  }

  :global(.menu-items-stack button:hover) {
    background-color: rgba(58, 58, 58, 0.3);
  }

  :global(.menu-items-stack button:disabled) {
    background-color: rgba(0, 0, 0, 0.3) !important;
    color: #777 !important;
    cursor: default !important;
  }

  :global(.large-checkbox) {
    position: relative;
    width: 100%;
    height: 60px;
    margin: 10px 0;
    display: flex;
    align-items: center;
  }

  :global(.large-checkbox input[type='checkbox']) {
    appearance: none;
    -webkit-appearance: none;
    -moz-appearance: none;
    width: 44px;
    height: 44px;
    border: 3px solid #eee;
    background-color: transparent;
    cursor: pointer;
    align-self: center;
    position: absolute;
    margin: 0;
  }

  :global(.large-checkbox label) {
    margin-left: 10px;
    font-size: 24px;
    color: #eee;
    cursor: pointer;
    width: 100%;
    text-align: center;
  }

  :global(.large-checkbox input[type='checkbox']:before) {
    content: '';
    position: absolute;
    top: 7px;
    left: 7px;
    right: 7px;
    bottom: 7px;
    background-color: transparent;
  }

  :global(.large-checkbox input[type='checkbox']:checked:before) {
    background-color: #d8d8d8;
  }

  :global(.large-checkbox input[type='checkbox']:hover:not(:checked):before) {
    background-color: #d8d8d844;
  }

  :global(.large-checkbox input[type='checkbox']:checked:hover:before) {
    background-color: #d8d8d8cc;
  }
</style>
