<script lang="ts">
  import type { NodeDef, TexturePreviewTarget } from 'src/geoscript/geotoyAPIClient';
  import { exportedNames } from 'src/geoscript/treeCodegen';
  import type { GeotoyTab } from 'src/geotoy/modules/tabs.svelte';

  let {
    isOpen = $bindable(),
    tabs,
    activeTabId,
    current,
    onPick,
  }: {
    isOpen: boolean;
    tabs: readonly GeotoyTab[];
    activeTabId: string;
    current: TexturePreviewTarget | null;
    onPick: (target: TexturePreviewTarget) => void;
  } = $props();

  let dialog = $state<HTMLDialogElement | null>(null);
  $effect(() => {
    if (!dialog) return;
    if (isOpen && !dialog.open) dialog.showModal();
    else if (!isOpen && dialog.open) dialog.close();
  });

  interface Row {
    node: NodeDef;
    depth: number;
    exports: string[];
  }

  const meshTabs = $derived(
    tabs
      .filter(t => t.kind === 'mesh' && t.id !== activeTabId)
      .map(tab => {
        const { tree } = tab.treeState.state;
        const rows: Row[] = [];
        const walk = (id: string, depth: number) => {
          const node = tree.nodes[id];
          if (!node) return;
          rows.push({ node, depth, exports: exportedNames(node.source) });
          for (const child of node.children) walk(child, depth + 1);
        };
        walk(tree.rootId, 0);
        return { tab, rows };
      })
  );

  const isCurrent = (tabId: string, nodeId: string, exportName?: string) =>
    current?.tabId === tabId && current.nodeId === nodeId && (current.exportName ?? undefined) === exportName;

  const pick = (tabId: string, nodeId: string, exportName?: string) => {
    onPick({ tabId, nodeId, ...(exportName ? { exportName } : {}) });
    isOpen = false;
  };
</script>

<dialog bind:this={dialog} onclose={() => (isOpen = false)}>
  <div class="content">
    <h2>preview object</h2>
    <p class="hint">
      a mesh tab's node renders in the 3d preview with its own materials and lights; pick its rendered output
      or one of its exported values.
    </p>
    {#if meshTabs.length === 0}
      <span class="empty">no mesh tabs in this composition</span>
    {/if}
    {#each meshTabs as { tab, rows } (tab.id)}
      <div class="tab">
        <div class="tab-name">{tab.name}</div>
        {#each rows as { node, depth, exports } (node.id)}
          <div class="row" class:disabled={node.disabled} style={`padding-left: ${depth * 14}px`}>
            <span class="node">{node.name}</span>
            <button
              class="chip"
              class:active={isCurrent(tab.id, node.id)}
              disabled={node.disabled}
              onclick={() => pick(tab.id, node.id)}
            >
              rendered output
            </button>
            {#each exports as name (name)}
              <button
                class="chip export"
                class:active={isCurrent(tab.id, node.id, name)}
                disabled={node.disabled}
                onclick={() => pick(tab.id, node.id, name)}
              >
                {name}
              </button>
            {/each}
          </div>
        {/each}
      </div>
    {/each}
    <div class="buttons">
      <button onclick={() => (isOpen = false)}>close</button>
    </div>
  </div>
</dialog>

<style>
  dialog {
    background: #222;
    color: #f0f0f0;
    border: 1px solid #888;
    padding: 20px 24px;
    width: 80%;
    max-width: 520px;
    max-height: 80vh;
  }

  dialog::backdrop {
    background: rgba(0, 0, 0, 0.6);
  }

  .content {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  h2 {
    margin: 0;
    font-size: 16px;
  }

  .hint {
    margin: 0;
    font-size: 11px;
    color: #999;
    line-height: 1.4;
  }

  .empty {
    font-size: 12px;
    color: #888;
  }

  .tab {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .tab-name {
    font-size: 11px;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: #0ff;
    margin: 6px 0 2px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-wrap: wrap;
    font-size: 12px;
  }

  .row.disabled .node {
    color: #666;
    text-decoration: line-through;
  }

  .node {
    color: #ddd;
    margin-right: 4px;
    min-width: 80px;
  }

  .chip {
    background: #141414;
    border: 1px solid #2e2e2e;
    color: #888;
    font-size: 12px;
    font-family: inherit;
    padding: 2px 7px 3px;
    cursor: pointer;
    line-height: 1;
  }

  .chip:hover:not(:disabled) {
    background: #1e1e1e;
    color: #fff;
  }

  .chip.active {
    background: #242424;
    color: #0ff;
    border-color: #0aa;
  }

  .chip.export {
    color: #9c9;
  }

  .chip:disabled {
    cursor: default;
    opacity: 0.5;
  }

  .buttons {
    display: flex;
    justify-content: flex-end;
    margin-top: 8px;
  }
</style>
