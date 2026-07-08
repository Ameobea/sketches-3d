<script lang="ts">
  import type { RenderedControl } from 'src/geoscript/runner/types';
  import { controlKey, type SplinePanelCtx } from 'src/geoscript/controlsUi';

  let { controls, spline }: { controls: RenderedControl[]; spline: SplinePanelCtx } = $props();

  // Script-level view-model: a fresh row array per change drives the keyed each.
  const rows = $derived(
    controls
      .filter(c => c.kind === 'spline')
      .map(c => {
        const key = controlKey(c);
        const active = spline.activeKey === key;
        return {
          c,
          key,
          active,
          label: c.label ?? c.handleId,
          count: active ? spline.points.length : Math.floor(c.value.length / 3),
          points: active ? spline.points : [],
          selectedIx: active ? spline.selectedIx : null,
        };
      })
  );

  const onPointField = (ix: number, axis: number, raw: string) => {
    const v = parseFloat(raw);
    if (!Number.isFinite(v)) return;
    const p = [...spline.points[ix]] as [number, number, number];
    p[axis] = v;
    spline.setPoint(ix, p);
  };
</script>

{#if rows.length > 0}
  <div class="splines">
    {#each rows as row (row.key)}
      <div class="spline-row">
        <span class="spline-label">{row.label}</span>
        <span class="spline-count">{row.count} pts</span>
        <button class="spline-btn" class:active={row.active} onclick={() => spline.toggle(row.c)}>
          {row.active ? 'done' : 'edit'}
        </button>
      </div>
      {#if row.active}
        <div class="pt-list">
          {#each row.points as p, i (i)}
            <div
              class="pt-row"
              class:selected={i === row.selectedIx}
              role="button"
              tabindex="0"
              onclick={() => spline.select(i)}
              onkeydown={e => {
                if (e.key === 'Enter') spline.select(i);
              }}
            >
              <span class="pt-ix">{i}</span>
              {#each [0, 1, 2] as axis (axis)}
                <input
                  class="pt-field"
                  type="number"
                  step="any"
                  value={p[axis]}
                  onchange={e => onPointField(i, axis, (e.target as HTMLInputElement).value)}
                  onclick={e => e.stopPropagation()}
                />
              {/each}
              <button
                class="pt-del"
                title="delete point"
                onclick={e => {
                  e.stopPropagation();
                  spline.remove(i);
                }}
              >
                ✕
              </button>
            </div>
          {/each}
          <button class="spline-btn pt-add" onclick={() => spline.add()}>+ add point</button>
        </div>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .splines {
    background: #1a1a1a;
    border: 1px solid #444;
    border-top: none;
    padding: 6px 8px;
    font: 12px monospace;
    color: #ccc;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .spline-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .spline-label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .spline-count {
    color: #888;
    font-size: 11px;
  }

  .spline-btn {
    background: #222;
    color: #e8e8e8;
    border: 1px solid #555;
    padding: 1px 8px;
    cursor: pointer;
    font: 11px monospace;
  }

  .spline-btn.active {
    border-color: #7a7;
    color: #9f9;
  }

  .pt-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 2px 0 4px;
  }

  .pt-row {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 1px 2px;
    border: 1px solid transparent;
    cursor: pointer;
  }

  .pt-row.selected {
    border-color: #7a7;
    background: #162016;
  }

  .pt-ix {
    width: 16px;
    color: #888;
    font-size: 10px;
    text-align: right;
    flex-shrink: 0;
  }

  .pt-field {
    width: 52px;
    background: #111;
    color: #ddd;
    border: 1px solid #444;
    font: 11px monospace;
    padding: 0 2px;
  }

  .pt-del {
    background: transparent;
    color: #c66;
    border: none;
    cursor: pointer;
    font-size: 11px;
    padding: 0 2px;
  }

  .pt-add {
    align-self: flex-start;
    margin-left: 20px;
  }
</style>
