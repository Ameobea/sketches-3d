<script lang="ts">
  import { untrack } from 'svelte';
  import type { MaterialDef, ShaderPropsJson, ShaderOptionsJson } from './types';
  import FormField from 'src/viz/scenes/geoscriptPlayground/materialEditor/FormField.svelte';

  interface Props {
    materials: Record<string, MaterialDef>;
    textureKeys: string[];
    initialSelectedId?: string | null;
    onchange: (id: string, def: MaterialDef) => void;
    onadd: (id: string, def: MaterialDef) => void;
    ondelete: (id: string) => void;
  }

  let {
    materials,
    textureKeys,
    initialSelectedId = null,
    onchange,
    onadd,
    ondelete,
  }: Props = $props();

  let selectedId = $state<string | null>(untrack(() => initialSelectedId ?? null));

  export const setSelectedId = (id: string | null) => {
    selectedId = id;
  };
  let nameInputMode = $state<'none' | 'new' | 'clone'>('none');
  let nameInputValue = $state('');
  let advancedOpen = $state(false);
  let localDef = $state<MaterialDef | null>(null);

  $effect(() => {
    if (selectedId && materials[selectedId]) {
      localDef = structuredClone(materials[selectedId]) as MaterialDef;
    } else {
      localDef = null;
    }
  });

  const emit = () => {
    if (selectedId && localDef) {
      onchange(selectedId, $state.snapshot(localDef) as MaterialDef);
    }
  };

  const ensureProps = (): ShaderPropsJson => {
    if (!localDef || localDef.type !== 'customShader') throw new Error('unreachable');
    if (!localDef.props) localDef.props = {};
    return localDef.props;
  };

  const ensureOpts = (): ShaderOptionsJson => {
    if (!localDef || localDef.type !== 'customShader') throw new Error('unreachable');
    if (!localDef.options) localDef.options = {};
    return localDef.options;
  };

  // Color conversion: hex int ↔ #rrggbb string
  const hexIntToStr = (c: number | undefined): string =>
    c !== undefined ? '#' + (c >>> 0).toString(16).padStart(6, '0') : '#808080';

  const strToHexInt = (s: string): number => parseInt(s.slice(1), 16);

  const confirmName = () => {
    const name = nameInputValue.trim();
    if (!name || materials[name]) return;

    let newDef: MaterialDef;
    if (nameInputMode === 'clone' && selectedId && materials[selectedId]) {
      newDef = structuredClone(materials[selectedId]) as MaterialDef;
    } else {
      newDef = { type: 'customShader' };
    }

    nameInputMode = 'none';
    nameInputValue = '';
    onadd(name, newDef);
  };

  const cancelName = () => {
    nameInputMode = 'none';
    nameInputValue = '';
  };

  const startNew = () => {
    nameInputValue = '';
    nameInputMode = 'new';
  };

  const startClone = () => {
    if (!selectedId) return;
    nameInputValue = `${selectedId}_copy`;
    nameInputMode = 'clone';
  };

  const handleDelete = () => {
    if (!selectedId) return;
    ondelete(selectedId);
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Enter') confirmName();
    else if (e.key === 'Escape') cancelName();
  };

  // Texture select helper
  const setTexture = (key: keyof ShaderPropsJson, val: string) => {
    (ensureProps() as any)[key] = val === '' ? undefined : val;
    emit();
  };

  // Tile breaking type
  const tileBreakingType = (o: ShaderOptionsJson): string => {
    if (!o.tileBreaking) return '';
    return o.tileBreaking.type;
  };

  const setTileBreaking = (val: string) => {
    const o = ensureOpts();
    if (val === '') {
      o.tileBreaking = undefined;
    } else if (val === 'neyret') {
      o.tileBreaking = { type: 'neyret', patchScale: 1.0 };
    } else {
      o.tileBreaking = { type: 'fastFixMipmap' };
    }
    emit();
  };
</script>

<div class="mat-editor">
  <div class="header">■ MATERIAL EDITOR</div>

  <div class="body">
    <!-- Left column: material list -->
    <div class="mat-list">
      {#each Object.keys(materials) as id (id)}
        <button
          class="mat-item"
          class:active={id === selectedId}
          onclick={() => (selectedId = id)}
        >{id}</button>
      {/each}

      {#if nameInputMode !== 'none'}
        <div class="name-input-row">
          <!-- svelte-ignore a11y_autofocus -->
          <input
            class="name-input"
            type="text"
            bind:value={nameInputValue}
            onkeydown={handleKeyDown}
            placeholder={nameInputMode === 'clone' ? 'clone name...' : 'new name...'}
            autofocus
          />
          <button class="confirm-btn" onclick={confirmName} title="Confirm">✓</button>
          <button class="cancel-btn" onclick={cancelName} title="Cancel">✕</button>
        </div>
      {/if}
    </div>

    <!-- Right column: form -->
    <div class="mat-form">
      {#if localDef?.type === 'customShader'}
        {@const p = localDef.props ?? {}}
        {@const o = localDef.options ?? {}}

        <!-- Core section -->
        <FormField label="color">
          <input
            type="color"
            value={hexIntToStr(p.color)}
            oninput={(e) => {
              ensureProps().color = strToHexInt((e.target as HTMLInputElement).value);
              emit();
            }}
          />
        </FormField>

        <FormField label="roughness">
          <input
            type="range" min="0" max="1" step="0.01"
            value={p.roughness ?? 0.5}
            oninput={(e) => { ensureProps().roughness = Number((e.target as HTMLInputElement).value); emit(); }}
          />
          <span class="val">{(p.roughness ?? 0.5).toFixed(2)}</span>
        </FormField>

        <FormField label="metalness">
          <input
            type="range" min="0" max="1" step="0.01"
            value={p.metalness ?? 0}
            oninput={(e) => { ensureProps().metalness = Number((e.target as HTMLInputElement).value); emit(); }}
          />
          <span class="val">{(p.metalness ?? 0).toFixed(2)}</span>
        </FormField>

        <FormField label="normalScale">
          <input
            type="range" min="0" max="5" step="0.01"
            value={p.normalScale ?? 1}
            oninput={(e) => { ensureProps().normalScale = Number((e.target as HTMLInputElement).value); emit(); }}
          />
          <span class="val">{(p.normalScale ?? 1).toFixed(2)}</span>
        </FormField>

        <FormField label="iridescence">
          <input
            type="range" min="0" max="1" step="0.01"
            value={p.iridescence ?? 0}
            oninput={(e) => { ensureProps().iridescence = Number((e.target as HTMLInputElement).value); emit(); }}
          />
          <span class="val">{(p.iridescence ?? 0).toFixed(2)}</span>
        </FormField>

        <FormField label="map">
          <select
            value={p.map ?? ''}
            onchange={(e) => setTexture('map', (e.target as HTMLSelectElement).value)}
          >
            <option value="">(none)</option>
            {#each textureKeys as k}
              <option value={k}>{k}</option>
            {/each}
          </select>
        </FormField>

        <FormField label="normalMap">
          <select
            value={p.normalMap ?? ''}
            onchange={(e) => setTexture('normalMap', (e.target as HTMLSelectElement).value)}
          >
            <option value="">(none)</option>
            {#each textureKeys as k}
              <option value={k}>{k}</option>
            {/each}
          </select>
        </FormField>

        <FormField label="roughnessMap">
          <select
            value={p.roughnessMap ?? ''}
            onchange={(e) => setTexture('roughnessMap', (e.target as HTMLSelectElement).value)}
          >
            <option value="">(none)</option>
            {#each textureKeys as k}
              <option value={k}>{k}</option>
            {/each}
          </select>
        </FormField>

        <FormField label="metalnessMap">
          <select
            value={p.metalnessMap ?? ''}
            onchange={(e) => setTexture('metalnessMap', (e.target as HTMLSelectElement).value)}
          >
            <option value="">(none)</option>
            {#each textureKeys as k}
              <option value={k}>{k}</option>
            {/each}
          </select>
        </FormField>

        <FormField label="uvScale">
          <input
            type="number" step="0.1" style="width:80px"
            value={p.uvScale?.[0] ?? 1}
            onchange={(e) => {
              const pr = ensureProps();
              pr.uvScale = [Number((e.target as HTMLInputElement).value), pr.uvScale?.[1] ?? 1];
              emit();
            }}
          />
          <input
            type="number" step="0.1" style="width:80px"
            value={p.uvScale?.[1] ?? 1}
            onchange={(e) => {
              const pr = ensureProps();
              pr.uvScale = [pr.uvScale?.[0] ?? 1, Number((e.target as HTMLInputElement).value)];
              emit();
            }}
          />
        </FormField>

        <!-- Options section -->
        <div class="section-divider"></div>

        <FormField label="useGeneratedUVs">
          <input
            type="checkbox"
            checked={o.useGeneratedUVs ?? false}
            onchange={(e) => { ensureOpts().useGeneratedUVs = (e.target as HTMLInputElement).checked; emit(); }}
          />
        </FormField>

        <FormField label="useTriplanarMapping">
          <input
            type="checkbox"
            checked={!!o.useTriplanarMapping}
            onchange={(e) => { ensureOpts().useTriplanarMapping = (e.target as HTMLInputElement).checked; emit(); }}
          />
        </FormField>

        <FormField label="tileBreaking">
          <select
            value={tileBreakingType(o)}
            onchange={(e) => setTileBreaking((e.target as HTMLSelectElement).value)}
          >
            <option value="">(none)</option>
            <option value="neyret">neyret</option>
            <option value="fastFixMipmap">fastFixMipmap</option>
          </select>
        </FormField>

        {#if o.tileBreaking?.type === 'neyret'}
          <FormField label="patchScale">
            <input
              type="number" step="0.1" style="width:80px"
              value={o.tileBreaking.patchScale ?? 1}
              onchange={(e) => {
                const opts = ensureOpts();
                if (opts.tileBreaking?.type === 'neyret') {
                  opts.tileBreaking.patchScale = Number((e.target as HTMLInputElement).value);
                  emit();
                }
              }}
            />
          </FormField>
        {/if}

        <FormField label="randomizeUVOffset">
          <input
            type="checkbox"
            checked={o.randomizeUVOffset ?? false}
            onchange={(e) => { ensureOpts().randomizeUVOffset = (e.target as HTMLInputElement).checked; emit(); }}
          />
        </FormField>

        <FormField label="materialClass">
          <select
            value={o.materialClass ?? 'default'}
            onchange={(e) => {
              ensureOpts().materialClass = (e.target as HTMLSelectElement).value as any;
              emit();
            }}
          >
            <option value="default">default</option>
            <option value="rock">rock</option>
            <option value="crystal">crystal</option>
            <option value="instakill">instakill</option>
          </select>
        </FormField>

        <!-- Advanced section -->
        <div class="section-divider"></div>

        <button class="adv-toggle" onclick={() => (advancedOpen = !advancedOpen)}>
          {advancedOpen ? '▼' : '▶'} advanced
        </button>

        {#if advancedOpen}
          <div class="advanced-content">
            <FormField label="clearcoat">
              <input
                type="range" min="0" max="1" step="0.01"
                value={p.clearcoat ?? 0}
                oninput={(e) => { ensureProps().clearcoat = Number((e.target as HTMLInputElement).value); emit(); }}
              />
              <span class="val">{(p.clearcoat ?? 0).toFixed(2)}</span>
            </FormField>

            <FormField label="clearcoatRoughness">
              <input
                type="range" min="0" max="1" step="0.01"
                value={p.clearcoatRoughness ?? 0}
                oninput={(e) => { ensureProps().clearcoatRoughness = Number((e.target as HTMLInputElement).value); emit(); }}
              />
              <span class="val">{(p.clearcoatRoughness ?? 0).toFixed(2)}</span>
            </FormField>

            <FormField label="clearcoatNormalScale">
              <input
                type="range" min="0" max="5" step="0.01"
                value={p.clearcoatNormalScale ?? 0}
                oninput={(e) => { ensureProps().clearcoatNormalScale = Number((e.target as HTMLInputElement).value); emit(); }}
              />
              <span class="val">{(p.clearcoatNormalScale ?? 0).toFixed(2)}</span>
            </FormField>

            <FormField label="clearcoatNormalMap">
              <select
                value={p.clearcoatNormalMap ?? ''}
                onchange={(e) => setTexture('clearcoatNormalMap', (e.target as HTMLSelectElement).value)}
              >
                <option value="">(none)</option>
                {#each textureKeys as k}
                  <option value={k}>{k}</option>
                {/each}
              </select>
            </FormField>

            <FormField label="sheen">
              <input
                type="range" min="0" max="1" step="0.01"
                value={p.sheen ?? 0}
                oninput={(e) => { ensureProps().sheen = Number((e.target as HTMLInputElement).value); emit(); }}
              />
              <span class="val">{(p.sheen ?? 0).toFixed(2)}</span>
            </FormField>

            <FormField label="sheenRoughness">
              <input
                type="range" min="0" max="1" step="0.01"
                value={p.sheenRoughness ?? 0}
                oninput={(e) => { ensureProps().sheenRoughness = Number((e.target as HTMLInputElement).value); emit(); }}
              />
              <span class="val">{(p.sheenRoughness ?? 0).toFixed(2)}</span>
            </FormField>

            <FormField label="sheenColor">
              <input
                type="color"
                value={hexIntToStr(p.sheenColor)}
                oninput={(e) => {
                  ensureProps().sheenColor = strToHexInt((e.target as HTMLInputElement).value);
                  emit();
                }}
              />
            </FormField>

            <FormField label="fogMultiplier">
              <input
                type="number" step="0.1" style="width:80px"
                value={p.fogMultiplier ?? 1}
                onchange={(e) => { ensureProps().fogMultiplier = Number((e.target as HTMLInputElement).value); emit(); }}
              />
            </FormField>

            <FormField label="ambientLightScale">
              <input
                type="number" step="0.1" style="width:80px"
                value={p.ambientLightScale ?? 1}
                onchange={(e) => { ensureProps().ambientLightScale = Number((e.target as HTMLInputElement).value); emit(); }}
              />
            </FormField>

            <FormField label="mapDisableDistance">
              <input
                type="checkbox"
                checked={p.mapDisableDistance != null}
                onchange={(e) => {
                  ensureProps().mapDisableDistance = (e.target as HTMLInputElement).checked ? 100 : undefined;
                  emit();
                }}
              />
              {#if p.mapDisableDistance != null}
                <input
                  type="number" step="1" style="width:80px"
                  value={p.mapDisableDistance}
                  onchange={(e) => { ensureProps().mapDisableDistance = Number((e.target as HTMLInputElement).value); emit(); }}
                />
              {/if}
            </FormField>

            <FormField label="opacity">
              <input
                type="number" min="0" max="1" step="0.01" style="width:80px"
                value={p.opacity ?? 1}
                onchange={(e) => { ensureProps().opacity = Number((e.target as HTMLInputElement).value); emit(); }}
              />
            </FormField>

            <FormField label="transmission">
              <input
                type="number" min="0" max="1" step="0.01" style="width:80px"
                value={p.transmission ?? 0}
                onchange={(e) => { ensureProps().transmission = Number((e.target as HTMLInputElement).value); emit(); }}
              />
            </FormField>

            <FormField label="ior">
              <input
                type="number" min="1" max="2.5" step="0.01" style="width:80px"
                value={p.ior ?? 1.5}
                onchange={(e) => { ensureProps().ior = Number((e.target as HTMLInputElement).value); emit(); }}
              />
            </FormField>
          </div>
        {/if}

      {:else if localDef?.type === 'customBasicShader'}
        <div class="placeholder">customBasicShader — no live editor</div>
      {:else}
        <div class="placeholder">select a material</div>
      {/if}
    </div>
  </div>

  <!-- Footer -->
  <div class="footer">
    <div class="footer-left">
      <button onclick={startNew}>+ New</button>
      <button onclick={startClone} disabled={!selectedId}>⧉ Clone</button>
    </div>
    <div class="footer-right">
      <button class="delete-btn" onclick={handleDelete} disabled={!selectedId}>✕ Delete</button>
    </div>
  </div>
</div>

<style>
  .mat-editor {
    position: fixed;
    top: 12px;
    right: 12px;
    width: 640px;
    background: #1a1a1a;
    color: #e8e8e8;
    font: 12px 'IBM Plex Mono', monospace;
    border: 1px solid #444;
    z-index: 9999;
    display: flex;
    flex-direction: column;
    max-height: calc(100vh - 24px);
    pointer-events: auto;
    user-select: none;
  }

  .header {
    padding: 8px 12px;
    font-weight: bold;
    color: #7ec8e3;
    border-bottom: 1px solid #444;
    flex-shrink: 0;
  }

  .body {
    display: flex;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .mat-list {
    width: 180px;
    flex-shrink: 0;
    border-right: 1px solid #444;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }

  .mat-item {
    display: block;
    width: 100%;
    background: none;
    border: none;
    border-bottom: 1px solid #333;
    color: #ccc;
    font: 11px 'IBM Plex Mono', monospace;
    padding: 6px 8px;
    text-align: left;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .mat-item:hover {
    background: #252525;
    color: #e8e8e8;
  }

  .mat-item.active {
    background: #2a3a4a;
    color: #7ec8e3;
  }

  .name-input-row {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 4px;
    border-top: 1px solid #444;
    flex-shrink: 0;
  }

  .name-input {
    flex: 1;
    min-width: 0;
    background: #111;
    color: #e8e8e8;
    border: 1px solid #555;
    padding: 3px 4px;
    font: 11px 'IBM Plex Mono', monospace;
  }

  .confirm-btn,
  .cancel-btn {
    background: #1a1a1a;
    border: 1px solid #555;
    color: #e8e8e8;
    cursor: pointer;
    padding: 2px 5px;
    font: 11px monospace;
    flex-shrink: 0;
  }

  .mat-form {
    flex: 1;
    overflow-y: auto;
    padding: 10px 12px;
    font: 12px 'IBM Plex Mono', monospace;
  }

  .placeholder {
    color: #666;
    padding: 12px 0;
    font-style: italic;
  }

  .section-divider {
    border-top: 1px solid #333;
    margin: 10px 0;
  }

  .val {
    color: #aaa;
    width: 36px;
    flex-shrink: 0;
    text-align: right;
    font-size: 11px;
  }

  .adv-toggle {
    background: none;
    border: none;
    color: #888;
    cursor: pointer;
    font: 11px 'IBM Plex Mono', monospace;
    padding: 2px 0;
    margin-bottom: 8px;
  }

  .adv-toggle:hover {
    color: #bbb;
  }

  .advanced-content {
    padding-left: 8px;
  }

  .footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 6px 10px;
    border-top: 1px solid #444;
    flex-shrink: 0;
    gap: 6px;
  }

  .footer-left,
  .footer-right {
    display: flex;
    gap: 4px;
  }

  .footer button,
  .footer-left button,
  .footer-right button {
    background: #1a1a1a;
    border: 1px solid #555;
    color: #e8e8e8;
    cursor: pointer;
    padding: 3px 8px;
    font: 11px 'IBM Plex Mono', monospace;
  }

  .footer button:disabled {
    color: #555;
    border-color: #333;
    cursor: default;
  }

  .footer button:not(:disabled):hover {
    background: #252525;
  }

  .delete-btn {
    color: #e87070 !important;
  }

  .delete-btn:not(:disabled):hover {
    background: #2a1a1a !important;
  }

  /* Input styles for controls not covered by FormField's :global() selectors */
  :global(.control input[type='range']) {
    flex-grow: 1;
    accent-color: #7ec8e3;
  }

  :global(.control input[type='number']) {
    background-color: #1a1a1a;
    color: #eee;
    border: 1px solid #444;
    padding: 3px 5px;
    font-size: 12px;
    font-family: inherit;
    box-sizing: border-box;
  }

  :global(.control input[type='color']) {
    width: 40px;
    height: 22px;
    border: 1px solid #444;
    padding: 1px;
    background: #111;
    cursor: pointer;
  }

  :global(.control input[type='checkbox']) {
    width: 14px;
    height: 14px;
    cursor: pointer;
    accent-color: #7ec8e3;
  }
</style>
