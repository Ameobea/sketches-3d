<script lang="ts">
  import type { RenderedControl } from 'src/geoscript/runner/types';
  import type { ImageLevelsJson } from 'src/geoscript/geotoyAPIClient';
  import { controlKey } from 'src/geoscript/controlsUi';
  import { dragAlongBar, redrawOn } from 'src/viz/UI/controlSection';
  import 'src/viz/UI/controlSection.css';

  let {
    controls,
    getValue,
    onChange,
  }: {
    controls: RenderedControl[];
    /** Optimistic value for a control key (panel state), so edits render immediately. */
    getValue: (key: string) => ImageLevelsJson | null;
    onChange: (key: string, levels: ImageLevelsJson) => void;
  } = $props();

  // In-flight edit, rendered locally until released (the commit re-runs the program).
  let draft = $state.raw<{ key: string; levels: ImageLevelsJson } | null>(null);

  const rows = $derived(
    controls
      .filter(c => c.kind === 'image_levels')
      .map(c => {
        const key = controlKey(c);
        return {
          c,
          key,
          label: c.label ?? c.handleId,
          levels: draft?.key === key ? draft.levels : getValue(key),
        };
      })
      .filter((r): r is typeof r & { levels: ImageLevelsJson } => r.levels !== null)
  );

  const GAMMA_MIN = 0.1;
  const GAMMA_MAX = 10;
  const clamp01 = (v: number) => Math.min(1, Math.max(0, v));

  // Midtone marker position between in_lo/in_hi: the input fraction that maps to 0.5
  // output, i.e. p = 0.5^gamma (gamma 2 sits nearer black and brightens).
  const gammaToFrac = (g: number) => clamp01(Math.pow(0.5, g));
  // Clamp in frac space derived from the gamma bounds, so the marker can address the whole
  // range the number field accepts (a tighter frac clamp silently truncates on click).
  const fracToGamma = (p: number) =>
    Math.log(Math.min(0.5 ** GAMMA_MIN, Math.max(0.5 ** GAMMA_MAX, p))) / Math.log(0.5);

  const drawHistogram = (canvas: HTMLCanvasElement, row: { c: RenderedControl; levels: ImageLevelsJson }) => {
    const ctx = canvas.getContext('2d')!;
    const { width: w, height: h } = canvas;
    ctx.clearRect(0, 0, w, h);
    ctx.fillStyle = '#1a1a1a';
    ctx.fillRect(0, 0, w, h);
    const hist = row.c.histogram;
    if (hist && hist.length > 0) {
      const peak = Math.max(1, ...hist);
      ctx.fillStyle = '#7a7a7a';
      const bw = w / hist.length;
      for (let i = 0; i < hist.length; i++) {
        // sqrt scaling keeps sparse bins visible next to dominant peaks
        const bh = Math.sqrt(hist[i] / peak) * (h - 2);
        if (bh > 0) ctx.fillRect(i * bw, h - bh, Math.max(1, bw), bh);
      }
    }
    // Shade outside the input window
    ctx.fillStyle = 'rgba(0, 0, 0, 0.55)';
    const x0 = clamp01(row.levels.in_lo) * w;
    const x1 = clamp01(row.levels.in_hi) * w;
    if (x0 > 0) ctx.fillRect(0, 0, x0, h);
    if (x1 < w) ctx.fillRect(x1, 0, w - x1, h);
  };

  const histCanvas = redrawOn(drawHistogram);

  const commit = (key: string, levels: ImageLevelsJson) => {
    draft = null;
    onChange(key, levels);
  };

  const preview = (key: string, levels: ImageLevelsJson) => {
    draft = { key, levels };
  };

  /** Marker drag: maps pointer x to [0,1], previews, commits on release. */
  const dragMarker = (e: PointerEvent, apply: (frac: number) => ImageLevelsJson, key: string) => {
    let live: ImageLevelsJson | null = null;
    dragAlongBar(
      e,
      f => {
        live = apply(f);
        preview(key, live);
      },
      () => {
        if (live) commit(key, live);
      },
      true
    );
  };

  const gammaFracAbs = (l: ImageLevelsJson) => l.in_lo + (l.in_hi - l.in_lo) * gammaToFrac(l.gamma);

  const onNumField = (row: (typeof rows)[number], field: keyof ImageLevelsJson, raw: string) => {
    const v = parseFloat(raw);
    if (!Number.isFinite(v)) return;
    commit(row.key, {
      ...row.levels,
      [field]: field === 'gamma' ? Math.min(GAMMA_MAX, Math.max(GAMMA_MIN, v)) : v,
    });
  };

  const IDENTITY: ImageLevelsJson = { in_lo: 0, in_hi: 1, out_lo: 0, out_hi: 1, gamma: 1 };
  const isIdentity = (l: ImageLevelsJson) =>
    (Object.keys(IDENTITY) as (keyof ImageLevelsJson)[]).every(k => l[k] === IDENTITY[k]);

  const NUM_FIELDS: { field: keyof ImageLevelsJson; title: string }[] = [
    { field: 'in_lo', title: 'input black point' },
    { field: 'in_hi', title: 'input white point' },
    { field: 'gamma', title: 'gamma (midtones)' },
    { field: 'out_lo', title: 'output black level' },
    { field: 'out_hi', title: 'output white level' },
  ];
</script>

{#if rows.length > 0}
  <div class="ctl-section levels">
    {#each rows as row (row.key)}
      <div class="ctl-head">
        <span class="ctl-head-label">{row.label}</span>
        <button
          class="ctl-btn reset"
          disabled={isIdentity(row.levels)}
          title="reset to identity"
          onclick={() => commit(row.key, { ...IDENTITY })}
        >
          reset
        </button>
      </div>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="ctl-bar-wrap hist-wrap">
        <canvas class="ctl-canvas hist" width={228} height={48} use:histCanvas={row}></canvas>
        <div
          class="ctl-marker marker in-lo"
          title="input black point"
          style="left: {clamp01(row.levels.in_lo) * 100}%"
          onpointerdown={e =>
            dragMarker(e, f => ({ ...row.levels, in_lo: Math.min(f, row.levels.in_hi - 0.004) }), row.key)}
        ></div>
        <div
          class="ctl-marker marker gamma"
          title="gamma (midtones)"
          style="left: {clamp01(gammaFracAbs(row.levels)) * 100}%"
          onpointerdown={e =>
            dragMarker(
              e,
              f => {
                const span = Math.max(1e-6, row.levels.in_hi - row.levels.in_lo);
                return { ...row.levels, gamma: fracToGamma((f - row.levels.in_lo) / span) };
              },
              row.key
            )}
        ></div>
        <div
          class="ctl-marker marker in-hi"
          title="input white point"
          style="left: {clamp01(row.levels.in_hi) * 100}%"
          onpointerdown={e =>
            dragMarker(e, f => ({ ...row.levels, in_hi: Math.max(f, row.levels.in_lo + 0.004) }), row.key)}
        ></div>
      </div>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="ctl-bar-wrap out-wrap">
        <div class="out-bar"></div>
        <div
          class="ctl-marker marker out-lo"
          title="output black level"
          style="left: {clamp01(row.levels.out_lo) * 100}%"
          onpointerdown={e => dragMarker(e, f => ({ ...row.levels, out_lo: f }), row.key)}
        ></div>
        <div
          class="ctl-marker marker out-hi"
          title="output white level"
          style="left: {clamp01(row.levels.out_hi) * 100}%"
          onpointerdown={e => dragMarker(e, f => ({ ...row.levels, out_hi: f }), row.key)}
        ></div>
      </div>
      <div class="ctl-fields num-row">
        {#each NUM_FIELDS as nf (nf.field)}
          <input
            class="ctl-num num"
            type="number"
            step="any"
            value={Math.round(row.levels[nf.field] * 1000) / 1000}
            title={nf.title}
            onchange={e => onNumField(row, nf.field, (e.target as HTMLInputElement).value)}
          />
        {/each}
      </div>
    {/each}
  </div>
{/if}

<style>
  /* Shared chrome lives in controlSection.css; only the levels-specific geometry is here. */
  .reset {
    font-size: 11px;
  }
  .hist-wrap {
    height: 58px;
    margin: 2px 4px 0 4px;
  }
  .hist {
    height: 48px;
  }
  .out-wrap {
    height: 18px;
    margin: 2px 4px 4px 4px;
  }
  .out-bar {
    height: 8px;
    margin-top: 1px;
    background: linear-gradient(to right, #000, #fff);
    border: 1px solid #444;
  }
  .marker {
    height: 10px;
    clip-path: polygon(50% 0, 100% 45%, 100% 100%, 0 100%, 0 45%);
  }
  .hist-wrap .marker {
    top: 48px;
  }
  .out-wrap .marker {
    top: 8px;
  }
  .marker.in-lo,
  .marker.out-lo {
    background: #000;
  }
  .marker.in-hi,
  .marker.out-hi {
    background: #fff;
  }
  .marker.gamma {
    background: #888;
  }
  .num-row {
    gap: 3px;
  }
  .num {
    width: 44px;
  }
</style>
