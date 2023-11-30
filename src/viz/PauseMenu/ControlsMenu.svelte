<script lang="ts">
  import type { CustomControlsEntry, SceneConfig } from '../scenes';

  export let customEntries: CustomControlsEntry[] = [];
</script>

<div>
  <div>Move</div>
  <div>WASD</div>
</div>
<div>
  <div>Jump</div>
  <div>Space</div>
</div>
<div>
  <div>Dash</div>
  <div>Shift</div>
</div>
{#each customEntries as { action, key, label }}
  <div>
    <div>{label}</div>
    <div>{key}</div>
  </div>
{/each}

<style lang="css">
  div {
    display: flex;
    flex-direction: row;
    justify-content: space-between;
    align-items: center;
  }
</style>
