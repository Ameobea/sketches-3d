<script lang="ts">
  import HierarchyPanel from './HierarchyPanel.svelte';
  import InfoPanel from './InfoPanel.svelte';
  import type { TransformSnapshot } from './LevelEditor.svelte';
  import type { LevelSceneNode } from './loadLevelDef';

  interface Props {
    assetIds: string[];
    materialIds: string[];
    rootNodes: LevelSceneNode[];
    selectedNodeId: string | null;
    selectedMaterialId: string | null;
    isGroupSelected: boolean;
    isGeneratedSelected: boolean;
    materialEditorOpen: boolean;
    isCsgAsset: boolean;
    position: [number, number, number];
    rotation: [number, number, number];
    scale: [number, number, number];
    onadd: (assetId: string, materialId: string | undefined) => void;
    onaddgroup: () => void;
    onmaterialchange: (matId: string | null) => void;
    onapplytransform: (snap: Partial<TransformSnapshot>) => void;
    ondelete: () => void;
    ontoggleMaterialEditor: () => void;
    onconvertToCsg: () => void;
    onselectnode: (node: LevelSceneNode) => void;
  }

  let {
    assetIds,
    materialIds,
    rootNodes,
    selectedNodeId,
    selectedMaterialId,
    isGroupSelected,
    isGeneratedSelected,
    materialEditorOpen,
    isCsgAsset,
    position,
    rotation,
    scale,
    onadd,
    onaddgroup,
    onmaterialchange,
    onapplytransform,
    ondelete,
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

    <div class="row add-btns">
      <button class="add-btn" onclick={handleAdd}>add object</button>
      <button class="add-btn" onclick={onaddgroup}>add group</button>
    </div>
  </div>

  <!-- Divider -->
  <div class="divider"></div>

  <!-- Edit Materials toggle -->
  <button class="edit-mats-btn" onclick={ontoggleMaterialEditor}>
    {materialEditorOpen ? 'close materials' : 'edit materials'}
  </button>

  <!-- Scene hierarchy -->
  <HierarchyPanel {rootNodes} selectedNodeId={selectedNodeId} {onselectnode} />

  <!-- Selected node info panel -->
  <InfoPanel
    nodeId={selectedNodeId}
    isGroup={isGroupSelected}
    isGenerated={isGeneratedSelected}
    materialId={selectedMaterialId}
    {materialIds}
    {isCsgAsset}
    {position}
    {rotation}
    {scale}
    {onapplytransform}
    {onmaterialchange}
    {onconvertToCsg}
    {ondelete}
  />
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
    min-width: 260px;
    max-height: calc(100vh - 24px);
    overflow-y: auto;
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

  .add-btns {
    gap: 6px;
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
    flex: 1;
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
</style>
