<script lang="ts">
  import { Textures } from './state.svelte';
  import { listTextures, type Texture, type TextureID } from 'src/geoscript/geotoyAPIClient';

  let {
    selectedTextureId = $bindable(),
    onclose = () => {},
    onupload = () => {},
  }: {
    selectedTextureId: TextureID | null | undefined;
    onclose: () => void;
    onupload: () => void;
  } = $props();

  const origSelectedTextureId = selectedTextureId;
  let filteredTextures = $state<Texture[]>([]);
  let selectedTextureForPreview = $state<Texture | null>(
    selectedTextureId ? Textures.textures[selectedTextureId] : null
  );
  let searchTerm = $state('');
  let isLoading = $state(true);

  $effect(() => {
    isLoading = true;
    listTextures().then(textures => {
      const texturesByID: Record<TextureID, Texture> = {};
      for (const texture of textures) {
        texturesByID[texture.id] = texture;
      }
      Textures.textures = texturesByID;
      filteredTextures = textures;
      isLoading = false;
    });
  });

  $effect(() => {
    if (searchTerm) {
      const lowerCaseSearchTerm = searchTerm.toLowerCase();
      filteredTextures = Object.values(Textures.textures).filter(tex =>
        tex.name.toLowerCase().includes(lowerCaseSearchTerm)
      );
    } else {
      filteredTextures = Object.values(Textures.textures);
    }
  });

  const handleSelect = (texture: Texture | null) => {
    selectedTextureId = texture?.id || null;
    selectedTextureForPreview = texture;
  };
</script>

<div>
  <div class="texture-picker">
    <div class="header">
      <input type="text" placeholder="search" bind:value={searchTerm} />
    </div>
    <div class="content">
      <div class="texture-list">
        {#if isLoading}
          <div class="loading">loading...</div>
        {:else}
          <div
            class="texture-item"
            class:selected={selectedTextureId === null}
            onclick={() => handleSelect(null)}
            role="button"
            tabindex="0"
            onkeypress={e => {
              if (e.key === 'Enter' || e.key === ' ') {
                handleSelect(null);
              }
            }}
          >
            <div class="no-texture"></div>
            <span style="font-style: italic; color: #aaa">none</span>
          </div>
          {#each filteredTextures as texture (texture.id)}
            <div
              class="texture-item"
              class:selected={selectedTextureId === texture.id}
              onclick={() => handleSelect(texture)}
              role="button"
              tabindex="0"
              onkeypress={e => {
                if (e.key === 'Enter' || e.key === ' ') {
                  handleSelect(texture);
                }
              }}
            >
              <img src={texture.thumbnailUrl} alt={texture.name} crossorigin="anonymous" />
              <span>{texture.name}</span>
            </div>
          {/each}
        {/if}
      </div>
      <div class="preview-pane">
        {#if selectedTextureForPreview}
          <img
            src={selectedTextureForPreview.url}
            alt={selectedTextureForPreview.name}
            crossorigin="anonymous"
          />
        {:else}
          <div class="placeholder">select a texture to preview</div>
        {/if}
      </div>
    </div>
    <div class="buttons">
      <button class="footer-button" onclick={onupload}>upload new</button>
      <div style="flex-grow: 1"></div>
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
  </div>
</div>

<style>
  .texture-picker {
    display: flex;
    flex: 1;
    flex-direction: column;
    height: 100%;
    background: #2a2a2a;
  }
  .header {
    display: flex;
    padding: 8px;
    border-bottom: 1px solid #444;
  }
  input[type='text'] {
    flex-grow: 1;
    background-color: #1a1a1a;
    color: #eee;
    border: 1px solid #444;
    padding: 4px 6px;
    font-size: 12px;
  }
  .content {
    display: flex;
    flex-grow: 1;
    min-height: 0;
  }
  .texture-list {
    min-width: 200px;
    width: 200px;
    border-right: 1px solid #444;
    overflow-y: auto;
    flex: 1;
  }
  .no-texture {
    height: 40px;
    width: 40px;
    background: #222
      repeating-linear-gradient(-45deg, transparent, transparent 9px, #181818 9px, #181818 18px);
  }
  .loading {
    font-size: 14px;
    text-align: center;
    padding: 16px;
  }
  .texture-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px;
    cursor: pointer;
    border-bottom: 1px solid #333;
  }
  .texture-item:hover {
    background: #333;
  }
  .texture-item.selected {
    background: #444;
  }
  .texture-item img {
    width: 40px;
    height: 40px;
    object-fit: cover;
  }
  .texture-item span {
    font-size: 12px;
  }
  .preview-pane {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 8px;
  }
  .preview-pane img {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
    flex: 1;
  }
  .placeholder {
    color: #888;
    font-size: 12px;
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    padding: 8px;
    border-top: 1px solid #444;
    gap: 8px;
  }
</style>
