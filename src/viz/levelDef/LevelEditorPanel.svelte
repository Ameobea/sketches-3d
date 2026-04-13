<script lang="ts">
  import AssetTreePicker from './AssetTreePicker.svelte';
  import HierarchyPanel from './HierarchyPanel.svelte';
  import InfoPanel from './InfoPanel.svelte';
  import LightInfoPanel from './LightInfoPanel.svelte';
  import type { AssetLibFolder } from './assetLibTypes';
  import type { TransformSnapshot } from './LevelEditor.svelte';
  import type { LevelLight, LevelSceneNode } from './loadLevelDef';
  import type { LightDef } from './types';

  interface Props {
    assetIds: string[];
    materialIds: string[];
    libFolders: AssetLibFolder[];
    rootNodes: LevelSceneNode[];
    lights: LevelLight[];
    selectedNodeIds: string[];
    selectedNodeId: string | null;
    treeVersion: number;
    selectedMaterialId: string | null;
    selectedLightId: string | null;
    selectedLightDef: LightDef | null;
    lightPosition: [number, number, number];
    isGroupSelected: boolean;
    isGeneratedSelected: boolean;
    materialEditorOpen: boolean;
    isCsgAsset: boolean;
    position: [number, number, number];
    rotation: [number, number, number];
    scale: [number, number, number];
    onadd: (assetId: string, materialId: string | undefined) => void;
    onaddlibrary: (libPath: string, materialId: string | undefined) => void;
    onaddgroup: () => void;
    onaddlight: (lightType: LightDef['type']) => void;
    onrename: (newId: string) => void;
    onmaterialchange: (matId: string | null) => void;
    onapplytransform: (snap: Partial<TransformSnapshot>) => void;
    ondelete: () => void;
    ontoggleMaterialEditor: () => void;
    onconvertToCsg: () => void;
    onselectnode: (node: LevelSceneNode, ctrlKey: boolean) => void;
    onselectlight: (light: LevelLight) => void;
    onlightpositionchange: (pos: [number, number, number]) => void;
    onlightpropertychange: (update: Partial<LightDef>) => void;
    ondeletelight: () => void;
    canGroupSelected?: boolean;
    ongroupselected?: () => void;
    onreparent?: (parentId: string | null) => void;
  }

  let {
    assetIds,
    materialIds,
    libFolders,
    rootNodes,
    lights,
    selectedNodeIds,
    selectedNodeId,
    treeVersion,
    selectedMaterialId,
    selectedLightId,
    selectedLightDef,
    lightPosition,
    isGroupSelected,
    isGeneratedSelected,
    materialEditorOpen,
    isCsgAsset,
    position,
    rotation,
    scale,
    onadd,
    onaddlibrary,
    onaddgroup,
    onaddlight,
    onrename,
    onmaterialchange,
    onapplytransform,
    ondelete,
    ontoggleMaterialEditor,
    onconvertToCsg,
    onselectnode,
    onselectlight,
    onlightpositionchange,
    onlightpropertychange,
    ondeletelight,
    canGroupSelected,
    ongroupselected,
    onreparent,
  }: Props = $props();

  const selectionCount = $derived(selectedNodeIds.length);

  let selectedLightType = $state<LightDef['type']>('point');

  // Selected asset: either a local asset ID or an __ASSETS__/… library path.
  let selectedAsset = $state<string | null>(null);
  // Keep selectedAsset valid when local assets change (e.g. after a page load or lib registration).
  $effect(() => {
    if (selectedAsset === null || selectedAsset.startsWith('__ASSETS__/')) return;
    if (!assetIds.includes(selectedAsset)) selectedAsset = assetIds[0] ?? null;
  });
  // Default to first local asset on first render.
  $effect(() => {
    if (selectedAsset === null && assetIds.length > 0) selectedAsset = assetIds[0];
  });

  let selectedMaterial = $state('');
  let assetPickerExpanded = $state(false);

  const selectedAssetLabel = $derived(
    selectedAsset
      ? selectedAsset.startsWith('__ASSETS__/')
        ? selectedAsset.split('/').pop()?.replace(/\.geo$/, '') ?? selectedAsset
        : selectedAsset
      : '(none)'
  );

  const handleAdd = () => {
    if (!selectedAsset) return;
    if (selectedAsset.startsWith('__ASSETS__/')) {
      onaddlibrary(selectedAsset, selectedMaterial || undefined);
    } else {
      onadd(selectedAsset, selectedMaterial || undefined);
    }
  };
</script>

<div class="panel">
  <div class="add-section">
    <div
      class="asset-picker-header"
      role="button"
      tabindex="0"
      onclick={() => { assetPickerExpanded = !assetPickerExpanded; }}
      onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') assetPickerExpanded = !assetPickerExpanded; }}
    >
      <span class="picker-arrow">{assetPickerExpanded ? '▾' : '▸'}</span>
      <span class="picker-selected">{selectedAssetLabel}</span>
    </div>
    {#if assetPickerExpanded}
      <AssetTreePicker
        localItems={assetIds}
        {libFolders}
        selected={selectedAsset}
        onselect={(v) => { selectedAsset = v; }}
      />
    {/if}

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

    <div class="row add-light-row">
      <select class="field-select light-type-select" bind:value={selectedLightType}>
        <option value="ambient">ambient</option>
        <option value="directional">directional</option>
        <option value="point">point</option>
        <option value="spot">spot</option>
      </select>
      <button class="add-btn" onclick={() => onaddlight(selectedLightType)}>add light</button>
    </div>
  </div>

  <div class="divider"></div>

  <button class="edit-mats-btn" onclick={ontoggleMaterialEditor}>
    {materialEditorOpen ? 'close materials' : 'edit materials'}
  </button>

  <HierarchyPanel
    {rootNodes}
    {selectedNodeIds}
    {lights}
    {selectedLightId}
    {treeVersion}
    {onselectnode}
    {onselectlight}
    {onreparent}
  />

  {#if selectedLightDef}
    <LightInfoPanel
      lightDef={selectedLightDef}
      lightPosition={lightPosition}
      onapplyposition={onlightpositionchange}
      onpropertychange={onlightpropertychange}
      ondelete={ondeletelight}
    />
  {:else if selectionCount > 1}
    <div class="multi-select-panel">
      <div class="multi-select-header">{selectionCount} objects selected</div>
      {#if ongroupselected}
        <button class="action-btn" onclick={ongroupselected} disabled={!canGroupSelected}>group selected</button>
      {/if}
      <button class="action-btn delete-btn" onclick={ondelete}>delete selected</button>
    </div>
  {:else}
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
      {onrename}
      {onmaterialchange}
      {onconvertToCsg}
      {ondelete}
    />
  {/if}
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
    z-index: 10001;
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

  .asset-picker-header {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 2px 4px;
    border: 1px solid #444;
    background: #111;
    cursor: pointer;
    font: 11px monospace;
    color: #aaa;
    user-select: none;
  }

  .asset-picker-header:hover {
    background: #1c1c1c;
  }

  .picker-arrow {
    font-size: 10px;
    flex-shrink: 0;
    width: 10px;
  }

  .picker-selected {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .add-btns {
    gap: 6px;
  }

  .add-light-row {
    gap: 6px;
    margin-top: 2px;
  }

  .light-type-select {
    flex: 1;
  }

  .field-label {
    width: 70px;
    flex-shrink: 0;
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

  .multi-select-panel {
    border-top: 1px solid #333;
    margin-top: 6px;
    padding-top: 8px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .multi-select-header {
    font-size: 12px;
    color: #ccc;
    margin-bottom: 4px;
  }

  .action-btn {
    background: #1a1a1a;
    color: #e8e8e8;
    border: 1px solid #555;
    padding: 3px 6px;
    cursor: pointer;
    font: 11px monospace;
    text-align: center;
  }

  .action-btn:hover {
    background: #252525;
  }

  .delete-btn {
    border-color: #633;
    color: #f88;
  }

  .delete-btn:hover {
    background: #2a1a1a;
  }
</style>
