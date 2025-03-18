<script lang="ts">
  import { get } from 'svelte/store';
  import type { Viz } from '..';
  import type { GameplaySettings, VizConfig } from '../conf';

  export let viz: Viz;
  export let onBack: () => void;
  export let onChange: (vizConf: VizConfig) => void;
  export let startVizConfig: VizConfig;

  let easyModeMovement = viz.fpCtx ? get(viz.fpCtx.easyModeMovement) : false;

  const handleSave = () => {
    const newGraphicsSettings: GameplaySettings = { easyModeMovement };
    onChange({
      ...startVizConfig,
      gameplay: newGraphicsSettings,
    });
    onBack();
  };
</script>

<div class="large-checkbox">
  <input
    id="easy-mode-movement-checkbox"
    type="checkbox"
    disabled={!viz.fpCtx}
    bind:checked={easyModeMovement}
  />
  <label for="easy-mode-movement-checkbox">Easy Mode Movement</label>
</div>

<button on:click={handleSave}>Save</button>
<button on:click={onBack}>Back</button>
