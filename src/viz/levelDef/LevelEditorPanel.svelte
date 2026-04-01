<script lang="ts">
  import HierarchyPanel from './HierarchyPanel.svelte';
  import type { LevelSceneNode } from './loadLevelDef';

  interface Props {
    assetIds: string[];
    materialIds: string[];
    rootNodes: LevelSceneNode[];
    selectedObjectId: string | null;
    selectedMaterialId: string | null;
    isGroupSelected: boolean;
    isGeneratedSelected: boolean;
    materialEditorOpen: boolean;
    isCsgAsset: boolean;
    onadd: (assetId: string, materialId: string | undefined) => void;
    onmaterialchange: (matId: string | null) => void;
    ontoggleMaterialEditor: () => void;
    onconvertToCsg: () => void;
    onselectnode: (node: LevelSceneNode) => void;
  }

  let {
    assetIds,
    materialIds,
    rootNodes,
    selectedObjectId,
    selectedMaterialId,
    isGroupSelected,
    isGeneratedSelected,
    materialEditorOpen,
    isCsgAsset,
    onadd,
    onmaterialchange,
    ontoggleMaterialEditor,
    onconvertToCsg,
    onselectnode,
  }: Props = $props();

  let selectedAssetOverride = $state<string | null>(null);
  let selectedAsset = $derived(
    selectedAssetOverride && assetIds.includes(selectedAssetOverride)
      ? selectedAssetOverride
      : (assetIds[0] ?? '')
  );
  let selectedMaterial = $state('');

  const handleAdd = () => {
    if (!selectedAsset) return;
    onadd(selectedAsset, selectedMaterial || undefined);
  };
</script>

<div class="panel">
  <!-- Add section -->
  <div class="add-section">
    <div class="row">
      <span class="field-label">asset:</span>
      <select
        class="field-select"
        value={selectedAsset}
        onchange={(e) => { selectedAssetOverride = (e.target as HTMLSelectElement).value; }}
      >
        {#each assetIds as id (id)}
          <option value={id}>{id}</option>
        {/each}
      </select>
    </div>

    <div class="row">
      <span class="field-label">material:</span>
      <select class="field-select" bind:value={selectedMaterial}>
        <option value="">(none)</option>
        {#each materialIds as id (id)}
          <option value={id}>{id}</option>
        {/each}
      </select>
    </div>

    <button class="add-btn" onclick={handleAdd}>add object</button>
  </div>

  <!-- Divider -->
  <div class="divider"></div>

  <!-- Edit Materials toggle -->
  <button class="edit-mats-btn" onclick={ontoggleMaterialEditor}>
    {materialEditorOpen ? 'close materials' : 'edit materials'}
  </button>

  <!-- Selected object info -->
  <span class="selected-label">
    Selected: {selectedObjectId ?? '(none)'}{isGroupSelected ? ' (group)' : ''}{isGeneratedSelected ? ' (generated)' : ''}
  </span>

  {#if selectedObjectId !== null && !isGroupSelected && !isGeneratedSelected}
    <div class="row selected-mat-row">
      <span class="field-label">material:</span>
      <select
        class="field-select"
        value={selectedMaterialId ?? ''}
        onchange={(e) => {
          const v = (e.target as HTMLSelectElement).value;
          onmaterialchange(v || null);
        }}
      >
        <option value="">(none)</option>
        {#each materialIds as id (id)}
          <option value={id}>{id}</option>
        {/each}
      </select>
    </div>

    {#if !isCsgAsset}
      <button class="csg-btn" onclick={onconvertToCsg}>convert to CSG</button>
    {:else}
      <span class="csg-label">CSG asset</span>
    {/if}
  {:else if selectedObjectId !== null && isGeneratedSelected}
    <span class="generated-note">Generated nodes are read-only.</span>
  {/if}

  <HierarchyPanel {rootNodes} selectedNodeId={selectedObjectId} {onselectnode} />
</div>

<style>
  .panel {
    position: fixed;
    top: 12px;
    left: 12px;
    background: #1a1a1a;
    color: #e8e8e8;
    font: 13px monospace;
    padding: 10px 14px;
    border: 1px solid #444;
    z-index: 9998;
    min-width: 240px;
    pointer-events: auto;
    user-select: none;
  }

  .add-section {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .field-label {
    width: 70px;
  }

  .field-select {
    flex: 1;
    background: #1a1a1a;
    color: #e8e8e8;
    border: 1px solid #555;
    padding: 2px 4px;
    font: 12px monospace;
  }

  .add-btn {
    margin-top: 2px;
    background: #1a1a1a;
    color: #e8e8e8;
    border: 1px solid #555;
    padding: 4px 8px;
    cursor: pointer;
    font: 12px monospace;
  }

  .add-btn:hover {
    background: #252525;
  }

  .divider {
    border-top: 1px solid #444;
    margin: 10px 0;
  }

  .edit-mats-btn {
    display: block;
    width: 100%;
    text-align: center;
    margin-bottom: 6px;
    background: #1a1a1a;
    color: #e8e8e8;
    border: 1px solid #555;
    padding: 4px 8px;
    cursor: pointer;
    font: 12px monospace;
  }

  .edit-mats-btn:hover {
    background: #252525;
  }

  .selected-label {
    color: #aaa;
    font-size: 12px;
  }

  .selected-mat-row {
    margin-top: 4px;
  }

  .generated-note {
    display: block;
    margin-top: 4px;
    color: #cfae62;
    font-size: 12px;
  }

  .csg-btn {
    display: block;
    width: 100%;
    text-align: center;
    margin-top: 6px;
    background: #1a1a1a;
    color: #e8e8e8;
    border: 1px solid #555;
    padding: 4px 8px;
    cursor: pointer;
    font: 12px monospace;
  }

  .csg-btn:hover {
    background: #252525;
  }

  .csg-label {
    display: block;
    margin-top: 6px;
    color: #8f8;
    font-size: 12px;
  }
</style>
