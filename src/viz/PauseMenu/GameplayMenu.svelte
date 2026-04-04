<script lang="ts">
  import type { VizConfig } from '../conf';

  export let onBack: () => void;
  export let onChange: (vizConf: VizConfig) => void;
  export let startVizConfig: VizConfig;

  let thirdPersonXray = startVizConfig.gameplay.thirdPersonXray;

  const handleSave = () => {
    onChange({
      ...startVizConfig,
      gameplay: { thirdPersonXray },
    });
    onBack();
  };
</script>

<div class="large-checkbox">
  <input id="third-person-xray-checkbox" type="checkbox" bind:checked={thirdPersonXray} />
  <label for="third-person-xray-checkbox">Third-Person X-Ray</label>
</div>

<button on:click={handleSave}>Save</button>
<button on:click={onBack}>Back</button>
