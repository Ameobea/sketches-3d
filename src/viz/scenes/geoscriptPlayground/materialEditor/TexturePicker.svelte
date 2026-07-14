<script lang="ts">
  import {
    listTextures,
    type TextureDescriptor,
    type TextureID,
    type User,
  } from 'src/geoscript/geotoyAPIClient';
  import { untrack } from 'svelte';
  import ItemPicker from './ItemPicker.svelte';
  import TextureUploader from './TextureUploader.svelte';
  import { Textures } from './state.svelte';

  let {
    selectedTextureId,
    onselect: onselectInner,
    onclose = () => {},
    onupload = () => {},
    me,
  }: {
    selectedTextureId: TextureID | null | undefined;
    onselect: (id: TextureID | null) => void;
    onclose: () => void;
    onupload: () => void;
    me: User | null | undefined;
  } = $props();

  const origSelectedTextureId = untrack(() => selectedTextureId);
  let textures = $state<TextureDescriptor[]>([]);
  let isLoading = $state(true);
  let editing = $state<TextureDescriptor | null>(null);

  const refresh = async () => {
    const textureList = await listTextures();
    const texturesMap: Record<number, TextureDescriptor> = {};
    for (const texture of textureList) {
      texturesMap[texture.id] = texture;
    }
    Textures.textures = texturesMap;

    textures = textureList;
    isLoading = false;
  };

  $effect(() => {
    isLoading = true;
    refresh();
  });

  const onselect = (id: TextureID | null | string) => {
    if (typeof id === 'string') {
      throw new Error('unreachable; id should not be a string');
    }
    onselectInner(id);
  };
</script>

{#if isLoading}
  <div class="loading">loading...</div>
{:else if editing}
  <TextureUploader
    texture={editing}
    onclose={() => (editing = null)}
    onupload={() => {
      editing = null;
      refresh();
    }}
    ondelete={() => {
      if (editing && selectedTextureId === editing.id) {
        onselectInner(null);
      }
      editing = null;
      refresh();
    }}
  />
{:else}
  <ItemPicker title="Select Texture" selectedId={selectedTextureId} {onselect} items={textures} {onclose}>
    {#snippet previewActions(item)}
      {#if me && textures.find(t => t.id === item.id)?.ownerId === me.id}
        <button
          class="footer-button edit-button"
          onclick={() => (editing = textures.find(t => t.id === item.id) ?? null)}
        >
          edit metadata
        </button>
      {/if}
    {/snippet}
    {#snippet footerStart()}
      {#if me}
        <button class="footer-button" onclick={onupload}>upload new</button>
      {/if}
    {/snippet}
    {#snippet footerEnd()}
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
    {/snippet}
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
  .edit-button {
    font-size: 11px;
    padding: 4px 8px;
  }
</style>
