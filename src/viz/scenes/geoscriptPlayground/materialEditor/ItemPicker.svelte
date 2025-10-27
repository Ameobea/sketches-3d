<script lang="ts">
  type Item = {
    id: string | number;
    name: string;
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
  }: {
    items: Item[];
    selectedId: string | number | null | undefined;
    onselect: (id: string | number | null) => void;
    onclose: () => void;
    title: string;
    showNoneOption?: boolean;
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
    if (searchTerm) {
      const lowerCaseSearchTerm = searchTerm.toLowerCase();
      filteredItems = items.filter(item => item.name.toLowerCase().includes(lowerCaseSearchTerm));
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
            <img src={item.thumbnailUrl} alt={item.name} crossorigin="anonymous" />
          {:else}
            <div class="no-item"></div>
          {/if}
          <span>{item.name}</span>
        </div>
      {/each}
    </div>
    <div class="preview-pane">
      {#if selectedItemForPreview}
        {#if selectedItemForPreview.url || selectedItemForPreview.thumbnailUrl}
          <img
            src={selectedItemForPreview.url || selectedItemForPreview.thumbnailUrl}
            alt={selectedItemForPreview.name}
            crossorigin="anonymous"
          />
        {:else}
          <div class="no-item" style="width: 200px; height: 200px;"></div>
        {/if}
      {:else}
        <div class="placeholder">select an item to preview</div>
      {/if}
    </div>
  </div>
  <div class="buttons">
    <slot name="footer-start" />
    <div style="flex-grow: 1"></div>
    <slot name="footer-end">
      <button class="footer-button" onclick={onclose}>close</button>
    </slot>
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
    align-items: center;
    justify-content: center;
    padding: 4px;
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
