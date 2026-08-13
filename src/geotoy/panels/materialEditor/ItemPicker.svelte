<script lang="ts">
  import type { Snippet } from 'svelte';

  type Item = {
    id: string | number;
    name: string;
    description?: string;
    tags?: string[];
    thumbnailUrl?: string | null;
    url?: string | null;
  };

  let {
    items = $bindable(),
    selectedId,
    onselect = () => {},
    onclose = () => {},
    title = 'Select an item',
    showNoneOption = true,
    footerStart,
    footerEnd,
    previewActions,
  }: {
    items: Item[];
    selectedId: string | number | null | undefined;
    onselect: (id: string | number | null) => void;
    onclose: () => void;
    title: string;
    showNoneOption?: boolean;
    footerStart?: Snippet;
    footerEnd?: Snippet;
    previewActions?: Snippet<[Item]>;
  } = $props();

  let filteredItems = $state<Item[]>([]);
  let selectedItemForPreview = $state<Item | null>(null);
  let searchTerm = $state('');

  $effect(() => {
    if (selectedId) {
      selectedItemForPreview = items.find(it => it.id === selectedId) || null;
    } else {
      selectedItemForPreview = null;
    }
  });

  $effect(() => {
    const term = searchTerm.trim().toLowerCase();
    if (term) {
      filteredItems = items.filter(item =>
        [item.name, item.description ?? '', ...(item.tags ?? [])].some(field =>
          field.toLowerCase().includes(term)
        )
      );
    } else {
      filteredItems = items;
    }
  });

  const handleSelect = (item: Item | null) => {
    onselect(item ? item.id : null);
    selectedItemForPreview = item;
  };
</script>

<div class="item-picker">
  <div class="header">
    <span class="title">{title}</span>
    <input type="text" placeholder="search" bind:value={searchTerm} />
  </div>
  <div class="content">
    <div class="item-list">
      {#if showNoneOption}
        <div
          class="item"
          class:selected={selectedId === null}
          onclick={() => handleSelect(null)}
          role="button"
          tabindex="0"
          onkeypress={e => {
            if (e.key === 'Enter' || e.key === ' ') {
              handleSelect(null);
            }
          }}
        >
          <div class="no-item"></div>
          <span style="font-style: italic; color: #aaa">none</span>
        </div>
      {/if}
      {#each filteredItems as item (item.id)}
        <div
          class="item"
          class:selected={selectedId === item.id}
          onclick={() => handleSelect(item)}
          role="button"
          tabindex="0"
          onkeypress={e => {
            if (e.key === 'Enter' || e.key === ' ') {
              handleSelect(item);
            }
          }}
        >
          {#if item.thumbnailUrl}
            <img src={item.thumbnailUrl} alt={item.name} crossorigin="anonymous" loading="lazy" />
          {:else}
            <div class="no-item"></div>
          {/if}
          <span>{item.name}</span>
        </div>
      {/each}
    </div>
    <div class="preview-pane">
      {#if selectedItemForPreview}
        {@const item = selectedItemForPreview}
        {#if item.url || item.thumbnailUrl}
          <img src={item.url || item.thumbnailUrl} alt={item.name} crossorigin="anonymous" />
        {:else}
          <div class="no-item" style="width: 200px; height: 200px;"></div>
        {/if}
        <div class="meta">
          {#if item.tags?.length}
            <div class="tags">
              {#each item.tags as tag (tag)}
                <button class="chip" title="filter by “{tag}”" onclick={() => (searchTerm = tag)}>
                  {tag}
                </button>
              {/each}
            </div>
          {/if}
          {#if item.description}
            <div class="description">{item.description}</div>
          {/if}
          {@render previewActions?.(item)}
        </div>
      {:else}
        <div class="placeholder">select an item to preview</div>
      {/if}
    </div>
  </div>
  <div class="buttons">
    {@render footerStart?.()}
    <div style="flex-grow: 1"></div>
    {#if footerEnd}
      {@render footerEnd()}
    {:else}
      <button class="footer-button" onclick={onclose}>close</button>
    {/if}
  </div>
</div>

<style>
  .item-picker {
    display: flex;
    flex: 1;
    flex-direction: column;
    background: #2a2a2a;
  }
  .header {
    display: flex;
    padding: 8px;
    border-bottom: 1px solid #444;
    align-items: center;
    gap: 8px;
  }
  .title {
    font-size: 14px;
    font-weight: bold;
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
    flex: 1;
  }
  .item-list {
    min-width: 200px;
    width: 200px;
    border-right: 1px solid #444;
    overflow-y: auto;
    flex: 1;
  }
  .no-item {
    height: 40px;
    width: 40px;
    background: #222
      repeating-linear-gradient(-45deg, transparent, transparent 9px, #181818 9px, #181818 18px);
  }
  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px;
    cursor: pointer;
    border-bottom: 1px solid #333;
  }
  .item:hover {
    background: #333;
  }
  .item.selected {
    background: #444;
  }
  .item img {
    width: 40px;
    height: 40px;
    object-fit: cover;
  }
  .item span {
    font-size: 12px;
  }
  .preview-pane {
    flex: 0.75;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 4px;
    min-width: 0;
    overflow-y: auto;
  }
  .preview-pane img {
    max-width: 100%;
    min-height: 0;
    object-fit: contain;
    flex: 1;
  }
  .meta {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    width: 100%;
  }
  .tags {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 4px;
  }
  .chip {
    background: #3a3a3a;
    border: 1px solid #555;
    color: #ddd;
    padding: 1px 6px;
    font-size: 11px;
    font-family: inherit;
    cursor: pointer;
  }
  .chip:hover {
    background: #4a4a4a;
    color: #fff;
  }
  .description {
    font-size: 11px;
    color: #aaa;
    text-align: center;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 96px;
    overflow-y: auto;
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
