<script lang="ts">
  import { get } from 'svelte/store';
  import type { Viz } from '..';
  import type { GameplaySettings, VizConfig } from '../conf';

  export let viz: Viz;
  export let onBack: () => void;
  export let onChange: (vizConf: VizConfig) => void;
  export let startVizConfig: VizConfig;

  let easyModeMovement = viz.fpCtx ? get(viz.fpCtx.easyModeMovement) : false;
  let thirdPersonXray = startVizConfig.gameplay.thirdPersonXray;

  const handleSave = () => {
    const newGameplaySettings: GameplaySettings = { easyModeMovement, thirdPersonXray };
    onChange({
      ...startVizConfig,
      gameplay: newGameplaySettings,
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

<div class="large-checkbox">
  <input id="third-person-xray-checkbox" type="checkbox" bind:checked={thirdPersonXray} />
  <label for="third-person-xray-checkbox">Third-Person X-Ray</label>
</div>

<button on:click={handleSave}>Save</button>
<button on:click={onBack}>Back</button>
