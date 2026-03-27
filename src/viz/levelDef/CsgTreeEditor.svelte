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

  const handleSwap = (path: string) => {
    if (!tree) return;
    const newRoot = cloneTree(tree);
    const node = getNodeAtPath(newRoot, path);
    if (isOpNode(node)) {
      [node.a, node.b] = [node.b, node.a];
      emitChange(newRoot);
    }
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

    if (selectedNodePath !== null) {
      // Wrap the selected node: op(selected, newLeaf)
      const selectedNode = getNodeAtPath(tree, selectedNodePath);
      const wrapper: CsgOpNode = {
        op: addOp,
        a: cloneTree(selectedNode),
        b: newLeaf,
      };
      emitChange(setNodeAtPath(tree, selectedNodePath, wrapper));
    } else {
      // Wrap root
      const newRoot: CsgOpNode = {
        op: addOp,
        a: cloneTree(tree),
        b: newLeaf,
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
          <button class="swap-btn" onclick={(e) => { e.stopPropagation(); handleSwap(path); }} title="Swap a↔b">⇄</button>
          {#if depth > 0}
            <button class="del-btn" onclick={(e) => { e.stopPropagation(); handleDeleteNode(path); }}>×</button>
          {/if}
        </div>
        {@render renderNode(node.a, path ? `${path}.a` : 'a', depth + 1)}
        {@render renderNode(node.b, path ? `${path}.b` : 'b', depth + 1)}
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
      {selectedNodePath !== null ? 'add at selected' : 'add at root'}
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

  .del-btn, .swap-btn {
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

  .swap-btn:hover {
    color: #adf;
    border-color: #adf;
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
