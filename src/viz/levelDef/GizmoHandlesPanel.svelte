<script lang="ts">
  import type { GizmoHandleRowInfo } from './levelEditorPanelTypes';

  interface Props {
    rows: GizmoHandleRowInfo[];
    onarm: (key: string | null) => void;
    onreset: (key: string) => void;
  }

  let { rows, onarm, onreset }: Props = $props();
</script>

<div class="gizmo-handles-panel">
  <div class="header">gizmos</div>
  {#each rows as row (row.key)}
    <div
      class="row"
      class:armed={row.armed}
      role="button"
      tabindex="0"
      onclick={() => onarm(row.armed ? null : row.key)}
      onkeydown={e => {
        if (e.key === 'Enter' || e.key === ' ') onarm(row.armed ? null : row.key);
      }}
    >
      <span class="chip" style="background: {row.color}"></span>
      <span class="name" title={row.key}>
        {row.handleId}{#if row.module}<span class="module">{row.module}</span>{/if}
      </span>
      <span class="readout">{row.readout}</span>
      {#if row.overridden}
        <button
          class="reset-btn"
          title="clear override"
          onclick={e => {
            e.stopPropagation();
            onreset(row.key);
          }}
        >
          ⟲
        </button>
      {:else}
        <span class="reset-spacer"></span>
      {/if}
    </div>
  {/each}
</div>

<style>
  .gizmo-handles-panel {
    border-top: 1px solid #333;
    margin-top: 6px;
    padding-top: 6px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .header {
    font-size: 11px;
    color: #888;
    margin-bottom: 2px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 4px;
    border: 1px solid transparent;
    cursor: pointer;
    font: 11px monospace;
    color: #ccc;
    user-select: none;
  }

  .row:hover {
    background: #1c1c1c;
  }

  .row.armed {
    border-color: #7a7;
    background: #162016;
  }

  .chip {
    width: 9px;
    height: 9px;
    flex-shrink: 0;
    border-radius: 2px;
  }

  .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .module {
    color: #777;
    margin-left: 5px;
    font-size: 10px;
  }

  .readout {
    color: #8a9;
    font-size: 10px;
    flex-shrink: 0;
  }

  .reset-btn {
    background: transparent;
    color: #c96;
    border: none;
    cursor: pointer;
    font-size: 12px;
    padding: 0 2px;
    flex-shrink: 0;
  }

  .reset-btn:hover {
    color: #fb5;
  }

  .reset-spacer {
    width: 16px;
    flex-shrink: 0;
  }
</style>
