<script lang="ts">
  import type { RenderedControl } from 'src/geoscript/runner/types';
  import type { UvParamsJson } from 'src/geoscript/geotoyAPIClient';
  import { controlKey } from 'src/geoscript/controlsUi';
  import { DEFAULT_UV_ALIGN } from 'src/viz/wasm/uv_unwrap/uvAlign';
  import RowMenu from 'src/viz/UI/RowMenu.svelte';
  import type { SettingAction } from 'src/viz/UI/ControlPanel';
  import 'src/viz/UI/controlSection.css';

  let {
    controls,
    getValue,
    onChange,
    actions,
    onViewUvMaps,
  }: {
    controls: RenderedControl[];
    /** Optimistic value for a control key (panel state), so edits render immediately. */
    getValue: (key: string) => UvParamsJson | null;
    onChange: (key: string, params: UvParamsJson) => void;
    actions?: (c: RenderedControl) => SettingAction[];
    onViewUvMaps?: (c: RenderedControl) => void;
  } = $props();

  const rows = $derived(
    controls
      .filter(c => c.kind === 'uv_params')
      .map(c => {
        const key = controlKey(c);
        return { c, key, label: c.label ?? c.handleId, params: getValue(key) };
      })
      .filter((r): r is typeof r & { params: UvParamsJson } => r.params !== null)
  );

  const TYPES = ['auto', 'unwrap', 'disk', 'sphere', 'planar', 'cylindrical', 'tube', 'strip'];
  const BFF = new Set(['auto', 'unwrap', 'disk', 'sphere']);
  const OPTION_KEYS: Record<string, (keyof UvParamsJson)[]> = {
    bff: ['up', 'fallback', 'axis', 'pre_split'],
    planar: [],
    cylindrical: ['normalize_v'],
    tube: [
      'caps',
      'cap_angle',
      'cap_max_span',
      'cap_alignment',
      'normalize_v',
      'seam_straightness',
      'detwist',
    ],
    strip: ['strip_angle', 'layout', 'u_mode', 'fallback'],
  };
  const familyOf = (t: string) => (BFF.has(t) ? 'bff' : t in OPTION_KEYS ? t : 'planar');

  type Row = (typeof rows)[number];

  const set = (row: Row, patch: Partial<UvParamsJson>) => onChange(row.key, { ...row.params, ...patch });

  const setType = (row: Row, type: string) => {
    const next: UvParamsJson = { type, scale: row.params.scale, n_cones: row.params.n_cones };
    // Options don't survive a family change — `fallback` in particular exists in both the
    // BFF (axis) and strip ('planar'|'error') vocabularies, and a carried-over value from
    // the other family hard-errors the run.
    if (familyOf(type) === familyOf(row.params.type)) {
      const keep = new Set<string>(OPTION_KEYS[familyOf(type)]);
      for (const [k, v] of Object.entries(row.params)) {
        if (keep.has(k) && v !== undefined) (next as any)[k] = v;
      }
    }
    onChange(row.key, next);
  };

  const setNum = (row: Row, field: keyof UvParamsJson, raw: string, int = false) => {
    // clearing an optional field reverts it to the solver default ('auto' placeholders)
    if (raw.trim() === '' && field !== 'scale' && field !== 'n_cones') {
      const { [field]: _drop, ...rest } = row.params;
      onChange(row.key, rest as UvParamsJson);
      return;
    }
    let v = parseFloat(raw);
    if (!Number.isFinite(v)) return;
    if (int) v = Math.round(v);
    // the interpreter hard-errors on scale <= 0 / clamps negative cones; don't store values
    // it will refuse
    if (field === 'scale') v = Math.max(v, 0.01);
    if (field === 'n_cones') v = Math.max(v, 0);
    set(row, { [field]: v });
  };

  const AXES = ['+x', '-x', '+y', '-y', '+z', '-z'];
  const isParallel = (a: string, b: string) => a[1] === b[1];

  const setUp = (row: Row, axis: string) => {
    const patch: Partial<UvParamsJson> = { up: axis };
    if (isParallel(axis, row.params.fallback ?? '-z')) {
      patch.fallback = axis[1] === 'y' ? '-z' : '+y';
    }
    set(row, patch);
  };

  const toggleAlign = (row: Row) => {
    if (row.params.up) {
      const { up: _u, fallback: _f, axis: _a, ...rest } = row.params;
      onChange(row.key, rest as UvParamsJson);
    } else {
      set(row, { ...DEFAULT_UV_ALIGN });
    }
  };
</script>

{#if rows.length > 0}
  <div class="ctl-section uv-params">
    {#each rows as row (row.key)}
      {@const fam = familyOf(row.params.type)}
      <div class="ctl-head">
        <span class="ctl-head-label">
          {#if row.c.hasOverride}<i class="ctl-dot" title="stored override"></i>{/if}{row.label}
        </span>
        <div class="ctl-head-right">
          {#if onViewUvMaps}
            <button class="ctl-btn" title="view the generated uv maps" onclick={() => onViewUvMaps(row.c)}>
              view uv maps
            </button>
          {/if}
          {#if actions}
            <RowMenu actions={actions(row.c)} />
          {/if}
        </div>
      </div>
      <div class="fields">
        <label>
          <span>type</span>
          <select value={row.params.type} onchange={e => setType(row, (e.target as HTMLSelectElement).value)}>
            {#each TYPES as t (t)}
              <option value={t}>{t}</option>
            {/each}
          </select>
        </label>
        <label>
          <span>scale</span>
          <input
            class="ctl-num"
            type="number"
            step="any"
            min="0"
            value={row.params.scale}
            onchange={e => setNum(row, 'scale', (e.target as HTMLInputElement).value)}
          />
        </label>
        {#if fam === 'bff'}
          <label>
            <span>cones</span>
            <input
              class="ctl-num"
              type="number"
              step="1"
              min="0"
              value={row.params.n_cones}
              onchange={e => setNum(row, 'n_cones', (e.target as HTMLInputElement).value, true)}
            />
          </label>
          <label
            title="split sharp edges topologically before unwrapping so creases become island boundaries (like the export pipeline's crease handling); uncheck to unwrap the welded smooth surface"
          >
            <span>split sharp edges</span>
            <input
              type="checkbox"
              checked={row.params.pre_split ?? true}
              onchange={e => set(row, { pre_split: (e.target as HTMLInputElement).checked })}
            />
          </label>
          <label title="rotate each island so a texture axis follows a fixed local-space direction">
            <span>align</span>
            <input type="checkbox" checked={!!row.params.up} onchange={() => toggleAlign(row)} />
          </label>
          {#if row.params.up}
            <label class="wide">
              <span>texture {row.params.axis ?? '+v'} →</span>
              <span class="toggle-group">
                {#each AXES as axis (axis)}
                  <button class:selected={row.params.up === axis} onclick={() => setUp(row, axis)}>
                    {axis.toUpperCase()}
                  </button>
                {/each}
              </span>
            </label>
            <label class="wide" title="used for faces whose normal is near the primary direction">
              <span>fallback →</span>
              <span class="toggle-group">
                {#each AXES as axis (axis)}
                  <button
                    class:selected={(row.params.fallback ?? '-z') === axis}
                    disabled={isParallel(axis, row.params.up)}
                    onclick={() => set(row, { fallback: axis })}
                  >
                    {axis.toUpperCase()}
                  </button>
                {/each}
              </span>
            </label>
            <label title="which texture axis follows the pinned direction">
              <span>pinned axis</span>
              <select
                value={row.params.axis ?? '+v'}
                onchange={e => set(row, { axis: (e.target as HTMLSelectElement).value })}
              >
                <option value="+v">+v (texture up)</option>
                <option value="+u">+u (texture right)</option>
                <option value="-v">−v</option>
                <option value="-u">−u</option>
              </select>
            </label>
          {/if}
        {:else if fam === 'cylindrical'}
          <label title="stretch V to span 0..1 axially instead of square texels">
            <span>normalize v</span>
            <input
              type="checkbox"
              checked={row.params.normalize_v ?? false}
              onchange={e => set(row, { normalize_v: (e.target as HTMLInputElement).checked })}
            />
          </label>
        {:else if fam === 'tube'}
          <label>
            <span>caps</span>
            <select
              value={row.params.caps ?? 'auto'}
              onchange={e => set(row, { caps: (e.target as HTMLSelectElement).value })}
            >
              <option value="auto">auto</option>
              <option value="none">none</option>
            </select>
          </label>
          <label title="crease angle bounding a cap patch (degrees)">
            <span>cap angle</span>
            <input
              class="ctl-num"
              type="number"
              step="any"
              value={row.params.cap_angle ?? ''}
              placeholder="auto"
              onchange={e => setNum(row, 'cap_angle', (e.target as HTMLInputElement).value)}
            />
          </label>
          <label title="max fraction of tube length a cap may span">
            <span>cap span</span>
            <input
              class="ctl-num"
              type="number"
              step="0.01"
              value={row.params.cap_max_span ?? 0.15}
              onchange={e => setNum(row, 'cap_max_span', (e.target as HTMLInputElement).value)}
            />
          </label>
          <label title="min alignment between cap normal and tube direction (0..1)">
            <span>cap align</span>
            <input
              class="ctl-num"
              type="number"
              step="0.05"
              value={row.params.cap_alignment ?? 0.6}
              onchange={e => setNum(row, 'cap_alignment', (e.target as HTMLInputElement).value)}
            />
          </label>
          <label title="penalize the seam cut for traveling around the tube">
            <span>seam straightness</span>
            <input
              class="ctl-num"
              type="number"
              step="any"
              value={row.params.seam_straightness ?? 8}
              onchange={e => setNum(row, 'seam_straightness', (e.target as HTMLInputElement).value)}
            />
          </label>
          <label title="stretch V to span 0..1 over the tube length">
            <span>normalize v</span>
            <input
              type="checkbox"
              checked={row.params.normalize_v ?? false}
              onchange={e => set(row, { normalize_v: (e.target as HTMLInputElement).checked })}
            />
          </label>
          <label title="cancel residual rotational drift of U along the spine">
            <span>detwist</span>
            <input
              type="checkbox"
              checked={row.params.detwist ?? true}
              onchange={e => set(row, { detwist: (e.target as HTMLInputElement).checked })}
            />
          </label>
        {:else if fam === 'strip'}
          <label title="crease angle that splits the mesh into patches (degrees)">
            <span>strip angle</span>
            <input
              class="ctl-num"
              type="number"
              step="any"
              value={row.params.strip_angle ?? ''}
              placeholder="auto"
              onchange={e => setNum(row, 'strip_angle', (e.target as HTMLInputElement).value)}
            />
          </label>
          <label>
            <span>layout</span>
            <select
              value={row.params.layout ?? 'stack'}
              onchange={e => set(row, { layout: (e.target as HTMLSelectElement).value })}
            >
              <option value="stack">stack</option>
              <option value="overlap">overlap</option>
              <option value="fill">fill</option>
            </select>
          </label>
          <label>
            <span>u mode</span>
            <select
              value={row.params.u_mode ?? 'uniform'}
              onchange={e => set(row, { u_mode: (e.target as HTMLSelectElement).value })}
            >
              <option value="uniform">uniform</option>
              <option value="rail">rail</option>
            </select>
          </label>
          <label title="what happens to non-strip patches">
            <span>fallback</span>
            <select
              value={row.params.fallback ?? 'planar'}
              onchange={e => set(row, { fallback: (e.target as HTMLSelectElement).value })}
            >
              <option value="planar">planar</option>
              <option value="error">error</option>
            </select>
          </label>
        {/if}
      </div>
    {/each}
  </div>
{/if}

<style>
  .fields {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 2px 6px 6px;
  }

  .fields label {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
  }

  .fields label > span:first-child {
    width: 110px;
    text-align: right;
    color: #999;
  }

  .fields .ctl-num {
    width: 64px;
  }

  .toggle-group {
    display: flex;
  }

  .toggle-group button {
    background: #333;
    border: 1px solid #555;
    color: #f0f0f0;
    padding: 1px 4px;
    cursor: pointer;
    font-size: 10px;
  }

  .toggle-group button.selected {
    background: #555;
    border-color: #777;
  }

  .toggle-group button:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .toggle-group button:not(:last-child) {
    border-right: none;
  }
</style>
