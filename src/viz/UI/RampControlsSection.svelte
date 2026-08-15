<script lang="ts">
  import type { RenderedControl } from 'src/geoscript/runner/types';
  import type { RampSpecJson, RampStopJson } from 'src/geoscript/geotoyAPIClient';
  import { controlKey } from 'src/geoscript/controlsUi';
  import { drawRampPreview, linearToSrgb, sampleRampSpec, srgbToLinear } from 'src/geoscript/rampPreview';

  let {
    controls,
    getSpec,
    onChange,
  }: {
    controls: RenderedControl[];
    /** Optimistic spec for a control key (panel state), so edits render immediately. */
    getSpec: (key: string) => RampSpecJson | null;
    onChange: (key: string, spec: RampSpecJson) => void;
  } = $props();

  const rows = $derived(
    controls
      .filter(c => c.kind === 'ramp')
      .map(c => ({ c, key: controlKey(c), label: c.label ?? c.handleId, spec: getSpec(controlKey(c)) }))
      .filter((r): r is typeof r & { spec: RampSpecJson } => r.spec !== null)
  );

  let selected = $state<{ key: string; ix: number } | null>(null);

  const canvasFor = (canvas: HTMLCanvasElement, spec: RampSpecJson) => {
    drawRampPreview(canvas, spec);
    return {
      update(next: RampSpecJson) {
        drawRampPreview(canvas, next);
      },
    };
  };

  const cloneSpec = (spec: RampSpecJson): RampSpecJson => JSON.parse(JSON.stringify(spec));

  /** Commit an edit; keeps stops sorted (stable) so the bar and wasm agree on order. */
  const commit = (key: string, spec: RampSpecJson, keepIx?: number): number => {
    const tagged = spec.stops.map((s, i) => ({ s, i }));
    tagged.sort((a, b) => a.s.pos - b.s.pos || a.i - b.i);
    const newIx = keepIx !== undefined ? tagged.findIndex(t => t.i === keepIx) : -1;
    spec.stops = tagged.map(t => t.s);
    onChange(key, spec);
    return newIx;
  };

  const extent = (spec: RampSpecJson): [number, number] => [
    spec.stops[0]?.pos ?? 0,
    spec.stops[spec.stops.length - 1]?.pos ?? 1,
  ];

  const frac = (spec: RampSpecJson, pos: number): number => {
    const [lo, hi] = extent(spec);
    return hi > lo ? (pos - lo) / (hi - lo) : 0;
  };

  const stopColorHex = (spec: RampSpecJson, s: RampStopJson): string => {
    const lin: [number, number, number] = spec.scalar
      ? [s.value[0], s.value[0], s.value[0]]
      : [s.value[0], s.value[1], s.value[2]];
    const enc = linearToSrgb(lin);
    const h = (c: number) =>
      Math.round(Math.min(1, Math.max(0, c)) * 255)
        .toString(16)
        .padStart(2, '0');
    return `#${h(enc[0])}${h(enc[1])}${h(enc[2])}`;
  };

  const onStopColor = (row: (typeof rows)[number], ix: number, hex: string) => {
    const spec = cloneSpec(row.spec);
    const lin = srgbToLinear([
      parseInt(hex.slice(1, 3), 16) / 255,
      parseInt(hex.slice(3, 5), 16) / 255,
      parseInt(hex.slice(5, 7), 16) / 255,
    ]);
    spec.stops[ix].value = spec.scalar ? [lin[0]] : [...lin];
    commit(row.key, spec);
  };

  const onStopField = (row: (typeof rows)[number], ix: number, field: 'pos' | 'value', raw: string) => {
    const v = parseFloat(raw);
    if (!Number.isFinite(v)) return;
    const spec = cloneSpec(row.spec);
    if (field === 'pos') spec.stops[ix].pos = v;
    else spec.stops[ix].value = [v];
    const newIx = commit(row.key, spec, ix);
    if (selected?.key === row.key && newIx >= 0) selected = { key: row.key, ix: newIx };
  };

  const onStopEase = (row: (typeof rows)[number], ix: number, ease: RampStopJson['ease']) => {
    const spec = cloneSpec(row.spec);
    spec.stops[ix].ease = ease;
    commit(row.key, spec);
  };

  const addStop = (row: (typeof rows)[number], e: MouseEvent) => {
    const bar = e.currentTarget as HTMLElement;
    const r = bar.getBoundingClientRect();
    const [lo, hi] = extent(row.spec);
    const pos = lo + (hi - lo) * Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
    const val = sampleRampSpec(row.spec, pos);
    const spec = cloneSpec(row.spec);
    spec.stops.push({ pos, value: spec.scalar ? [val[0]] : [...val], ease: 'linear' });
    const ix = commit(row.key, spec, spec.stops.length - 1);
    selected = { key: row.key, ix };
  };

  const removeStop = (row: (typeof rows)[number], ix: number) => {
    if (row.spec.stops.length <= 2) return;
    const spec = cloneSpec(row.spec);
    spec.stops.splice(ix, 1);
    commit(row.key, spec);
    selected = null;
  };

  // Interior stops drag along the bar; the two extremes pin the extent (edit their pos
  // numerically instead) so the bar's mapping stays stable mid-drag.
  const onMarkerPointerDown = (row: (typeof rows)[number], ix: number, e: PointerEvent) => {
    selected = { key: row.key, ix };
    if (ix === 0 || ix === row.spec.stops.length - 1) return;
    const marker = e.currentTarget as HTMLElement;
    const bar = marker.parentElement!;
    marker.setPointerCapture(e.pointerId);
    const [lo, hi] = extent(row.spec);
    const onMove = (ev: PointerEvent) => {
      const r = bar.getBoundingClientRect();
      const pos = lo + (hi - lo) * Math.min(1, Math.max(0, (ev.clientX - r.left) / r.width));
      const spec = cloneSpec(row.spec);
      spec.stops[ix].pos = Math.min(hi, Math.max(lo, pos));
      const newIx = commit(row.key, spec, ix);
      if (newIx >= 0) selected = { key: row.key, ix: newIx };
    };
    const onUp = () => {
      marker.removeEventListener('pointermove', onMove);
      marker.removeEventListener('pointerup', onUp);
      marker.removeEventListener('pointercancel', onUp);
    };
    marker.addEventListener('pointermove', onMove);
    marker.addEventListener('pointerup', onUp);
    marker.addEventListener('pointercancel', onUp);
  };
</script>

{#if rows.length > 0}
  <div class="ramps">
    {#each rows as row (row.key)}
      <div class="ramp-row">
        <span class="ramp-label">{row.label}</span>
        {#if !row.spec.scalar}
          <select
            class="space-select"
            value={row.spec.space}
            onchange={e => {
              const spec = cloneSpec(row.spec);
              spec.space = (e.target as HTMLSelectElement).value as RampSpecJson['space'];
              commit(row.key, spec);
            }}
          >
            {#each ['oklab', 'oklch', 'linear', 'srgb'] as s (s)}
              <option value={s}>{s}</option>
            {/each}
          </select>
        {/if}
      </div>
      <!-- svelte-ignore a11y_no_static_element_interactions, a11y_no_noninteractive_element_interactions -->
      <div class="bar-wrap" ondblclick={e => addStop(row, e)} title="double-click to add a stop">
        <canvas class="bar" width={228} height={18} use:canvasFor={row.spec}></canvas>
        {#each row.spec.stops as s, ix (ix)}
          <div
            class="marker"
            class:selected={selected?.key === row.key && selected.ix === ix}
            class:pinned={ix === 0 || ix === row.spec.stops.length - 1}
            style="left: {frac(row.spec, s.pos) * 100}%; background: {stopColorHex(row.spec, s)}"
            onpointerdown={e => onMarkerPointerDown(row, ix, e)}
          ></div>
        {/each}
      </div>
      {#if selected?.key === row.key && row.spec.stops[selected.ix]}
        {@const ix = selected.ix}
        {@const s = row.spec.stops[ix]}
        <div class="stop-editor">
          <input
            class="num"
            type="number"
            step="any"
            value={s.pos}
            title="position"
            onchange={e => onStopField(row, ix, 'pos', (e.target as HTMLInputElement).value)}
          />
          {#if row.spec.scalar}
            <input
              class="num"
              type="number"
              step="any"
              value={s.value[0]}
              title="value"
              onchange={e => onStopField(row, ix, 'value', (e.target as HTMLInputElement).value)}
            />
          {:else}
            <input
              class="swatch"
              type="color"
              value={stopColorHex(row.spec, s)}
              oninput={e => onStopColor(row, ix, (e.target as HTMLInputElement).value)}
            />
          {/if}
          <select
            class="ease-select"
            value={s.ease}
            title="easing toward the next stop"
            onchange={e => onStopEase(row, ix, (e.target as HTMLSelectElement).value as RampStopJson['ease'])}
          >
            {#each ['linear', 'smooth', 'smoother', 'step'] as ez (ez)}
              <option value={ez}>{ez}</option>
            {/each}
          </select>
          <button
            class="del"
            disabled={row.spec.stops.length <= 2}
            title="remove stop"
            onclick={() => removeStop(row, ix)}
          >
            ×
          </button>
        </div>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .ramps {
    background: #141414;
    border: 1px solid #2e2e2e;
    padding: 6px 8px 8px 8px;
    width: 260px;
    box-sizing: border-box;
    font-size: 12px;
    color: #ddd;
  }
  .ramp-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 3px 0;
  }
  .ramp-label {
    color: #aaa;
  }
  .space-select,
  .ease-select {
    background: #1a1a1a;
    color: #ddd;
    border: 1px solid #444;
    font-size: 11px;
  }
  .bar-wrap {
    position: relative;
    height: 26px;
    margin: 2px 4px 6px 4px;
  }
  .bar {
    width: 100%;
    height: 18px;
    display: block;
    border: 1px solid #444;
    box-sizing: border-box;
  }
  .marker {
    position: absolute;
    top: 14px;
    width: 9px;
    height: 11px;
    transform: translateX(-50%);
    border: 1px solid #ccc;
    cursor: grab;
    touch-action: none;
  }
  .marker.pinned {
    cursor: pointer;
  }
  .marker.selected {
    border-color: #0ff;
    outline: 1px solid #0ff;
  }
  .stop-editor {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 2px 4px 4px 4px;
  }
  .num {
    width: 58px;
    background: #1a1a1a;
    color: #ddd;
    border: 1px solid #444;
    font-size: 11px;
    padding: 1px 3px;
  }
  .swatch {
    width: 30px;
    height: 20px;
    padding: 0;
    border: 1px solid #444;
    background: none;
  }
  .del {
    background: #333;
    border: 1px solid #555;
    color: #f0f0f0;
    cursor: pointer;
    font-size: 12px;
    padding: 0 6px;
  }
  .del:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>
