<script lang="ts">
  import type { SelectSetting } from '../types';

  let {
    setting,
    value,
    onChange,
  }: { setting: SelectSetting; value: unknown; onChange: (v: unknown) => void } = $props();

  // [label, value] pairs; array options use the option as both.
  const entries = $derived(
    Array.isArray(setting.options)
      ? setting.options.map(o => [o, o] as [string, unknown])
      : Object.entries(setting.options)
  );
  const selectedIndex = $derived(
    Math.max(
      0,
      entries.findIndex(([, v]) => v === value)
    )
  );
</script>

<div class="select">
  <select
    value={String(selectedIndex)}
    onchange={e => {
      const el = e.currentTarget as HTMLSelectElement;
      onChange(entries[+el.value][1]);
      el.blur();
    }}
  >
    {#each entries as [label], i (i)}
      <option value={String(i)}>{label}</option>
    {/each}
  </select>
  <span class="triangle"></span>
</div>

<style>
  .select {
    position: relative;
    width: 100%;
    height: var(--cp-row-h);
  }

  select {
    width: 100%;
    height: 100%;
    padding: 0 14px 0 4px;
    border: none;
    outline: none;
    appearance: none;
    -webkit-appearance: none;
    -moz-appearance: none;
    font: inherit;
    background: var(--cp-bg2);
    color: var(--cp-text2);
    cursor: pointer;
  }

  .triangle {
    position: absolute;
    right: 5px;
    top: 50%;
    margin-top: -2px;
    border-left: 3px solid transparent;
    border-right: 3px solid transparent;
    border-top: 5px solid var(--cp-text2);
    pointer-events: none;
  }
</style>
