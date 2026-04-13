<script lang="ts">
  import { SvelteMap } from 'svelte/reactivity';

  import type { LevelLight, LevelSceneNode } from './levelSceneTypes';
  import { isLevelGroup } from './levelSceneTypes';
  import { isGeneratedDef } from './levelDefTreeUtils';

  interface Props {
    rootNodes: LevelSceneNode[];
    selectedNodeIds: string[];
    lights: LevelLight[];
    selectedLightId: string | null;
    treeVersion: number;
    onselectnode: (node: LevelSceneNode, ctrlKey: boolean) => void;
    onselectlight: (light: LevelLight) => void;
    onreparent?: (parentId: string | null) => void;
  }

  let {
    rootNodes,
    selectedNodeIds,
    lights,
    selectedLightId,
    treeVersion,
    onselectnode,
    onselectlight,
    onreparent,
  }: Props = $props();

  const isNodeSelected = (id: string) => selectedNodeIds.includes(id);
  const compareIds = (a: string, b: string) => a.localeCompare(b, undefined, { numeric: true });
  const sortNodes = (nodes: LevelSceneNode[]) => [...nodes].sort((a, b) => compareIds(a.id, b.id));
  const sortLights = (items: LevelLight[]) => [...items].sort((a, b) => compareIds(a.id, b.id));

  // Track expanded state per group id
  const expanded = new SvelteMap<string, boolean>();

  const toggle = (id: string) => {
    expanded.set(id, !expanded.get(id));
  };

  /** Returns the group IDs that are ancestors of the node with the given ID, or null if not found. */
  function findAncestorGroupIds(nodes: LevelSceneNode[], targetId: string): string[] | null {
    for (const node of nodes) {
      if (node.id === targetId) return [];
      if (isLevelGroup(node)) {
        const result = findAncestorGroupIds(node.children, targetId);
        if (result !== null) return [node.id, ...result];
      }
    }
    return null;
  }

  // Auto-expand ancestor groups when a node is selected (e.g. via raycast).
  $effect(() => {
    for (const id of selectedNodeIds) {
      const ancestors = findAncestorGroupIds(rootNodes, id);
      if (ancestors) {
        for (const ancestorId of ancestors) {
          expanded.set(ancestorId, true);
        }
      }
    }
  });

  // --- Drag and Drop ---

  let draggedNodeId = $state<string | null>(null);
  let dropTargetId = $state<string | null | 'root'>(null);

  const isValidDropTarget = (target: LevelSceneNode | 'root') => {
    if (target === 'root') return true;
    if (target.generated) return false;
    if (draggedNodeId === target.id) return false;

    const ancestors = findAncestorGroupIds(rootNodes, target.id);
    return !ancestors?.includes(draggedNodeId ?? '');
  };

  const handleDragStart = (e: DragEvent, node: LevelSceneNode) => {
    if (node.generated) {
      e.preventDefault();
      return;
    }

    // Ensure the node being dragged is selected.
    // If it's not already selected, select it (and clear others).
    // If it IS already selected, we leave the multi-selection as-is to drag it all.
    if (!isNodeSelected(node.id)) {
      onselectnode(node, false);
    }

    draggedNodeId = node.id;
    e.dataTransfer?.setData('text/plain', node.id);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move';
  };

  const handleDragOver = (e: DragEvent, target: LevelSceneNode | 'root') => {
    if (!draggedNodeId || !isValidDropTarget(target)) {
      dropTargetId = null;
      if (e.dataTransfer) e.dataTransfer.dropEffect = 'none';
      return;
    }

    e.preventDefault();
    dropTargetId = target === 'root' ? 'root' : target.id;
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
  };

  const handleDragLeave = () => {
    dropTargetId = null;
  };

  const handleDrop = (e: DragEvent, target: LevelSceneNode | 'root') => {
    e.preventDefault();
    const sourceId = e.dataTransfer?.getData('text/plain');
    if (!sourceId && !draggedNodeId) return;
    if (!isValidDropTarget(target)) {
      draggedNodeId = null;
      dropTargetId = null;
      return;
    }

    // Perform reparenting
    onreparent?.(target === 'root' ? null : target.id);

    draggedNodeId = null;
    dropTargetId = null;
  };
</script>

<div class="hierarchy">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="section-label root-drop-target"
    class:drop-over={dropTargetId === 'root'}
    ondragover={(e) => handleDragOver(e, 'root')}
    ondragleave={handleDragLeave}
    ondrop={(e) => handleDrop(e, 'root')}
  >scene</div>
  {#each sortNodes(rootNodes) as node (node.id)}
    {@render renderNode(node, 0)}
  {/each}

  {#if lights.length > 0}
    <div class="section-label lights-label">lights</div>
    {#each sortLights(lights) as light (light.id)}
      <div
        class="row leaf-row"
        class:selected={light.id === selectedLightId}
        role="button"
        tabindex="0"
        onclick={() => onselectlight(light)}
        onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') onselectlight(light); }}
      >
        <span class="node-id">{light.id}</span>
        <span class="badge light-type-badge">{light.def.type}</span>
      </div>
    {/each}
  {/if}
</div>

{#snippet renderNode(node: LevelSceneNode, depth: number)}
  {#if isLevelGroup(node)}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="row group-row"
      class:selected={isNodeSelected(node.id)}
      class:drop-over={dropTargetId === node.id}
      style:padding-left="{4 + depth * 12}px"
      role="button"
      tabindex="0"
      draggable={!node.generated}
      ondragstart={(e) => handleDragStart(e, node)}
      ondragover={(e) => handleDragOver(e, node)}
      ondragleave={handleDragLeave}
      ondrop={(e) => handleDrop(e, node)}
      onclick={(e) => onselectnode(node, e.ctrlKey || e.metaKey)}
      onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') onselectnode(node, e.ctrlKey || e.metaKey); }}
    >
      <button
        class="chevron"
        onclick={(e) => { e.stopPropagation(); toggle(node.id); }}
        aria-label={expanded.get(node.id) ? 'collapse' : 'expand'}
      >{expanded.get(node.id) ? '▾' : '▸'}</button>
      <span class="node-id">{node.id}</span>
      <span class="badge group-badge">{node.children.length}</span>
      {#if isGeneratedDef(node.def)}
        <span class="badge generated-badge">generated</span>
      {/if}
    </div>
    {#if expanded.get(node.id)}
      {#each sortNodes(treeVersion >= 0 ? node.children : node.children) as child (child.id)}
        {@render renderNode(child, depth + 1)}
      {/each}
    {/if}
  {:else}
    <div
      class="row leaf-row"
      class:selected={isNodeSelected(node.id)}
      style:padding-left="{4 + depth * 12}px"
      role="button"
      tabindex="0"
      draggable={!node.generated}
      ondragstart={(e) => handleDragStart(e, node)}
      onclick={(e) => onselectnode(node, e.ctrlKey || e.metaKey)}
      onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') onselectnode(node, e.ctrlKey || e.metaKey); }}
    >
      <span class="node-id">{node.id}</span>
      {#if isGeneratedDef(node.def)}
        <span class="badge generated-badge">gen</span>
      {/if}
    </div>
  {/if}
{/snippet}

<style>
  .hierarchy {
    margin-top: 8px;
  }

  .section-label {
    color: #888;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: 4px;
    padding: 2px 4px;
    border-radius: 2px;
  }

  .root-drop-target.drop-over {
    background: #2a3a2a;
    color: #cfc;
    outline: 1px dashed #4a4;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 4px;
    padding-top: 2px;
    padding-bottom: 2px;
    padding-right: 4px;
    cursor: pointer;
    border-radius: 2px;
    border: 1px solid transparent;
  }

  .row:hover {
    background: #252525;
  }

  .row.selected {
    background: #2a3a2a;
  }

  .row.drop-over {
    background: #2a3a2a;
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

  .leaf-row {
    padding-left: 20px;
  }

  .node-id {
    flex: 1;
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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

  .generated-badge {
    color: #f2c66d;
    border: 1px solid #6a5526;
  }

  .lights-label {
    margin-top: 8px;
  }

  .light-type-badge {
    color: #adf;
    border: 1px solid #36a;
  }
</style>
