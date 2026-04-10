<script lang="ts">
  import type { AssetLibFolder } from './assetLibTypes';

  interface Props {
    /** Flat list of locally-defined items (asset IDs or material IDs). */
    localItems: string[];
    /** Asset library folder tree. If empty, no "asset lib" section is shown. */
    libFolders?: AssetLibFolder[];
    /** Currently selected value: a local item ID or an `__ASSETS__/…` path. */
    selected: string | null;
    /** Show a "(none)" option at the top — useful for material pickers. */
    allowNone?: boolean;
    onselect: (value: string | null) => void;
  }

  let {
    localItems,
    libFolders = [],
    selected,
    allowNone = false,
    onselect,
  }: Props = $props();

  let libExpanded = $state(false);
  // Tracks which folder keys are expanded; key is the folder's path string.
  let expandedFolders = $state(new Set<string>());

  const toggleFolder = (key: string) => {
    const next = new Set(expandedFolders);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    expandedFolders = next;
  };
</script>

<div class="tree-picker">
  {#if libFolders.length > 0}
    <div
      class="folder-row"
      role="button"
      tabindex="0"
      onclick={() => { libExpanded = !libExpanded; }}
      onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') libExpanded = !libExpanded; }}
    >
      <span class="arrow">{libExpanded ? '▾' : '▸'}</span><span class="folder-name">asset lib</span>
    </div>
    {#if libExpanded}
      {#each libFolders as folder (folder.name)}
        {@render folderNode(folder, folder.name, 1)}
      {/each}
    {/if}
  {/if}

  {#if allowNone}
    <div
      class="item"
      class:selected={selected === null}
      role="option"
      aria-selected={selected === null}
      tabindex="0"
      onclick={() => onselect(null)}
      onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') onselect(null); }}
    >(none)</div>
  {/if}

  {#each localItems as id (id)}
    <div
      class="item"
      class:selected={selected === id}
      role="option"
      aria-selected={selected === id}
      tabindex="0"
      onclick={() => onselect(id)}
      onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') onselect(id); }}
    >{id}</div>
  {/each}
</div>

{#snippet folderNode(folder: AssetLibFolder, key: string, depth: number)}
  <div
    class="folder-row"
    style:padding-left="{depth * 10}px"
    role="button"
    tabindex="0"
    onclick={() => toggleFolder(key)}
    onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') toggleFolder(key); }}
  >
    <span class="arrow">{expandedFolders.has(key) ? '▾' : '▸'}</span><span class="folder-name">{folder.name}</span>
  </div>
  {#if expandedFolders.has(key)}
    {#each folder.files as file (file.path)}
      <div
        class="item"
        class:selected={selected === file.path}
        role="option"
        aria-selected={selected === file.path}
        tabindex="0"
        style:padding-left="{(depth + 1) * 10}px"
        onclick={() => onselect(file.path)}
        onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') onselect(file.path); }}
      >{file.name}</div>
    {/each}
    {#each folder.subfolders as sub (sub.name)}
      {@render folderNode(sub, `${key}/${sub.name}`, depth + 1)}
    {/each}
  {/if}
{/snippet}

<style>
  .tree-picker {
    max-height: 180px;
    overflow-y: auto;
    border: 1px solid #444;
    background: #111;
    font: 11px monospace;
    color: #aaa;
  }

  .item {
    padding: 1px 6px;
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .item:hover {
    background: #1c1c1c;
  }

  .item.selected {
    background: #2a3a2a;
    color: #9fca9f;
  }

  .folder-row {
    display: flex;
    align-items: center;
    padding: 1px 4px;
    cursor: pointer;
    user-select: none;
  }

  .folder-row:hover {
    background: #1c1c1c;
  }

  .arrow {
    width: 12px;
    flex-shrink: 0;
    font-size: 10px;
  }

  .folder-name {
    color: #888;
  }
</style>
