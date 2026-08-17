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

  // The native picker reports only `input`/`change` (Chrome fires both on every color change),
  // so a release of one of its sliders is observable only as a settle in that stream. Track the
  // pick locally until then, like Range does mid-drag, so a consumer that re-runs on change
  // isn't hammered every tick.
  const SETTLE_MS = 150;
  let settleTimer = 0;
  let live = $state.raw<Rgb | null>(null);
  const shown = $derived(live ?? rgb);

  const pick = (hex: string) => {
    if (hex === toHex(shown)) return;
    live = fromHex(hex);
    clearTimeout(settleTimer);
    settleTimer = window.setTimeout(() => {
      const v = live;
      live = null;
      if (v) onChange(v);
    }, SETTLE_MS);
  };
</script>

<div class="color">
  <input
    type="color"
    value={toHex(shown)}
    oninput={e => pick((e.currentTarget as HTMLInputElement).value)}
    onchange={e => pick((e.currentTarget as HTMLInputElement).value)}
  />
  <span class="readout">{shown.map(x => x.toFixed(2)).join(', ')}</span>
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
