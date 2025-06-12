<script lang="ts" context="module">
  interface InitialInfiniteCtx {
    onResume: () => void;
  }
</script>

<script lang="ts">
  import { type Writable } from 'svelte/store';
  import type { InfiniteConfig, PopupScreenFocus } from '../util';

  export let infiniteCtx: InitialInfiniteCtx;
  export let popUpCalled: Writable<PopupScreenFocus>;
  export let onSubmit: (config: InfiniteConfig) => void;

  let seed = '';
  //min is two and  will not go lower
  let activePathLength = 5;
  //min is one and will overide
  let goalLength = 10;
  let timerActive = true;
  let varyingGaps = false;
</script>

<!-- svelte-ignore a11y-click-events-have-key-events -->
<!-- svelte-ignore a11y-no-static-element-interactions -->
<div class="backdrop">
  <div class="pause-menu">
    <div class="menu-items-stack">
      <h1 style="margin-left: auto;margin-right: auto;">Initial Settings</h1>
      <h3>Seed</h3>
      <input type="text" bind:value={seed} />
      <h3>Active Path Length</h3>
      <input type="number" bind:value={activePathLength} />
      <h3>Goal Length</h3>
      <input type="number" bind:value={goalLength} />
      <div class="large-checkbox">
        <input id="timer-active-checkbox" type="checkbox" bind:checked={timerActive} />
        <label for="timer-active-checkbox">Timer Active</label>
      </div>
      <div class="large-checkbox">
        <input id="varying-gaps-checkbox" type="checkbox" bind:checked={varyingGaps} />
        <label for="colored-path-checkbox">Varying Gaps Length</label>
      </div>
      <button
        on:click={evt => {
          if (evt.target === evt.currentTarget) {
            onSubmit({
              seed,
              activePathLength: +activePathLength,
              goalLength: +goalLength,
              timerActive: timerActive,
              varyingGaps: varyingGaps,
            } satisfies InfiniteConfig);

            popUpCalled.set({ type: 'pause' });
            infiniteCtx.onResume();
          }
        }}
      >
        Start
      </button>
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
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
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
