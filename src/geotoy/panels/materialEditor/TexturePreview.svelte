<script lang="ts">
  import type { TextureDescriptor } from 'src/geoscript/geotoyAPIClient';

  let {
    texture = undefined,
    proceduralLabel = undefined,
    onclick = () => {},
  }: {
    texture?: TextureDescriptor;
    /** `tab:output` of a procedural ref; rendered as a labeled chip (no thumbnail exists). */
    proceduralLabel?: string;
    onclick?: () => void;
  } = $props();
</script>

<button class="texture-preview" {onclick}>
  {#if texture}
    <img src={texture.thumbnailUrl} alt={texture.name} crossorigin="anonymous" title={texture.name} />
  {:else if proceduralLabel}
    <div class="procedural" title={proceduralLabel}>fx</div>
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
  .procedural {
    font-size: 13px;
    font-style: italic;
    color: #0ff;
  }
</style>
