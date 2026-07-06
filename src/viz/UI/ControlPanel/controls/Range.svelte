<script lang="ts">
  import type { RangeSetting } from '../types';

  let { setting, value, onChange }: { setting: RangeSetting; value: number; onChange: (v: number) => void } =
    $props();

  const current = $derived(
    typeof value === 'number' && Number.isFinite(value)
      ? value
      : (setting.initial ?? (setting.min + setting.max) / 2)
  );

  // Slider always runs in a linear domain; `toValue`/`toSlider` map to/from the real value.
  // Log math mirrors react-control-panel's getLogScalerFunctions.
  const scaler = $derived.by(() => {
    const { min, max, step, steps, scale } = setting;
    if (scale === 'log') {
      const sign = min > 0 ? 1 : -1;
      const lmin = Math.abs(min);
      const lmax = Math.abs(max);
      return {
        sliderMin: 0,
        sliderMax: 100,
        sliderStep: steps ? 100 / steps : 1,
        toValue: (x: number) =>
          sign * Math.exp(Math.log(lmin) + ((Math.log(lmax) - Math.log(lmin)) * x) / 100),
        toSlider: (y: number) =>
          ((Math.log(y * sign) - Math.log(lmin)) * 100) / (Math.log(lmax) - Math.log(lmin)),
      };
    }
    return {
      sliderMin: min,
      sliderMax: max,
      sliderStep: step ?? (steps ? (max - min) / steps : (max - min) / 100),
      toValue: (x: number) => x,
      toSlider: (y: number) => y,
    };
  });

  // During a drag, track the value locally and only commit (fire onChange) on release, so a
  // consumer that re-runs on change isn't hammered every tick. Discrete edits commit immediately.
  let dragging = $state(false);
  let liveVal = $state(0);
  const shown = $derived(dragging ? liveVal : current);

  const fmt = (n: number): string => (Number.isFinite(n) ? String(Number(n.toPrecision(5))) : '0');

  let editing = $state(false);
  let draft = $state('');

  const startEdit = () => {
    draft = fmt(current);
    editing = true;
  };
  const commitEdit = () => {
    const n = parseFloat(draft);
    if (Number.isFinite(n)) onChange(n);
    editing = false;
  };
  const focus = (el: HTMLInputElement) => el.focus();
</script>

<div class="range">
  <input
    type="range"
    min={scaler.sliderMin}
    max={scaler.sliderMax}
    step={scaler.sliderStep}
    value={scaler.toSlider(shown)}
    oninput={e => {
      dragging = true;
      liveVal = scaler.toValue((e.currentTarget as HTMLInputElement).valueAsNumber);
    }}
    onchange={() => {
      onChange(liveVal);
      dragging = false;
    }}
  />
  {#if editing}
    <input
      class="readout edit"
      type="text"
      bind:value={draft}
      use:focus
      onblur={commitEdit}
      onkeydown={e => {
        if (e.key === 'Enter') commitEdit();
        else if (e.key === 'Escape') editing = false;
      }}
    />
  {:else}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <span class="readout" title="double-click to edit" ondblclick={startEdit}>{fmt(shown)}</span>
  {/if}
</div>

<style>
  .range {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
  }

  input[type='range'] {
    flex: 1;
    min-width: 0;
  }

  .readout {
    flex: 0 0 46px;
    height: var(--cp-row-h);
    line-height: var(--cp-row-h);
    padding: 0 2px;
    background: var(--cp-bg2);
    color: var(--cp-text2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: text;
  }

  .edit {
    border: none;
    font: inherit;
    outline: none;
  }
  .edit:focus {
    color: var(--cp-text1);
  }
</style>
