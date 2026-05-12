<script lang="ts">
  import { SvelteMap } from 'svelte/reactivity';

  import type { NodeDef, TreeDef } from 'src/geoscript/geotoyAPIClient';
  import { findParentId, isAncestorOf } from './treeOps';
  import { GLOBALS_SELECTION_ID } from './treeState.svelte';

  interface Props {
    tree: TreeDef;
    selectedId: string | null;
    soloId: string | null;
    /** Node ids whose modules failed to compile/eval in the last run. Used for red-border highlight. */
    failedNodeIds?: ReadonlySet<string>;
    onselect: (id: string) => void;
    onsoloToggle: (id: string) => void;
    onDisableToggle: (id: string) => void;
    /** Create a new node. `parentId === null` adds it as a child of `_root`. */
    oncreate: (parentId: string | null) => void;
    ondelete: (id: string) => void;
    /** Returns true if rename succeeded (caller validates uniqueness etc.). */
    onrename: (id: string, newName: string) => boolean;
    onreparent: (id: string, newParentId: string | null) => void;
    /** Returns false if the given node cannot be deleted (e.g., `_root`). */
    canDelete?: (id: string) => boolean;
  }

  let {
    tree,
    selectedId,
    soloId,
    failedNodeIds,
    onselect,
    onsoloToggle,
    onDisableToggle,
    oncreate,
    ondelete,
    onrename,
    onreparent,
    canDelete,
  }: Props = $props();

  const isDeletable = (id: string | null): boolean => {
    if (!id || id === GLOBALS_SELECTION_ID || !tree.nodes[id]) return false;
    return canDelete ? canDelete(id) : id !== tree.rootId;
  };

  // Per-id expanded state, defaulting to expanded for groups (tree starts small).
  const expanded = new SvelteMap<string, boolean>();
  const isExpanded = (id: string) => expanded.get(id) ?? true;
  const toggleExpanded = (id: string) => expanded.set(id, !isExpanded(id));

  // Auto-expand ancestors when selection changes.
  $effect(() => {
    if (!selectedId || selectedId === GLOBALS_SELECTION_ID) return;
    let cur = findParentId(tree, selectedId);
    while (cur) {
      expanded.set(cur, true);
      cur = findParentId(tree, cur);
    }
  });

  // Inline rename state.
  let renamingId = $state<string | null>(null);
  let renameValue = $state('');
  let renameError = $state<string | null>(null);

  const startRename = (node: NodeDef) => {
    renamingId = node.id;
    renameValue = node.name;
    renameError = null;
  };
  const commitRename = () => {
    if (!renamingId) return;
    const newName = renameValue.trim();
    if (!newName) {
      cancelRename();
      return;
    }
    try {
      const ok = onrename(renamingId, newName);
      if (!ok) {
        renameError = 'invalid';
        return;
      }
    } catch (err) {
      renameError = String((err as Error).message ?? err);
      return;
    }
    renamingId = null;
    renameError = null;
  };
  const cancelRename = () => {
    renamingId = null;
    renameError = null;
  };

  // Drag and drop. Reparenting is "drop dragged onto target" → target becomes the
  // new parent. `_root` is always the topmost node, so there is no "drop into the
  // background to make a new root" affordance.
  let draggedNodeId = $state<string | null>(null);
  let dropTargetId = $state<string | null>(null);

  const isValidDropTarget = (targetId: string): boolean => {
    if (!draggedNodeId) return false;
    if (draggedNodeId === tree.rootId) return false;
    if (targetId === draggedNodeId) return false;
    if (isAncestorOf(tree, draggedNodeId, targetId)) return false;
    return true;
  };

  const handleDragStart = (e: DragEvent, node: NodeDef) => {
    draggedNodeId = node.id;
    e.dataTransfer?.setData('text/plain', node.id);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move';
  };
  const handleDragOver = (e: DragEvent, target: string) => {
    if (!isValidDropTarget(target)) {
      dropTargetId = null;
      if (e.dataTransfer) e.dataTransfer.dropEffect = 'none';
      return;
    }
    e.preventDefault();
    dropTargetId = target;
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
  };
  const handleDragLeave = () => {
    dropTargetId = null;
  };
  const handleDrop = (e: DragEvent, target: string) => {
    e.preventDefault();
    if (!draggedNodeId || !isValidDropTarget(target)) {
      draggedNodeId = null;
      dropTargetId = null;
      return;
    }
    onreparent(draggedNodeId, target);
    draggedNodeId = null;
    dropTargetId = null;
  };

  // Scoped to the `.hierarchy` element below (not `window`) so the shortcut only fires
  // when focus is within the panel — otherwise typing Backspace in the code editor or
  // any text input would trigger node deletion. Delete only; not Backspace.
  const handlePanelKeydown = (e: KeyboardEvent) => {
    if (renamingId) return;
    if (e.key !== 'Delete') return;
    if (!isDeletable(selectedId)) return;
    e.preventDefault();
    ondelete(selectedId!);
  };
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="hierarchy" onkeydown={handlePanelKeydown}>
  <div class="toolbar">
    <button
      class="tb-btn"
      title="add child of selected (or of _root if nothing selected)"
      onclick={() => oncreate(
        selectedId && selectedId !== GLOBALS_SELECTION_ID ? selectedId : null
      )}
    >+ child</button>
    <button
      class="tb-btn danger"
      title={isDeletable(selectedId) ? 'delete selected' : "the root node can't be deleted"}
      disabled={!isDeletable(selectedId)}
      onclick={() => isDeletable(selectedId) && ondelete(selectedId!)}
    >×</button>
  </div>

  {#if tree.nodes[tree.rootId]}
    {@render renderNode(tree.nodes[tree.rootId], 0)}
  {/if}

  <div
    class="globals-row"
    class:selected={selectedId === GLOBALS_SELECTION_ID}
    role="button"
    tabindex="0"
    onclick={() => onselect(GLOBALS_SELECTION_ID)}
    onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') onselect(GLOBALS_SELECTION_ID); }}
  >
    <span class="node-id">_globals</span>
    <span class="badge globals-badge">ambient</span>
  </div>
</div>

{#snippet renderNode(node: NodeDef, depth: number)}
  {@const hasChildren = node.children.length > 0}
  {@const isFailed = failedNodeIds?.has(node.id) === true}
  {@const isSoloed = soloId === node.id}
  {@const isDisabled = node.disabled === true}
  {@const isRootNode = node.id === tree.rootId}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="row"
    class:selected={selectedId === node.id}
    class:failed={isFailed}
    class:disabled={isDisabled}
    class:soloed={isSoloed}
    class:root={isRootNode}
    class:drop-over={dropTargetId === node.id}
    style:padding-left="{4 + depth * 12}px"
    role="button"
    tabindex="0"
    draggable={!isRootNode && renamingId !== node.id}
    ondragstart={(e) => !isRootNode && handleDragStart(e, node)}
    ondragover={(e) => handleDragOver(e, node.id)}
    ondragleave={handleDragLeave}
    ondrop={(e) => handleDrop(e, node.id)}
    onclick={() => onselect(node.id)}
    onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') onselect(node.id); }}
    ondblclick={() => { if (!isRootNode) startRename(node); }}
  >
    {#if hasChildren}
      <button
        class="chevron"
        onclick={(e) => { e.stopPropagation(); toggleExpanded(node.id); }}
        aria-label={isExpanded(node.id) ? 'collapse' : 'expand'}
      >{isExpanded(node.id) ? '▾' : '▸'}</button>
    {:else}
      <span class="chevron-spacer"></span>
    {/if}

    {#if renamingId === node.id}
      <!-- svelte-ignore a11y_autofocus -->
      <input
        class="rename-input"
        class:invalid={renameError !== null}
        type="text"
        bind:value={renameValue}
        onkeydown={(e) => {
          e.stopPropagation();
          if (e.key === 'Enter') commitRename();
          else if (e.key === 'Escape') cancelRename();
        }}
        onblur={commitRename}
        onclick={(e) => e.stopPropagation()}
        autofocus
      />
    {:else}
      <span class="node-id" title={node.id}>{node.name}</span>
    {/if}

    {#if isRootNode}
      <span class="badge root-badge">root</span>
    {:else if hasChildren}
      <span class="badge group-badge">{node.children.length}</span>
    {/if}

    {#if !isRootNode}
      <button
        class="row-btn"
        class:active={isSoloed}
        title={isSoloed ? 'unsolo' : 'solo'}
        onclick={(e) => { e.stopPropagation(); onsoloToggle(node.id); }}
      >S</button>
      <button
        class="row-btn"
        class:active={isDisabled}
        title={isDisabled ? 'enable' : 'disable'}
        onclick={(e) => { e.stopPropagation(); onDisableToggle(node.id); }}
      >D</button>
    {/if}
  </div>
  {#if hasChildren && isExpanded(node.id)}
    {#each node.children as cid (cid)}
      {#if tree.nodes[cid]}
        {@render renderNode(tree.nodes[cid], depth + 1)}
      {/if}
    {/each}
  {/if}
{/snippet}

<style>
  .hierarchy {
    display: flex;
    flex-direction: column;
    min-height: 0;
    height: 100%;
  }

  .toolbar {
    display: flex;
    gap: 4px;
    padding: 4px;
    border-bottom: 1px solid #333;
    flex-shrink: 0;
  }

  .tb-btn {
    background: #1c1c1c;
    color: #ddd;
    border: 1px solid #444;
    border-radius: 2px;
    padding: 2px 8px;
    font-size: 11px;
    cursor: pointer;
    font-family: inherit;
  }

  .tb-btn:hover:not(:disabled) {
    background: #2a2a2a;
    border-color: #666;
  }

  .tb-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .tb-btn.danger:hover:not(:disabled) {
    background: #3a1c1c;
    border-color: #844;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 2px 4px;
    cursor: pointer;
    border: 1px solid transparent;
    user-select: none;
  }

  .row:hover {
    background: #252525;
  }

  .row.selected {
    background: #2a3a2a;
  }

  .row.failed {
    border-color: #a33;
    background: #2a1a1a;
  }

  .row.failed.selected {
    background: #3a1a1a;
  }

  .row.disabled .node-id {
    color: #777;
    text-decoration: line-through;
  }

  .row.soloed {
    box-shadow: inset 2px 0 0 #db5;
  }

  .row.drop-over {
    outline: 1px dashed #4a4;
  }

  .chevron {
    background: none;
    border: none;
    color: #aaa;
    font-size: 10px;
    cursor: pointer;
    padding: 0;
    width: 12px;
    flex-shrink: 0;
    line-height: 1;
  }

  .chevron-spacer {
    display: inline-block;
    width: 12px;
    flex-shrink: 0;
  }

  .node-id {
    flex: 1;
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .rename-input {
    flex: 1;
    background: #111;
    color: #ddd;
    border: 1px solid #555;
    border-radius: 2px;
    font: inherit;
    font-size: 12px;
    padding: 0 4px;
    outline: none;
  }

  .rename-input.invalid {
    border-color: #a44;
  }

  .badge {
    font-size: 10px;
    border-radius: 2px;
    padding: 0 3px;
    flex-shrink: 0;
  }

  .group-badge {
    color: #888;
    border: 1px solid #444;
  }

  .globals-badge {
    color: #adf;
    border: 1px solid #36a;
  }

  .root-badge {
    color: #db5;
    border: 1px solid #864;
  }

  .row.root .node-id {
    font-weight: 600;
  }

  .row-btn {
    background: transparent;
    color: #888;
    border: 1px solid #333;
    border-radius: 2px;
    padding: 0 4px;
    font-size: 10px;
    cursor: pointer;
    font-family: inherit;
    line-height: 14px;
    flex-shrink: 0;
  }

  .row-btn:hover {
    background: #1f1f1f;
    color: #ddd;
    border-color: #555;
  }

  .row-btn.active {
    background: #2a2a1a;
    color: #db5;
    border-color: #864;
  }

  .globals-row {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 6px;
    margin-top: 8px;
    border-top: 1px solid #333;
    cursor: pointer;
    user-select: none;
  }

  .globals-row:hover {
    background: #252525;
  }

  .globals-row.selected {
    background: #2a3a2a;
  }
</style>
