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
  <footer>
    <span>
      Geoscript and Geotoy by <a target="_blank" href="https://cprimozic.net">Casey Primozic</a>
    </span>
    <span><a target="_blank" href="https://github.com/ameobea/sketches-3d">100% Free + Open Source</a></span>
    <span><a href="/geotoy/credits">Credits + Acknowledgements</a></span>
  </footer>
</div>

<style lang="css">
  .root {
    min-height: 100vh;
    display: flex;
    flex-direction: column;
    gap: 8px;
    flex-wrap: wrap;
    padding: 8px;
    box-sizing: border-box;
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

  footer {
    margin-top: auto;
    display: flex;
    justify-content: space-around;
    align-items: center;
    padding: 4px 8px 0 8px;
    margin-left: -8px;
    margin-right: -8px;
    color: #ccc;
    font-size: 13px;
    border-top: 1px solid #282828;
    gap: 12px;
  }

  @media (max-width: 600px) {
    footer {
      flex-direction: column;
      gap: 4px;
      text-align: center;
    }
  }
</style>
