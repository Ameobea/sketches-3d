<script lang="ts">
  import type { CompositionMaterialInfo } from './levelEditorPanelTypes';

  interface Props {
    info: CompositionMaterialInfo;
    materialIds: string[];
    onmap: (geotoyName: string, matId: string | null) => void;
  }

  let { info, materialIds, onmap }: Props = $props();
</script>

<div class="comp-mat-panel">
  <div class="comp-mat-header">
    composition materials
    {#if info.placementCount > 1}
      <span class="comp-mat-count" title="editing affects every placement of this composition">
        ×{info.placementCount}
      </span>
    {/if}
  </div>
  {#each info.rows as row (row.geotoyName)}
    <div class="comp-mat-row">
      <span class="comp-mat-name" title={row.geotoyName}>{row.geotoyName}</span>
      <select
        class="comp-mat-select"
        value={row.mappedTo ?? ''}
        onchange={e => onmap(row.geotoyName, (e.currentTarget as HTMLSelectElement).value || null)}
      >
        <option value="">default (from composition)</option>
        {#each materialIds as m (m)}
          <option value={m}>{m}</option>
        {/each}
      </select>
    </div>
  {/each}
</div>

<style>
  .comp-mat-panel {
    border-top: 1px solid #333;
    margin-top: 6px;
    padding-top: 8px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .comp-mat-header {
    font-size: 11px;
    color: #888;
    display: flex;
    align-items: center;
    gap: 5px;
  }

  .comp-mat-count {
    color: #cfae62;
  }

  .comp-mat-row {
    display: flex;
    align-items: center;
    gap: 5px;
  }

  .comp-mat-name {
    flex: 0 0 34%;
    min-width: 0;
    font: 11px monospace;
    color: #ccc;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .comp-mat-select {
    flex: 1;
    min-width: 0;
    font: 11px monospace;
    color: #e8e8e8;
    background: #1a1a1a;
    border: 1px solid #555;
    padding: 2px 3px;
  }
</style>
