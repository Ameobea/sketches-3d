<script lang="ts">
  import { formatGraphicsQuality, GraphicsQuality, type VizConfig } from '../conf';
  import SelectButton from './SelectButton.svelte';

  export let onBack: () => void;
  export let onChange: (vizConf: VizConfig) => void;
  export let startVizConfig: VizConfig;

  let graphicsQuality: GraphicsQuality = startVizConfig.graphics.quality;

  const AllQualities = [GraphicsQuality.Low, GraphicsQuality.Medium, GraphicsQuality.High];
  const AllQualityNames = AllQualities.map(formatGraphicsQuality);
  const handleQualityChange = (newQualityIx: number) => {
    graphicsQuality = AllQualities[newQualityIx];
  };

  $: graphicsSettingsChanged = graphicsQuality !== startVizConfig.graphics.quality;
</script>

<SelectButton
  onChange={handleQualityChange}
  curIx={AllQualities.indexOf(graphicsQuality)}
  options={['low', 'medium', 'high']}
/>
<button
  disabled={!graphicsSettingsChanged}
  on:click={() => {
    onChange({
      ...startVizConfig,
      graphics: {
        ...startVizConfig.graphics,
        quality: graphicsQuality,
      },
    });
    localStorage['goBackOnLoad'] = 'true';
    window.location.reload();
  }}
>
  Save
</button>
<button on:click={onBack}>Back</button>

<style lang="css">
</style>
