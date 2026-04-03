<script lang="ts">
  import { SvelteMap } from 'svelte/reactivity';

  import type { LevelSceneNode } from './levelSceneTypes';
  import { isLevelGroup } from './levelSceneTypes';
  import { isGeneratedDef } from './levelDefTreeUtils';

  interface Props {
    rootNodes: LevelSceneNode[];
    selectedNodeId: string | null;
    onselectnode: (node: LevelSceneNode) => void;
  }

  let { rootNodes, selectedNodeId, onselectnode }: Props = $props();

  // Track expanded state per group id
  const expanded = new SvelteMap<string, boolean>();

  const toggle = (id: string) => {
    expanded.set(id, !expanded.get(id));
  };
</script>

<div class="hierarchy">
  <div class="section-label">scene</div>
  {#each rootNodes as node (node.id)}
    {@render renderNode(node, 0)}
  {/each}
</div>

{#snippet renderNode(node: LevelSceneNode, depth: number)}
  {#if isLevelGroup(node)}
    <div
      class="row group-row"
      class:selected={node.id === selectedNodeId}
      style:padding-left="{4 + depth * 12}px"
      role="button"
      tabindex="0"
      onclick={() => onselectnode(node)}
      onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') onselectnode(node); }}
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
      {#each node.children as child (child.id)}
        {@render renderNode(child, depth + 1)}
      {/each}
    {/if}
  {:else}
    <div
      class="row leaf-row"
      class:selected={node.id === selectedNodeId}
      style:padding-left="{4 + depth * 12}px"
      role="button"
      tabindex="0"
      onclick={() => onselectnode(node)}
      onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') onselectnode(node); }}
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
  }

  .row:hover {
    background: #252525;
  }

  .row.selected {
    background: #2a3a2a;
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
</style>
