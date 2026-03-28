<script lang="ts">
  import { untrack } from 'svelte';
  import type { CsgTreeNode, CsgLeafNode, CsgOpNode } from './types';
  import { isOpNode, cloneTree, getNodeAtPath, setNodeAtPath, deleteAtPath } from './csgTreeUtils';

  interface Props {
    tree: CsgTreeNode | null;
    assetIds: string[];
    selectedNodePath: string | null;
    nodePolarities: Map<string, 'positive' | 'negative'>;
    ontreechange: (tree: CsgTreeNode) => void;
    onnodeselect: (path: string | null) => void;
    onexitcsg: () => void;
  }

  let {
    tree,
    assetIds,
    selectedNodePath,
    nodePolarities,
    ontreechange,
    onnodeselect,
    onexitcsg,
  }: Props = $props();

  let addAssetId = $state(untrack(() => assetIds[0] ?? ''));
  let addOp = $state<'union' | 'difference' | 'intersection'>('union');

  const emitChange = (newTree: CsgTreeNode) => {
    ontreechange(newTree);
  };

  const handleOpChange = (path: string, newOp: 'union' | 'difference' | 'intersection') => {
    if (!tree) return;
    const newRoot = cloneTree(tree);
    const node = getNodeAtPath(newRoot, path);
    if (isOpNode(node)) {
      node.op = newOp;
      emitChange(newRoot);
    }
  };

  const handleMoveChild = (parentPath: string, childIndex: number, direction: -1 | 1) => {
    if (!tree) return;
    const newRoot = cloneTree(tree);
    const parent = getNodeAtPath(newRoot, parentPath);
    if (!isOpNode(parent)) return;
    const targetIndex = childIndex + direction;
    if (targetIndex < 0 || targetIndex >= parent.children.length) return;
    [parent.children[childIndex], parent.children[targetIndex]] =
      [parent.children[targetIndex], parent.children[childIndex]];
    emitChange(newRoot);
  };

  const handleDeleteNode = (path: string) => {
    if (!tree) return;
    const result = deleteAtPath(tree, path);
    if (result) {
      onnodeselect(null);
      emitChange(result);
    }
  };

  const handleAddLeaf = () => {
    if (!tree || addAssetId === '') return;
    const newLeaf: CsgLeafNode = { asset: addAssetId };

    if (selectedNodePath !== null && selectedNodePath !== '') {
      const selectedNode = getNodeAtPath(tree, selectedNodePath);
      if (isOpNode(selectedNode)) {
        // Selected node is an op — append child directly
        const newRoot = cloneTree(tree);
        const opNode = getNodeAtPath(newRoot, selectedNodePath) as CsgOpNode;
        opNode.children.push(newLeaf);
        emitChange(newRoot);
      } else {
        // Selected node is a leaf — wrap it: op(selected, newLeaf)
        const wrapper: CsgOpNode = {
          op: addOp,
          children: [cloneTree(selectedNode), newLeaf],
        };
        emitChange(setNodeAtPath(tree, selectedNodePath, wrapper));
      }
    } else {
      const newRoot: CsgOpNode = {
        op: addOp,
        children: [cloneTree(tree), newLeaf],
      };
      emitChange(newRoot);
    }
  };

  const handleNodeClick = (path: string, e: MouseEvent) => {
    e.stopPropagation();
    onnodeselect(selectedNodePath === path ? null : path);
  };

  const polarityColor = (path: string): string => {
    const p = nodePolarities.get(path);
    return p === 'negative' ? '#ff6633' : '#88cc88';
  };

  /** Get the parent path and child index from a full path. */
  const splitPath = (path: string): { parentPath: string; childIndex: number } | null => {
    if (!path) return null;
    const parts = path.split('.');
    return {
      parentPath: parts.slice(0, -1).join('.'),
      childIndex: Number(parts[parts.length - 1]),
    };
  };

  /** Get the sibling count for a node at a given path. */
  const getSiblingCount = (path: string): number => {
    if (!tree || !path) return 0;
    const info = splitPath(path);
    if (!info) return 0;
    const parent = getNodeAtPath(tree, info.parentPath);
    return isOpNode(parent) ? parent.children.length : 0;
  };
</script>

<div class="csg-panel">
  <div class="csg-header">
    <span>CSG Tree</span>
    <button class="done-btn" onclick={onexitcsg}>Done</button>
  </div>

  {#if tree}
    {#snippet renderNode(node: CsgTreeNode, path: string, depth: number)}
      {#if isOpNode(node)}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="op-node"
          class:selected={selectedNodePath === path}
          style="margin-left: {depth * 16}px"
          onclick={(e) => handleNodeClick(path, e)}
        >
          <span class="polarity-dot" style="color: {polarityColor(path)}">●</span>
          <select
            class="op-select"
            value={node.op}
            onclick={(e) => e.stopPropagation()}
            onchange={(e) => handleOpChange(path, (e.target as HTMLSelectElement).value as any)}
          >
            <option value="union">union</option>
            <option value="difference">difference</option>
            <option value="intersection">intersection</option>
          </select>
          {#if depth > 0}
            {@const info = splitPath(path)}
            {@const siblingCount = getSiblingCount(path)}
            {#if info && siblingCount > 1}
              <button class="move-btn" disabled={info.childIndex === 0} onclick={(e) => { e.stopPropagation(); handleMoveChild(info.parentPath, info.childIndex, -1); }} title="Move up">↑</button>
              <button class="move-btn" disabled={info.childIndex === siblingCount - 1} onclick={(e) => { e.stopPropagation(); handleMoveChild(info.parentPath, info.childIndex, 1); }} title="Move down">↓</button>
            {/if}
            <button class="del-btn" onclick={(e) => { e.stopPropagation(); handleDeleteNode(path); }}>×</button>
          {/if}
        </div>
        {#each node.children as child, i}
          {@render renderNode(child, path ? `${path}.${i}` : `${i}`, depth + 1)}
        {/each}
      {:else}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="leaf-node"
          class:selected={selectedNodePath === path}
          style="margin-left: {depth * 16}px"
          onclick={(e) => handleNodeClick(path, e)}
        >
          <span class="polarity-dot" style="color: {polarityColor(path)}">●</span>
          <span class="leaf-name">{node.asset}</span>
          {#if splitPath(path) && getSiblingCount(path) > 1}
            <button class="move-btn" disabled={splitPath(path)!.childIndex === 0} onclick={(e) => { e.stopPropagation(); const sp = splitPath(path)!; handleMoveChild(sp.parentPath, sp.childIndex, -1); }} title="Move up">↑</button>
            <button class="move-btn" disabled={splitPath(path)!.childIndex === getSiblingCount(path) - 1} onclick={(e) => { e.stopPropagation(); const sp = splitPath(path)!; handleMoveChild(sp.parentPath, sp.childIndex, 1); }} title="Move down">↓</button>
          {/if}
          <button class="del-btn" onclick={(e) => { e.stopPropagation(); handleDeleteNode(path); }}>×</button>
        </div>
      {/if}
    {/snippet}
    {@render renderNode(tree, '', 0)}
  {:else}
    <span class="empty-label">No tree</span>
  {/if}

  <div class="csg-divider"></div>

  <div class="add-section">
    <div class="add-row">
      <select class="op-select" bind:value={addOp}>
        <option value="union">union</option>
        <option value="difference">difference</option>
        <option value="intersection">intersection</option>
      </select>
      <select class="asset-select" bind:value={addAssetId}>
        {#each assetIds as id (id)}
          <option value={id}>{id}</option>
        {/each}
      </select>
    </div>
    <button class="add-node-btn" onclick={handleAddLeaf}>
      {#if selectedNodePath !== null && selectedNodePath !== ''}
        {@const selNode = tree ? getNodeAtPath(tree, selectedNodePath) : null}
        {selNode && isOpNode(selNode) ? 'add to selected op' : 'add at selected'}
      {:else}
        add at root
      {/if}
    </button>
  </div>
</div>

<style>
  .csg-panel {
    position: fixed;
    top: 12px;
    right: 12px;
    background: #1a1a1a;
    color: #e8e8e8;
    font: 13px monospace;
    padding: 10px 14px;
    border: 1px solid #444;
    z-index: 9998;
    min-width: 260px;
    max-height: 80vh;
    overflow-y: auto;
    pointer-events: auto;
    user-select: none;
  }

  .csg-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-weight: bold;
    margin-bottom: 8px;
    color: #8f8;
  }

  .done-btn {
    background: #1a1a1a;
    color: #e8e8e8;
    border: 1px solid #555;
    padding: 2px 8px;
    cursor: pointer;
    font: 11px monospace;
  }

  .done-btn:hover {
    background: #252525;
  }

  .op-node, .leaf-node {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 4px;
    border-radius: 2px;
    cursor: pointer;
  }

  .op-node:hover, .leaf-node:hover {
    background: #252525;
  }

  .op-node.selected, .leaf-node.selected {
    background: #1a3040;
    outline: 1px solid #00ffff;
  }

  .polarity-dot {
    font-size: 8px;
    line-height: 1;
  }

  .leaf-name {
    color: #adf;
  }

  .op-select {
    background: #1a1a1a;
    color: #e8e8e8;
    border: 1px solid #555;
    padding: 1px 4px;
    font: 11px monospace;
  }

  .asset-select {
    flex: 1;
    background: #1a1a1a;
    color: #e8e8e8;
    border: 1px solid #555;
    padding: 1px 4px;
    font: 11px monospace;
  }

  .del-btn, .move-btn {
    background: none;
    border: 1px solid #555;
    color: #e8e8e8;
    cursor: pointer;
    font: 11px monospace;
    padding: 0 4px;
    line-height: 1.4;
  }

  .del-btn:hover {
    color: #f88;
    border-color: #f88;
  }

  .move-btn:hover:not(:disabled) {
    color: #adf;
    border-color: #adf;
  }

  .move-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .csg-divider {
    border-top: 1px solid #444;
    margin: 8px 0;
  }

  .add-section {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .add-row {
    display: flex;
    gap: 6px;
  }

  .add-node-btn {
    background: #1a1a1a;
    color: #e8e8e8;
    border: 1px solid #555;
    padding: 4px 8px;
    cursor: pointer;
    font: 12px monospace;
  }

  .add-node-btn:hover {
    background: #252525;
  }

  .empty-label {
    color: #666;
  }
</style>
