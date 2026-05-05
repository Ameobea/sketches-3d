<script lang="ts">
  import AssetTreePicker from './AssetTreePicker.svelte';
  import TransformInputs from './TransformInputs.svelte';
  import type { TransformSnapshot } from './LevelEditor.svelte';

  interface Props {
    nodeId: string | null;
    isGroup: boolean;
    isGenerated: boolean;
    materialId: string | null;
    materialIds: string[];
    isCsgAsset: boolean;
    position: [number, number, number];
    rotation: [number, number, number];
    scale: [number, number, number];
    onapplytransform: (snap: Partial<TransformSnapshot>) => void;
    onrename: (newId: string) => void;
    onmaterialchange: (matId: string | null) => void;
    onconvertToCsg: () => void;
    ondelete: () => void;
  }

  let {
    nodeId,
    isGroup,
    isGenerated,
    materialId,
    materialIds,
    isCsgAsset,
    position,
    rotation,
    scale,
    onapplytransform,
    onrename,
    onmaterialchange,
    onconvertToCsg,
    ondelete,
  }: Props = $props();

  // Editable node ID
  let idDraft = $state('');
  let idFocused = $state(false);

  $effect(() => {
    // Reset draft whenever the selected node changes.
    if (!idFocused) idDraft = nodeId ?? '';
  });

  const commitRename = () => {
    const trimmed = idDraft.trim();
    if (trimmed && trimmed !== nodeId) onrename(trimmed);
    else idDraft = nodeId ?? ''; // revert if empty or unchanged
    idFocused = false;
  };

  const onIdKeydown = (e: KeyboardEvent) => {
    if (e.key === 'Enter') { (e.target as HTMLInputElement).blur(); }
    if (e.key === 'Escape') { idDraft = nodeId ?? ''; (e.target as HTMLInputElement).blur(); }
  };

</script>

{#if nodeId !== null}
  <div class="info-panel">
    <div class="node-header">
      {#if isGenerated}
        <span class="node-id">{nodeId}</span>
      {:else}
        <input
          class="node-id-input"
          type="text"
          value={idFocused ? idDraft : (nodeId ?? '')}
          oninput={(e) => { idDraft = (e.target as HTMLInputElement).value; }}
          onfocus={(e) => { idDraft = nodeId ?? ''; idFocused = true; (e.target as HTMLInputElement).select(); }}
          onblur={commitRename}
          onkeydown={onIdKeydown}
        />
      {/if}
      {#if isGroup}<span class="badge group-badge">group</span>{/if}
      {#if isGenerated}<span class="badge gen-badge">generated</span>{/if}
    </div>

    {#if !isGenerated}
      <!-- Transform section -->
      <TransformInputs
        {position}
        {rotation}
        {scale}
        onapply={(patch) => onapplytransform(patch)}
      />

      <!-- Material assignment (leaf objects) -->
      {#if !isGroup}
        <div class="mat-row">
          <span class="tf-label">mat</span>
          <div class="mat-picker-wrap">
            <AssetTreePicker
              localItems={materialIds}
              selected={materialId}
              allowNone={true}
              onselect={(v) => onmaterialchange(v)}
            />
          </div>
        </div>
      {/if}

      <!-- CSG -->
      {#if !isGroup}
        {#if isCsgAsset}
          <span class="csg-label">CSG asset</span>
        {:else}
          <button class="action-btn" onclick={onconvertToCsg}>convert to CSG</button>
        {/if}
      {/if}

      <!-- Delete -->
      <button class="action-btn delete-btn" onclick={ondelete}>delete</button>
    {:else}
      <span class="readonly-note">Generated nodes are read-only.</span>
    {/if}
  </div>
{/if}

<style>
  .info-panel {
    border-top: 1px solid #333;
    margin-top: 6px;
    padding-top: 8px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .node-header {
    display: flex;
    align-items: center;
    gap: 5px;
    margin-bottom: 4px;
  }

  .node-id {
    font-size: 12px;
    color: #ccc;
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .node-id-input {
    flex: 1;
    min-width: 0;
    font: 12px monospace;
    color: #ccc;
    background: transparent;
    border: 1px solid transparent;
    padding: 0 2px;
    outline: none;
  }

  .node-id-input:hover {
    border-color: #555;
  }

  .node-id-input:focus {
    border-color: #7a7;
    background: #111;
    color: #ddd;
  }

  .badge {
    font-size: 10px;
    border-radius: 2px;
    padding: 0 3px;
    flex-shrink: 0;
  }

  .group-badge {
    color: #aaa;
    border: 1px solid #555;
  }

  .gen-badge {
    color: #f2c66d;
    border: 1px solid #6a5526;
  }

  .mat-row {
    display: flex;
    align-items: flex-start;
    gap: 3px;
    margin-top: 2px;
  }

  .mat-picker-wrap {
    flex: 1;
    min-width: 0;
  }

  .tf-label {
    width: 24px;
    font-size: 11px;
    color: #888;
    flex-shrink: 0;
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

  .csg-label {
    font-size: 11px;
    color: #8f8;
  }

  .readonly-note {
    font-size: 11px;
    color: #cfae62;
  }
</style>
