<script lang="ts">
  import { listTextures, type TextureDescriptor, type TextureID } from 'src/geoscript/geotoyAPIClient';
  import ItemPicker from './ItemPicker.svelte';
  import { Textures } from './state.svelte';

  let {
    selectedTextureId,
    onselect,
    onclose = () => {},
    onupload = () => {},
  }: {
    selectedTextureId: TextureID | null | undefined;
    onselect: (id: TextureID | null) => void;
    onclose: () => void;
    onupload: () => void;
  } = $props();

  const origSelectedTextureId = selectedTextureId;
  let textures = $state<TextureDescriptor[]>([]);
  let isLoading = $state(true);

  $effect(() => {
    isLoading = true;
    listTextures().then(textureList => {
      const texturesMap: Record<number, TextureDescriptor> = {};
      for (const texture of textureList) {
        texturesMap[texture.id] = texture;
      }
      Textures.textures = texturesMap;

      textures = textureList;
      isLoading = false;
    });
  });
</script>

{#if isLoading}
  <div class="loading">loading...</div>
{:else}
  <ItemPicker title="Select Texture" selectedId={selectedTextureId} {onselect} items={textures} {onclose}>
    <div slot="footer-start">
      <button class="footer-button" onclick={onupload}>upload new</button>
    </div>
    <div slot="footer-end">
      <button
        class="footer-button"
        onclick={() => {
          selectedTextureId = origSelectedTextureId;
          onclose();
        }}
      >
        cancel
      </button>
      <button class="footer-button" onclick={onclose}>select</button>
    </div>
  </ItemPicker>
{/if}

<style>
  .loading {
    font-size: 14px;
    text-align: center;
    padding: 16px;
    display: flex;
    flex: 1;
    align-items: center;
    justify-content: center;
  }
  :global(.footer-button) {
    background: #333;
    border: 1px solid #555;
    color: #f0f0f0;
    padding: 10px 9px 9px 9px;
    cursor: pointer;
  }
  :global(.footer-button:hover) {
    background: #3d3d3d;
  }
</style>
