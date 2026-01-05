<script lang="ts">
  import { applyGraphicsSettings, type Viz } from '..';
  import { GraphicsQuality, type GraphicsSettings, type VizConfig } from '../conf';
  import type { SceneConfig } from '../scenes';
  import RangeInput from './RangeInput.svelte';
  import SelectButton from './SelectButton.svelte';

  export let viz: Viz;
  export let onBack: () => void;
  export let onChange: (vizConf: VizConfig) => void;
  export let startVizConfig: VizConfig;
  export let sceneConfig: SceneConfig;

  let graphicsQuality: GraphicsQuality = startVizConfig.graphics.quality;
  let fov = startVizConfig.graphics.fov;
  let showFPSStats: boolean = startVizConfig.graphics.showFPSStats;

  const AllQualities = [GraphicsQuality.Low, GraphicsQuality.Medium, GraphicsQuality.High];
  const handleQualityChange = (newQualityIx: number) => {
    graphicsQuality = AllQualities[newQualityIx];
  };

  const handleFOVChange = (newFov: number) => {
    fov = newFov;
  };

  $: graphicsSettingsChanged =
    graphicsQuality !== startVizConfig.graphics.quality ||
    fov !== startVizConfig.graphics.fov ||
    showFPSStats !== startVizConfig.graphics.showFPSStats;

  const handleSave = () => {
    const newGraphicsSettings: GraphicsSettings = {
      quality: graphicsQuality,
      fov,
      showFPSStats,
    };
    onChange({
      ...startVizConfig,
      graphics: newGraphicsSettings,
    });
    if (sceneConfig.goBackOnLoad !== false) {
      localStorage['goBackOnLoad'] = 'true';
    }

    const needsReload = graphicsQuality !== startVizConfig.graphics.quality;
    if (needsReload) {
      window.location.reload();
    }

    applyGraphicsSettings(viz, newGraphicsSettings);
  };
</script>

<SelectButton
  onChange={handleQualityChange}
  curIx={AllQualities.indexOf(graphicsQuality)}
  options={['low', 'medium', 'high']}
/>
<RangeInput label="FOV" min={60} max={120} step={1} value={fov} onChange={handleFOVChange} />
<div class="large-checkbox">
  <input id="show-fps-stats-checkbox" type="checkbox" bind:checked={showFPSStats} />
  <label for="show-fps-stats-checkbox">Show FPS</label>
</div>
<button disabled={!graphicsSettingsChanged} on:click={handleSave}>Save</button>
<button on:click={onBack}>Back</button>
