<script lang="ts">
  import type { RenderedControl } from 'src/geoscript/runner/types';
  import type { RampSpecJson, RampStopJson } from 'src/geoscript/geotoyAPIClient';
  import { controlKey } from 'src/geoscript/controlsUi';
  import { drawRampPreview, linearToSrgb, sampleRampSpec, srgbToLinear } from 'src/geoscript/rampPreview';
  import { dragAlongBar, redrawOn } from 'src/viz/UI/controlSection';
  import 'src/viz/UI/controlSection.css';

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

  // In-flight edit, rendered locally until released. Raw: `commit` hands the spec to
  // `treeState`, which `structuredClone`s it — a state proxy throws there.
  let draft = $state.raw<{ key: string; spec: RampSpecJson } | null>(null);

  const rows = $derived(
    controls
      .filter(c => c.kind === 'ramp')
      .map(c => {
        const key = controlKey(c);
        return {
          c,
          key,
          label: c.label ?? c.handleId,
          spec: draft?.key === key ? draft.spec : getSpec(key),
        };
      })
      .filter((r): r is typeof r & { spec: RampSpecJson } => r.spec !== null)
  );

  let selected = $state<{ key: string; ix: number } | null>(null);

  const canvasFor = redrawOn(drawRampPreview);

  const cloneSpec = (spec: RampSpecJson): RampSpecJson => JSON.parse(JSON.stringify(spec));

  /** Keeps stops sorted (stable) so the bar and wasm agree on order. */
  const sortStops = (spec: RampSpecJson, keepIx?: number): number => {
    const tagged = spec.stops.map((s, i) => ({ s, i }));
    tagged.sort((a, b) => a.s.pos - b.s.pos || a.i - b.i);
    spec.stops = tagged.map(t => t.s);
    return keepIx === undefined ? -1 : tagged.findIndex(t => t.i === keepIx);
  };

  const commit = (key: string, spec: RampSpecJson, keepIx?: number): number => {
    const ix = sortStops(spec, keepIx);
    draft = null;
    onChange(key, spec);
    return ix;
  };

  /** Render-only update; the re-run is deferred to the matching `commit` on release. */
  const preview = (key: string, spec: RampSpecJson, keepIx?: number): number => {
    const ix = sortStops(spec, keepIx);
    draft = { key, spec };
    return ix;
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

  // The native picker reports only `input`/`change` (Chrome fires both on every color change),
  // so a release of one of its sliders is observable only as a settle in that stream.
  const COLOR_SETTLE_MS = 150;
  let colorSettleTimer = 0;

  const onStopColor = (row: (typeof rows)[number], ix: number, hex: string) => {
    if (hex === stopColorHex(row.spec, row.spec.stops[ix])) return;
    const spec = cloneSpec(row.spec);
    const lin = srgbToLinear([
      parseInt(hex.slice(1, 3), 16) / 255,
      parseInt(hex.slice(3, 5), 16) / 255,
      parseInt(hex.slice(5, 7), 16) / 255,
    ]);
    spec.stops[ix].value = spec.scalar ? [lin[0]] : [...lin];
    preview(row.key, spec);
    clearTimeout(colorSettleTimer);
    colorSettleTimer = window.setTimeout(flushColorSettle, COLOR_SETTLE_MS);
  };

  /** Land a pending color pick before anything else takes over the draft. */
  const flushColorSettle = () => {
    clearTimeout(colorSettleTimer);
    if (draft) commit(draft.key, draft.spec);
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
    flushColorSettle();
    if (ix === 0 || ix === row.spec.stops.length - 1) return;
    const [lo, hi] = extent(row.spec);
    const start = row.spec;
    let live: RampSpecJson | null = null;
    dragAlongBar(
      e,
      f => {
        live = cloneSpec(start);
        live.stops[ix].pos = Math.min(hi, Math.max(lo, lo + (hi - lo) * f));
        const newIx = preview(row.key, live, ix);
        if (newIx >= 0) selected = { key: row.key, ix: newIx };
      },
      () => {
        if (live) commit(row.key, live);
      }
    );
  };
</script>

{#if rows.length > 0}
  <div class="ctl-section ramps">
    {#each rows as row (row.key)}
      <div class="ctl-head">
        <span class="ctl-head-label">{row.label}</span>
        {#if !row.spec.scalar}
          <select
            class="ctl-select space-select"
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
      <div class="ctl-bar-wrap bar-wrap" ondblclick={e => addStop(row, e)} title="double-click to add a stop">
        <canvas class="ctl-canvas bar" width={228} height={18} use:canvasFor={row.spec}></canvas>
        {#each row.spec.stops as s, ix (ix)}
          <div
            class="ctl-marker marker"
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
        <div class="ctl-fields stop-editor">
          <input
            class="ctl-num num"
            type="number"
            step="any"
            value={s.pos}
            title="position"
            onchange={e => onStopField(row, ix, 'pos', (e.target as HTMLInputElement).value)}
          />
          {#if row.spec.scalar}
            <input
              class="ctl-num num"
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
              onchange={e => onStopColor(row, ix, (e.target as HTMLInputElement).value)}
            />
          {/if}
          <select
            class="ctl-select ease-select"
            value={s.ease}
            title="easing toward the next stop"
            onchange={e => onStopEase(row, ix, (e.target as HTMLSelectElement).value as RampStopJson['ease'])}
          >
            {#each ['linear', 'smooth', 'smoother', 'step'] as ez (ez)}
              <option value={ez}>{ez}</option>
            {/each}
          </select>
          <button
            class="ctl-btn del"
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
  /* Shared chrome lives in controlSection.css; only the ramp-specific geometry is here. */
  .bar-wrap {
    height: 26px;
    margin: 2px 4px 6px 4px;
  }
  .bar {
    height: 18px;
  }
  .marker {
    top: 14px;
    height: 11px;
  }
  .marker.pinned {
    cursor: pointer;
  }
  .marker.selected {
    border-color: #0ff;
    outline: 1px solid #0ff;
  }
  .stop-editor {
    gap: 5px;
  }
  .num {
    width: 58px;
  }
  .swatch {
    width: 30px;
    height: 20px;
    padding: 0;
    border: 1px solid #444;
    background: none;
  }
  .del {
    font-size: 12px;
  }
</style>
