<script lang="ts">
  import type { TreeKind } from 'src/geoscript/geotoyAPIClient';
  import type { GeotoyTab } from 'src/geotoy/modules/tabs.svelte';
  import { dismissOn } from './dismissOn';

  let {
    tabs,
    activeId,
    barHeight,
    canDelete,
    onSelect,
    onCreate,
    onRename,
    onDelete,
  }: {
    tabs: GeotoyTab[];
    activeId: string;
    /** Menus open upward from the bar's top edge; taken from the shell's layout constant. */
    barHeight: number;
    canDelete: (id: string) => boolean;
    onSelect: (id: string) => void;
    onCreate: (kind: TreeKind) => void;
    onRename: (id: string, name: string) => void;
    onDelete: (id: string) => void;
  } = $props();

  // Fixed group order; future kinds append rather than interleave.
  const KINDS: { kind: TreeKind; label: string; newLabel: string }[] = [
    { kind: 'mesh', label: '3d', newLabel: '3d scene' },
    { kind: 'texture', label: 'textures', newLabel: 'texture' },
  ];
  const groups = $derived(
    KINDS.map(k => ({ ...k, members: tabs.filter(t => t.kind === k.kind) })).filter(g => g.members.length > 0)
  );

  /** One open menu at a time; a union rather than two flag/coord pairs, so opening either
   *  can't leave the other half set. */
  let menu = $state<{ kind: 'add' } | { kind: 'tab'; id: string } | null>(null);
  let menuX = $state(0);

  // Rename reuses the hierarchy panel's idiom: double-click, Enter commits, Escape cancels.
  let renamingId = $state<string | null>(null);
  let renameValue = $state('');

  const beginRename = (tab: GeotoyTab) => {
    menu = null;
    renamingId = tab.id;
    renameValue = tab.name;
  };
  const commitRename = () => {
    if (renamingId) onRename(renamingId, renameValue);
    renamingId = null;
  };

  const openTabMenu = (e: MouseEvent, id: string) => {
    e.preventDefault();
    menu = { kind: 'tab', id };
    menuX = e.clientX;
  };

  $effect(() => {
    if (!menu) return;
    return dismissOn('[data-tabstrip-menu]', () => (menu = null));
  });
</script>

<div class="strip">
  {#each groups as group (group.kind)}
    <div class="group">
      <span class="kind">{group.label}</span>
      {#each group.members as tab (tab.id)}
        {#if renamingId === tab.id}
          <!-- svelte-ignore a11y_autofocus -->
          <input
            class="rename"
            bind:value={renameValue}
            onkeydown={e => {
              e.stopPropagation();
              if (e.key === 'Enter') commitRename();
              else if (e.key === 'Escape') renamingId = null;
            }}
            onblur={commitRename}
            onclick={e => e.stopPropagation()}
            autofocus
          />
        {:else}
          <button
            class="tab"
            class:active={tab.id === activeId}
            onclick={() => onSelect(tab.id)}
            ondblclick={() => beginRename(tab)}
            oncontextmenu={e => openTabMenu(e, tab.id)}
            title={`${tab.name} — double-click to rename, right-click for more`}
          >
            {tab.name}
          </button>
        {/if}
      {/each}
    </div>
  {/each}

  <button
    class="add"
    class:open={menu?.kind === 'add'}
    title="new tab"
    onclick={e => {
      menuX = (e.currentTarget as HTMLElement).getBoundingClientRect().left;
      menu = menu?.kind === 'add' ? null : { kind: 'add' };
    }}
  >
    +
  </button>

  {#if menu?.kind === 'add'}
    <div class="menu add-menu" data-tabstrip-menu style={`left: ${menuX}px; bottom: ${barHeight}px;`}>
      <div class="menu-header">new tab</div>
      {#each KINDS as k (k.kind)}
        <button
          class="menu-item"
          onclick={() => {
            menu = null;
            onCreate(k.kind);
          }}
        >
          {k.newLabel}
        </button>
      {/each}
    </div>
  {/if}

  {#if menu?.kind === 'tab'}
    {@const id = menu.id}
    <div class="menu tab-menu" data-tabstrip-menu style={`left: ${menuX}px; bottom: ${barHeight}px;`}>
      <button
        class="menu-item"
        onclick={() => {
          const tab = tabs.find(t => t.id === id);
          if (tab) beginRename(tab);
        }}
      >
        rename
      </button>
      <button
        class="menu-item danger"
        disabled={!canDelete(id)}
        title={canDelete(id) ? 'delete this tab' : "a composition can't have zero tabs"}
        onclick={() => {
          // Snapshot first: `id` is an `{@const}` derived over `menu`, so clearing `menu`
          // before reading it would hand `onDelete` a null.
          const target = id;
          menu = null;
          onDelete(target);
        }}
      >
        delete
      </button>
    </div>
  {/if}
</div>

<style>
  .strip {
    display: flex;
    align-items: stretch;
    gap: 12px;
    flex: 1;
    min-width: 0;
    overflow-x: auto;
    overflow-y: hidden;
  }

  .group {
    display: flex;
    align-items: stretch;
    background: #141414;
    border-left: 1px solid #2e2e2e;
    border-right: 1px solid #2e2e2e;
    flex-shrink: 0;
  }

  .kind {
    display: flex;
    align-items: center;
    background: #101010;
    border-right: 1px solid #2e2e2e;
    padding: 0 7px;
    font-size: 9px;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: #5d5d5d;
    user-select: none;
  }

  .tab {
    background: none;
    border: none;
    border-right: 1px solid #2e2e2e;
    color: #ddd;
    font-family: inherit;
    font-size: 12px;
    padding: 0 12px 0 9px;
    cursor: pointer;
    white-space: nowrap;
  }

  .group .tab:last-child {
    border-right: none;
  }

  .tab:hover {
    background: #1e1e1e;
    color: #fff;
  }

  .tab.active {
    background: #242424;
    color: #fff;
  }

  .rename {
    background: #111;
    border: 1px solid #555;
    color: #ddd;
    font: inherit;
    font-size: 12px;
    padding: 0 6px;
    outline: none;
    width: 110px;
  }

  .add {
    align-self: center;
    background: #242424;
    border: 1px solid #4a4a4a;
    color: #ddd;
    font-family: inherit;
    padding: 2px 9px;
    cursor: pointer;
    flex-shrink: 0;
  }

  .add:hover {
    background: #333;
    border-color: #777;
  }

  .add.open {
    background: #3a3a3a;
    border-color: #888;
  }

  /* Menus open upward so they never overhang the render. */
  .menu {
    position: fixed;
    width: 146px;
    background: #222;
    border: 1px solid #555;
    display: flex;
    flex-direction: column;
    z-index: 5;
  }

  .menu-header {
    font-size: 8px;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: #666;
    padding: 4px 8px 2px;
  }

  .menu-item {
    background: none;
    border: none;
    color: #ddd;
    font-family: inherit;
    font-size: 11px;
    text-align: left;
    padding: 4px 8px;
    cursor: pointer;
  }

  .menu-item:hover:not(:disabled) {
    background: #2a2a2a;
  }

  .menu-item:disabled {
    color: #666;
    cursor: default;
  }

  .menu-item.danger:hover:not(:disabled) {
    background: #3a1c1c;
  }
</style>
