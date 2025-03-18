<script lang="ts">
  import type { Readable } from 'svelte/store';

  import type { AudioSettings } from '../conf';
  import type { Viz } from '..';
  import { onMount } from 'svelte';

  export let viz: Viz;
  export let onBack: () => void;
  export let conf: Readable<AudioSettings>;
  export let onChange: (newConf: AudioSettings) => void;

  onMount(() => {
    viz.sfxManager.loadSfx('dash_pickup');
  });
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
    on:change={evt => void onChange({ ...$conf, globalVolume: +evt.currentTarget.value })}
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
<div class="slider-input">
  <label for="sfx-volume-slider">SFX Volume</label>
  <input
    type="range"
    id="sfx-volume-slider"
    name="volume"
    min="0"
    max="1"
    step="0.01"
    value={$conf.sfxVolume}
    on:change={e => {
      onChange({ ...$conf, sfxVolume: +e.currentTarget.value });
      viz.sfxManager.playSfx('dash_pickup');
    }}
  />
</div>
<button on:click={onBack}>Back</button>
