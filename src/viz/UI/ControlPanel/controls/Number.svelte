<script lang="ts">
  import type { NumberSetting } from '../types';

  let { setting, value, onChange }: { setting: NumberSetting; value: number; onChange: (v: number) => void } =
    $props();

  const current = $derived(
    typeof value === 'number' && Number.isFinite(value) ? value : (setting.initial ?? 0)
  );

  const commit = (raw: string) => {
    const n = parseFloat(raw);
    if (Number.isFinite(n)) onChange(n);
  };
</script>

<input
  type="number"
  min={setting.min}
  max={setting.max}
  step={setting.step}
  value={current}
  oninput={e => commit((e.currentTarget as HTMLInputElement).value)}
/>

<style>
  input {
    width: 100%;
    height: var(--cp-row-h);
    padding: 0 4px;
    border: none;
    outline: none;
    background: var(--cp-bg2);
    color: var(--cp-text2);
    font: inherit;
  }
  input:focus {
    color: var(--cp-text1);
  }
</style>
