<script lang="ts">
  import type { Readable } from 'svelte/store';

  import type { AudioSettings } from '../conf';

  export let onBack: () => void;
  export let conf: Readable<AudioSettings>;
  export let onChange: (newConf: AudioSettings) => void;
</script>

<div class="slider-input">
  <label for="global-volume-slider">Global Volume</label>
  <input
    type="range"
    id="global-volume-slider"
    name="volume"
    min="0"
    max="1"
    step="0.01"
    value={$conf.globalVolume}
    on:change={e => {
      onChange({ ...$conf, globalVolume: +e.currentTarget.value });
    }}
  />
</div>
<div class="slider-input">
  <label for="music-volume-slider">Music Volume</label>
  <input
    disabled
    type="range"
    id="music-volume-slider"
    name="volume"
    min="0"
    max="1"
    step="0.01"
    value={$conf.musicVolume}
    on:change={e => {
      onChange({ ...$conf, musicVolume: +e.currentTarget.value });
    }}
  />
</div>
<button on:click={onBack}>Back</button>
