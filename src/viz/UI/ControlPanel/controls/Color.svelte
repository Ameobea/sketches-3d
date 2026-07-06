<script lang="ts">
  import type { Rgb } from '../types';

  let { value, onChange }: { value: Rgb; onChange: (v: Rgb) => void } = $props();

  const rgb = $derived(Array.isArray(value) ? value : ([0, 0, 0] as Rgb));

  const clamp01 = (x: number) => Math.min(1, Math.max(0, x));
  const toHex = (c: Rgb) =>
    '#' +
    c
      .map(x =>
        Math.round(clamp01(x) * 255)
          .toString(16)
          .padStart(2, '0')
      )
      .join('');
  const fromHex = (hex: string): Rgb => {
    const n = parseInt(hex.slice(1), 16);
    return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
  };
</script>

<div class="color">
  <input
    type="color"
    value={toHex(rgb)}
    oninput={e => onChange(fromHex((e.currentTarget as HTMLInputElement).value))}
  />
  <span class="readout">{rgb.map(x => x.toFixed(2)).join(', ')}</span>
</div>

<style>
  .color {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
  }

  input[type='color'] {
    flex: 0 0 auto;
    width: 28px;
    height: var(--cp-row-h);
    padding: 0;
    border: none;
    background: none;
    cursor: pointer;
  }

  .readout {
    flex: 1;
    min-width: 0;
    color: var(--cp-text2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
