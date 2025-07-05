<script lang="ts">
  import type { Composition } from 'src/geoscript/geoscriptAPIClient';

  export let featuredCompositions: Composition[];
</script>

<div class="root">
  <h1>geotoy</h1>
  {#each featuredCompositions as composition}
    <div class="composition-tile">
      <div class="composition-title">{composition.title}</div>
      <div>{composition.description}</div>
      <div>Created by: {composition.author_username}</div>
    </div>
  {/each}
</div>

<style lang="css">
  .root {
    display: flex;
    flex-direction: column;
    gap: 8px;
    flex-wrap: wrap;
  }

  .composition-tile {
    border: 1px solid #ccc;
    padding: 16px;
    display: flex;
    flex-direction: column;
  }

  .composition-title {
    font-weight: bold;
    font-size: 24px;
  }
</style>
