<script lang="ts">
  import type { Texture } from 'src/geoscript/geotoyAPIClient';

  export let texture: Texture | undefined = undefined;
  export let onclick: () => void = () => {};
</script>

<button class="texture-preview" on:click={onclick}>
  {#if texture}
    <img src={texture.thumbnailUrl} alt={texture.name} crossorigin="anonymous" title={texture.name} />
  {:else}
    <div class="none">-</div>
  {/if}
</button>

<style>
  .texture-preview {
    width: 32px;
    height: 32px;
    border: 1px solid #555;
    background: #1a1a1a;
    cursor: pointer;
    padding: 0;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .texture-preview:hover {
    border-color: #777;
  }
  img {
    max-width: 100%;
    max-height: 100%;
    object-fit: cover;
  }
  .none {
    font-size: 20px;
    color: #888;
  }
</style>
