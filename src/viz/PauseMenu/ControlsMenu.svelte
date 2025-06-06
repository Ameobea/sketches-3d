<script lang="ts">
  import type { Readable } from 'svelte/store';
  import type { CustomControlsEntry } from '../scenes';
  import type { ControlsSettings } from '../conf';
  import type { Viz } from '..';

  export let viz: Viz;
  export let customEntries: CustomControlsEntry[] = [];
  export let conf: Readable<ControlsSettings>;
  export let onChange: (newControlsConf: ControlsSettings) => void;

  $: isFirstPerson = viz.viewMode && viz.viewMode.type === 'firstPerson';
  $: dashEnabled = viz.sceneConf?.player?.dashConfig?.enable !== false;
</script>

<div style="display: flex; flex-direction: row; gap: 5px">
  <div class="slider-input" style="flex: 1">
    <label for="mouse-sensitivity-slider">Mouse Sensitivity</label>
    <input
      type="range"
      id="mouse-sensitivity-slider"
      name="mouse-sensitivity"
      min="0.01"
      max="10"
      step="0.01"
      value={$conf.mouseSensitivity}
      on:input={e => onChange({ ...$conf, mouseSensitivity: +e.currentTarget.value })}
    />
  </div>
  <div style="font-size: 12px; display: flex; align-items: flex-end;">
    {$conf.mouseSensitivity.toFixed(2)}
  </div>
</div>

{#if isFirstPerson}
  <div class="control-mapping">
    <div>Move</div>
    <div>WASD</div>
  </div>
  <div class="control-mapping">
    <div>Jump</div>
    <div>Space</div>
  </div>
  {#if dashEnabled}
    <div class="control-mapping">
      <div>Dash</div>
      <div>Shift</div>
    </div>
  {/if}
{/if}
{#each customEntries as { key, label }}
  <div class="control-mapping">
    <div>{label}</div>
    <div>{key}</div>
  </div>
{/each}

<style lang="css">
  .control-mapping {
    display: flex;
    flex-direction: row;
    justify-content: space-between;
    align-items: center;
  }
</style>
