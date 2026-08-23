<script lang="ts">
  import type { RenderedControl } from 'src/geoscript/runner/types';
  import type { ImageLevelsJson } from 'src/geoscript/geotoyAPIClient';
  import { controlKey } from 'src/geoscript/controlsUi';
  import ValueHistogram from 'src/viz/UI/ValueHistogram.svelte';
  import { padWindow } from 'src/geoscript/textureStats';
  import { dragAlongBar } from 'src/viz/UI/controlSection';
  import RowMenu from 'src/viz/UI/RowMenu.svelte';
  import type { SettingAction } from 'src/viz/UI/ControlPanel';
  import 'src/viz/UI/controlSection.css';

  let {
    controls,
    getValue,
    onChange,
    actions,
  }: {
    controls: RenderedControl[];
    /** Optimistic value for a control key (panel state), so edits render immediately. */
    getValue: (key: string) => ImageLevelsJson | null;
    onChange: (key: string, levels: ImageLevelsJson) => void;
    actions?: (c: RenderedControl) => SettingAction[];
  } = $props();

  // In-flight edit, rendered locally until released (the commit re-runs the program).
  let draft = $state.raw<{ key: string; levels: ImageLevelsJson } | null>(null);

  const rows = $derived(
    controls
      .filter(c => c.kind === 'image_levels')
      .map(c => {
        const key = controlKey(c);
        // Histogram window: the unit interval widened to the data, so [0, 1] inputs look as
        // before while signed / Gaussian inputs get a real histogram and draggable range.
        let lo = 0;
        let hi = 1;
        for (const s of c.stats ?? []) {
          lo = Math.min(lo, s.min);
          hi = Math.max(hi, s.max);
        }
        return {
          c,
          key,
          label: c.label ?? c.handleId,
          levels: draft?.key === key ? draft.levels : getValue(key),
          win: padWindow(lo, hi),
        };
      })
      .filter((r): r is typeof r & { levels: ImageLevelsJson } => r.levels !== null)
  );

  const GAMMA_MIN = 0.1;
  const GAMMA_MAX = 10;
  const clamp01 = (v: number) => Math.min(1, Math.max(0, v));
  type Row = (typeof rows)[number];
  /** Bar fraction of a value within the row's histogram window (clamped for display). */
  const frac = (row: Row, v: number) => clamp01((v - row.win[0]) / (row.win[1] - row.win[0]));
  const valueAt = (row: Row, f: number) => row.win[0] + f * (row.win[1] - row.win[0]);

  // Midtone marker position between in_lo/in_hi: the input fraction that maps to 0.5
  // output, i.e. p = 0.5^gamma (gamma 2 sits nearer black and brightens).
  const gammaToFrac = (g: number) => clamp01(Math.pow(0.5, g));
  // Clamp in frac space derived from the gamma bounds, so the marker can address the whole
  // range the number field accepts (a tighter frac clamp silently truncates on click).
  const fracToGamma = (p: number) =>
    Math.log(Math.min(0.5 ** GAMMA_MIN, Math.max(0.5 ** GAMMA_MAX, p))) / Math.log(0.5);

  const commit = (key: string, levels: ImageLevelsJson) => {
    draft = null;
    onChange(key, levels);
  };

  const preview = (key: string, levels: ImageLevelsJson) => {
    draft = { key, levels };
  };

  /** Marker drag: maps pointer x to a bar fraction, previews, commits on release. */
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
        <span class="ctl-head-label">
          {#if row.c.hasOverride}<i class="ctl-dot" title="stored override"></i>{/if}{row.label}
        </span>
        <div class="ctl-head-right">
          <button
            class="ctl-btn reset"
            disabled={isIdentity(row.levels)}
            title="reset to identity"
            onclick={() => commit(row.key, { ...IDENTITY })}
          >
            reset
          </button>
          {#if actions}
            <RowMenu actions={actions(row.c)} />
          {/if}
        </div>
      </div>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="ctl-bar-wrap hist-wrap">
        <div class="hist">
          <ValueHistogram stats={row.c.stats ?? []} lo={row.win[0]} hi={row.win[1]} width={228} height={48} />
        </div>
        <div class="shade" style="left: 0; width: {frac(row, row.levels.in_lo) * 100}%"></div>
        <div class="shade" style="left: {frac(row, row.levels.in_hi) * 100}%; right: 0"></div>
        <div
          class="ctl-marker marker in-lo"
          title="input black point"
          style="left: {frac(row, row.levels.in_lo) * 100}%"
          onpointerdown={e =>
            dragMarker(
              e,
              f => ({ ...row.levels, in_lo: Math.min(valueAt(row, f), row.levels.in_hi - 0.004) }),
              row.key
            )}
        ></div>
        <div
          class="ctl-marker marker gamma"
          title="gamma (midtones)"
          style="left: {frac(row, gammaFracAbs(row.levels)) * 100}%"
          onpointerdown={e =>
            dragMarker(
              e,
              f => {
                const span = Math.max(1e-6, row.levels.in_hi - row.levels.in_lo);
                return { ...row.levels, gamma: fracToGamma((valueAt(row, f) - row.levels.in_lo) / span) };
              },
              row.key
            )}
        ></div>
        <div
          class="ctl-marker marker in-hi"
          title="input white point"
          style="left: {frac(row, row.levels.in_hi) * 100}%"
          onpointerdown={e =>
            dragMarker(
              e,
              f => ({ ...row.levels, in_hi: Math.max(valueAt(row, f), row.levels.in_lo + 0.004) }),
              row.key
            )}
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
    border: 1px solid var(--ctl-field-border);
    box-sizing: border-box;
    overflow: hidden;
  }
  .shade {
    position: absolute;
    top: 0;
    height: 48px;
    background: rgba(0, 0, 0, 0.55);
    pointer-events: none;
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
