<script lang="ts">
  import { untrack } from 'svelte';
  import type { CsgTreeNode, CsgLeafNode, CsgOpNode } from './types';
  import {
    isOpNode,
    cloneTree,
    getNodeAtPath,
    setNodeAtPath,
    deleteAtPath,
    insertAfterPath,
    splitPath,
  } from './csgTreeUtils';
  import TransformInputs, { type TransformPatch } from './TransformInputs.svelte';
  import { round } from './mathUtils';

  // Marker key set on the drop-target node before mutation so we can re-find
  // it post-detach (paths may shift when the source's parent collapses).
  // Survives JSON-based cloneTree since it's a plain enumerable string key.
  const DROP_TARGET_MARK = '__csgDropTargetMark';

  interface Props {
    tree: CsgTreeNode | null;
    assetIds: string[];
    selectedNodePath: string | null;
    nodePolarities: Map<string, 'positive' | 'negative'>;
    ontreechange: (tree: CsgTreeNode) => void;
    onnodeselect: (path: string | null) => void;
    onexitcsg: () => void;
  }

  let { tree, assetIds, selectedNodePath, nodePolarities, ontreechange, onnodeselect, onexitcsg }: Props =
    $props();

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
    [parent.children[childIndex], parent.children[targetIndex]] = [
      parent.children[targetIndex],
      parent.children[childIndex],
    ];
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

  /**
   * Wrap the selected node in a new op of the chosen kind, with `newLeaf` as
   * the second child. Works for any path (root or descendant), op or leaf.
   */
  const handleWrap = () => {
    if (!tree || addAssetId === '' || selectedNodePath === null) return;
    const selected = getNodeAtPath(tree, selectedNodePath);
    const newLeaf: CsgLeafNode = { asset: addAssetId };
    const wrapper: CsgOpNode = {
      op: addOp,
      children: [cloneTree(selected), newLeaf],
    };
    emitChange(setNodeAtPath(tree, selectedNodePath, wrapper));
  };

  const handleAddLeaf = (mode: 'into' | 'after') => {
    if (!tree || addAssetId === '') return;
    const newLeaf: CsgLeafNode = { asset: addAssetId };

    if (mode === 'after' && selectedNodePath !== null && selectedNodePath !== '') {
      emitChange(insertAfterPath(tree, selectedNodePath, newLeaf));
      return;
    }

    // mode === 'into' (or fallback for 'after' on root)
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
      // Nothing selected (or root selected but we want into)
      // If root is an op, add to it. Otherwise wrap it.
      if (isOpNode(tree)) {
        const newRoot = cloneTree(tree) as CsgOpNode;
        newRoot.children.push(newLeaf);
        emitChange(newRoot);
      } else {
        const newRoot: CsgOpNode = {
          op: addOp,
          children: [cloneTree(tree), newLeaf],
        };
        emitChange(newRoot);
      }
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

  // --- Drag and Drop ---

  let draggedPath = $state<string | null>(null);
  let dropTargetPath = $state<string | null>(null);

  const isValidDropTarget = (sourcePath: string, targetPath: string): boolean => {
    if (sourcePath === '') return false; // root can't be dragged
    if (sourcePath === targetPath) return false;
    if (targetPath.startsWith(sourcePath + '.')) return false; // descendant of source
    if (!tree) return false;
    return isOpNode(getNodeAtPath(tree, targetPath));
  };

  const handleDragStart = (e: DragEvent, path: string) => {
    if (path === '') {
      e.preventDefault();
      return;
    }
    draggedPath = path;
    e.dataTransfer?.setData('text/plain', path);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move';
  };

  const handleDragEnd = () => {
    draggedPath = null;
    dropTargetPath = null;
  };

  const handleDragOver = (e: DragEvent, path: string) => {
    if (!draggedPath || !isValidDropTarget(draggedPath, path)) {
      if (e.dataTransfer) e.dataTransfer.dropEffect = 'none';
      return;
    }
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
    dropTargetPath = path;
  };

  const handleDragLeave = () => {
    dropTargetPath = null;
  };

  const handleDrop = (e: DragEvent, targetPath: string) => {
    e.preventDefault();
    const sourcePath = draggedPath;
    draggedPath = null;
    dropTargetPath = null;
    if (!tree || sourcePath === null) return;
    if (!isValidDropTarget(sourcePath, targetPath)) return;

    const sourceSubtree = cloneTree(getNodeAtPath(tree, sourcePath));

    // Mark the target on a clone, then run detach. The marker survives clones
    // and the collapse-to-sibling path inside deleteAtPath, but is gone if the
    // marked node was itself the source's parent and got collapsed away — in
    // which case the move is a no-op (would leave the tree unchanged).
    const marked = cloneTree(tree);
    (getNodeAtPath(marked, targetPath) as any)[DROP_TARGET_MARK] = true;

    const afterDetach = deleteAtPath(marked, sourcePath);
    if (!afterDetach) return;

    const findMarked = (n: CsgTreeNode): CsgTreeNode | null => {
      if (DROP_TARGET_MARK in (n as any)) return n;
      if (isOpNode(n)) {
        for (const c of n.children) {
          const r = findMarked(c);
          if (r) return r;
        }
      }
      return null;
    };

    const found = findMarked(afterDetach);
    if (!found) return;
    delete (found as any)[DROP_TARGET_MARK];
    if (!isOpNode(found)) return;

    found.children.push(sourceSubtree);
    onnodeselect(null); // selection paths shift after restructure
    emitChange(afterDetach);
  };

  type Vec3 = [number, number, number];

  const arrEq = (a: Vec3, b: Vec3) => a[0] === b[0] && a[1] === b[1] && a[2] === b[2];

  const roundVec = (v: Vec3): Vec3 => [round(v[0]), round(v[1]), round(v[2])];

  const getSelectedNodeTransform = (): { position: Vec3; rotation: Vec3; scale: Vec3 } | null => {
    if (!tree || selectedNodePath === null || selectedNodePath === '') return null;
    const node = getNodeAtPath(tree, selectedNodePath);
    return {
      position: (node.position ?? [0, 0, 0]) as Vec3,
      rotation: (node.rotation ?? [0, 0, 0]) as Vec3,
      scale: (node.scale ?? [1, 1, 1]) as Vec3,
    };
  };

  const handleTransformApply = (patch: TransformPatch) => {
    if (!tree || selectedNodePath === null || selectedNodePath === '') return;
    const current = getSelectedNodeTransform();
    if (!current) return;

    const nextPos = patch.position ? roundVec(patch.position as Vec3) : current.position;
    const nextRot = patch.rotation ? roundVec(patch.rotation as Vec3) : current.rotation;
    const nextScale = patch.scale ? roundVec(patch.scale as Vec3) : current.scale;

    if (
      arrEq(nextPos, current.position) &&
      arrEq(nextRot, current.rotation) &&
      arrEq(nextScale, current.scale)
    ) {
      return;
    }

    const newRoot = cloneTree(tree);
    const node = getNodeAtPath(newRoot, selectedNodePath);
    node.position = nextPos;
    node.rotation = nextRot;
    node.scale = nextScale;
    emitChange(newRoot);
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
          class:drop-over={dropTargetPath === path}
          class:dragging={draggedPath === path}
          style="margin-left: {depth * 16}px"
          draggable={path !== ''}
          ondragstart={e => handleDragStart(e, path)}
          ondragend={handleDragEnd}
          ondragover={e => handleDragOver(e, path)}
          ondragleave={handleDragLeave}
          ondrop={e => handleDrop(e, path)}
          onclick={e => handleNodeClick(path, e)}
        >
          <span class="polarity-dot" style="color: {polarityColor(path)}">●</span>
          <select
            class="op-select"
            value={node.op}
            onclick={e => e.stopPropagation()}
            onchange={e => handleOpChange(path, (e.target as HTMLSelectElement).value as any)}
          >
            <option value="union">union</option>
            <option value="difference">difference</option>
            <option value="intersection">intersection</option>
          </select>
          {#if depth > 0}
            {@const info = splitPath(path)}
            {@const siblingCount = getSiblingCount(path)}
            {#if info && siblingCount > 1}
              <button
                class="move-btn"
                disabled={info.childIndex === 0}
                onclick={e => {
                  e.stopPropagation();
                  handleMoveChild(info.parentPath, info.childIndex, -1);
                }}
                title="Move up"
              >
                ↑
              </button>
              <button
                class="move-btn"
                disabled={info.childIndex === siblingCount - 1}
                onclick={e => {
                  e.stopPropagation();
                  handleMoveChild(info.parentPath, info.childIndex, 1);
                }}
                title="Move down"
              >
                ↓
              </button>
            {/if}
            <button
              class="del-btn"
              onclick={e => {
                e.stopPropagation();
                handleDeleteNode(path);
              }}
            >
              ×
            </button>
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
          class:dragging={draggedPath === path}
          style="margin-left: {depth * 16}px"
          draggable={true}
          ondragstart={e => handleDragStart(e, path)}
          ondragend={handleDragEnd}
          onclick={e => handleNodeClick(path, e)}
        >
          <span class="polarity-dot" style="color: {polarityColor(path)}">●</span>
          <span class="leaf-name">{node.asset}</span>
          {#if splitPath(path) && getSiblingCount(path) > 1}
            <button
              class="move-btn"
              disabled={splitPath(path)!.childIndex === 0}
              onclick={e => {
                e.stopPropagation();
                const sp = splitPath(path)!;
                handleMoveChild(sp.parentPath, sp.childIndex, -1);
              }}
              title="Move up"
            >
              ↑
            </button>
            <button
              class="move-btn"
              disabled={splitPath(path)!.childIndex === getSiblingCount(path) - 1}
              onclick={e => {
                e.stopPropagation();
                const sp = splitPath(path)!;
                handleMoveChild(sp.parentPath, sp.childIndex, 1);
              }}
              title="Move down"
            >
              ↓
            </button>
          {/if}
          <button
            class="del-btn"
            onclick={e => {
              e.stopPropagation();
              handleDeleteNode(path);
            }}
          >
            ×
          </button>
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
    <div class="add-buttons">
      <button class="add-node-btn" onclick={() => handleAddLeaf('into')}>
        {#if selectedNodePath !== null && selectedNodePath !== ''}
          {@const selNode = tree ? getNodeAtPath(tree, selectedNodePath) : null}
          {selNode && isOpNode(selNode) ? 'add into selected' : 'wrap selected'}
        {:else}
          add to root
        {/if}
      </button>
      {#if tree && selectedNodePath !== null}
        {@const selNode = getNodeAtPath(tree, selectedNodePath)}
        {#if isOpNode(selNode)}
          <button class="add-node-btn" onclick={handleWrap}>
            {selectedNodePath === '' ? 'wrap root' : 'wrap selected'}
          </button>
        {/if}
      {/if}
      {#if selectedNodePath !== null && selectedNodePath !== ''}
        <button class="add-node-btn" onclick={() => handleAddLeaf('after')}>add after selected</button>
      {/if}
    </div>
  </div>

  {#if tree && selectedNodePath !== null && selectedNodePath !== ''}
    {@const tf = getSelectedNodeTransform()}
    {#if tf}
      <div class="csg-divider"></div>
      <div class="transform-section">
        <TransformInputs
          position={tf.position}
          rotation={tf.rotation}
          scale={tf.scale}
          onapply={handleTransformApply}
        />
      </div>
    {/if}
  {/if}
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

  .op-node,
  .leaf-node {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 4px;
    cursor: pointer;
  }

  .op-node:hover,
  .leaf-node:hover {
    background: #252525;
  }

  .op-node.selected,
  .leaf-node.selected {
    background: #1a3040;
    outline: 1px solid #00ffff;
  }

  .op-node.drop-over {
    background: #2a3a2a;
    outline: 1px dashed #4a4;
  }

  .op-node.dragging,
  .leaf-node.dragging {
    opacity: 0.5;
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

  .del-btn,
  .move-btn {
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

  .add-buttons {
    display: flex;
    gap: 4px;
  }

  .add-buttons > .add-node-btn {
    flex: 1;
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

  .transform-section {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .empty-label {
    color: #666;
  }
</style>
