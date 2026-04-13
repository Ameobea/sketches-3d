<script lang="ts">
  import AssetTreePicker from './AssetTreePicker.svelte';
  import HierarchyPanel from './HierarchyPanel.svelte';
  import InfoPanel from './InfoPanel.svelte';
  import LightInfoPanel from './LightInfoPanel.svelte';
  import type { LevelEditorPanelActions, LevelEditorPanelViewState } from './levelEditorPanelTypes';
  import type { LightDef } from './types';

  interface Props {
    view: LevelEditorPanelViewState;
    actions: LevelEditorPanelActions;
  }

  let { view, actions }: Props = $props();

  const selectionCount = $derived(view.selectedNodeIds.length);

  let selectedLightType = $state<LightDef['type']>('point');

  // Selected asset: either a local asset ID or an __ASSETS__/… library path.
  let selectedAsset = $state<string | null>(null);
  // Keep selectedAsset valid when local assets change (e.g. after a page load or lib registration).
  $effect(() => {
    if (selectedAsset === null || selectedAsset.startsWith('__ASSETS__/')) return;
    if (!view.assetIds.includes(selectedAsset)) selectedAsset = view.assetIds[0] ?? null;
  });
  // Default to first local asset on first render.
  $effect(() => {
    if (selectedAsset === null && view.assetIds.length > 0) selectedAsset = view.assetIds[0];
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
      actions.addLibraryObject(selectedAsset, selectedMaterial || undefined);
    } else {
      actions.addObject(selectedAsset, selectedMaterial || undefined);
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
        localItems={view.assetIds}
        libFolders={view.libFolders}
        selected={selectedAsset}
        onselect={(v) => { selectedAsset = v; }}
      />
    {/if}

    <div class="row">
      <span class="field-label">material:</span>
      <select class="field-select" bind:value={selectedMaterial}>
        <option value="">(none)</option>
        {#each view.materialIds as id (id)}
          <option value={id}>{id}</option>
        {/each}
      </select>
    </div>

    <div class="row add-btns">
      <button class="add-btn" onclick={handleAdd}>add object</button>
      <button class="add-btn" onclick={actions.addGroup}>add group</button>
    </div>

    <div class="row add-light-row">
      <select class="field-select light-type-select" bind:value={selectedLightType}>
        <option value="ambient">ambient</option>
        <option value="directional">directional</option>
        <option value="point">point</option>
        <option value="spot">spot</option>
      </select>
      <button class="add-btn" onclick={() => actions.addLight(selectedLightType)}>add light</button>
    </div>
  </div>

  <div class="divider"></div>

  <button class="edit-mats-btn" onclick={actions.toggleMaterialEditor}>
    {view.materialEditorOpen ? 'close materials' : 'edit materials'}
  </button>

  <HierarchyPanel
    rootNodes={view.rootNodes}
    selectedNodeIds={view.selectedNodeIds}
    lights={view.lights}
    selectedLightId={view.selectedLightId}
    treeVersion={view.treeVersion}
    onselectnode={actions.selectNode}
    onselectlight={actions.selectLight}
    onreparent={actions.reparent}
  />

  {#if view.selectedLightDef}
    <LightInfoPanel
      lightDef={view.selectedLightDef}
      lightPosition={view.lightPosition}
      onapplyposition={actions.applyLightPosition}
      onpropertychange={actions.applyLightProperty}
      ondelete={actions.deleteLight}
    />
  {:else if selectionCount > 1}
    <div class="multi-select-panel">
      <div class="multi-select-header">{selectionCount} objects selected</div>
      {#if actions.groupSelected}
        <button class="action-btn" onclick={actions.groupSelected} disabled={!view.canGroupSelected}>group selected</button>
      {/if}
      <button class="action-btn delete-btn" onclick={actions.deleteSelection}>delete selected</button>
    </div>
  {:else}
    <InfoPanel
      nodeId={view.selectedNodeId}
      isGroup={view.isGroupSelected}
      isGenerated={view.isGeneratedSelected}
      materialId={view.selectedMaterialId}
      materialIds={view.materialIds}
      isCsgAsset={view.isCsgAsset}
      position={view.position}
      rotation={view.rotation}
      scale={view.scale}
      onapplytransform={actions.applyTransform}
      onrename={actions.rename}
      onmaterialchange={actions.changeMaterial}
      onconvertToCsg={actions.convertToCsg}
      ondelete={actions.deleteSelection}
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
