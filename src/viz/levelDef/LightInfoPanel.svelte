<script lang="ts">
  import type { LightDef } from './types';
  import { hexIntToStr, hexStrToInt } from './colorUtils';
  import { fmt } from './mathUtils';
  import Vec3Input from './Vec3Input.svelte';

  interface Props {
    lightDef: LightDef;
    /** Synced from Three.js light position (updated by TransformControls). */
    lightPosition: [number, number, number];
    onapplyposition: (pos: [number, number, number]) => void;
    onpropertychange: (update: Partial<LightDef>) => void;
    ondelete: () => void;
  }

  let { lightDef, lightPosition, onapplyposition, onpropertychange, ondelete }: Props = $props();

  const hasColor = (def: LightDef): boolean => def.type !== 'hemisphere';
  const isHemisphere = (def: LightDef): boolean => def.type === 'hemisphere';
  const isRectArea = (def: LightDef): boolean => def.type === 'rectArea';
  const hasPosition = (def: LightDef): boolean => def.type !== 'ambient' && def.type !== 'hemisphere';
  const hasDistanceDecay = (def: LightDef): boolean => def.type === 'point' || def.type === 'spot';
  const hasShadow = (def: LightDef): boolean =>
    def.type === 'directional' || def.type === 'point' || def.type === 'spot';
  const isSpot = (def: LightDef): boolean => def.type === 'spot';

  // --- Color ---
  const colorStr = $derived(hexIntToStr((lightDef as any).color ?? 0xffffff));

  const onColorChange = (e: Event) => {
    const val = (e.target as HTMLInputElement).value;
    onpropertychange({ color: hexStrToInt(val) } as Partial<LightDef>);
  };

  // --- Hemisphere sky/ground colors ---
  const skyColorStr = $derived(hexIntToStr((lightDef as any).skyColor ?? 0xffffff));
  const groundColorStr = $derived(hexIntToStr((lightDef as any).groundColor ?? 0x444444));

  const onSkyColorChange = (e: Event) => {
    onpropertychange({ skyColor: hexStrToInt((e.target as HTMLInputElement).value) } as Partial<LightDef>);
  };
  const onGroundColorChange = (e: Event) => {
    onpropertychange({ groundColor: hexStrToInt((e.target as HTMLInputElement).value) } as Partial<LightDef>);
  };

  // --- Intensity ---
  let intensityDraft = $state('');
  let intensityFocused = $state(false);
  $effect(() => {
    if (!intensityFocused) intensityDraft = fmt(lightDef.intensity ?? 1);
  });

  const commitIntensity = () => {
    const n = parseFloat(intensityDraft);
    if (!isNaN(n)) onpropertychange({ intensity: n } as Partial<LightDef>);
    else intensityDraft = fmt(lightDef.intensity ?? 1);
    intensityFocused = false;
  };

  // --- Light-type-specific fields ---
  type FieldKey = 'distance' | 'decay' | 'angle' | 'penumbra' | 'width' | 'height';

  const getFieldValue = (key: FieldKey): number => {
    const def = lightDef as any;
    return def[key] ?? defaultFor(key);
  };

  const defaultFor = (key: FieldKey): number => {
    if (key === 'distance') return 0;
    if (key === 'decay') return 2;
    if (key === 'angle') return Math.PI / 4;
    if (key === 'width' || key === 'height') return 10;
    return 0; // penumbra
  };

  let extraFocused: FieldKey | null = $state(null);
  let extraDraft = $state('');

  const extraDisplayVal = (key: FieldKey): string => {
    if (extraFocused === key) return extraDraft;
    return fmt(getFieldValue(key));
  };

  const onExtraFocus = (key: FieldKey) => {
    extraDraft = fmt(getFieldValue(key));
    extraFocused = key;
  };

  const onExtraBlur = () => {
    commitExtra();
    extraFocused = null;
  };

  const commitExtra = () => {
    if (extraFocused === null) return;
    const n = parseFloat(extraDraft);
    if (isNaN(n)) return;
    onpropertychange({ [extraFocused]: n } as Partial<LightDef>);
  };

  const onExtraKeydown = (e: KeyboardEvent) => {
    if (e.key === 'Enter') {
      commitExtra();
      (e.target as HTMLInputElement).blur();
    }
    if (e.key === 'Escape') {
      extraFocused = null;
      (e.target as HTMLInputElement).blur();
    }
  };

  // --- Cast shadow ---
  const onShadowChange = (e: Event) => {
    onpropertychange({ castShadow: (e.target as HTMLInputElement).checked } as Partial<LightDef>);
  };
</script>

<div class="light-panel">
  <div class="node-header">
    <span class="node-id">{lightDef.id}</span>
    <span class="badge type-badge">{lightDef.type}</span>
  </div>

  <!-- Color (single-color lights) -->
  {#if hasColor(lightDef)}
    <div class="row">
      <span class="tf-label">color</span>
      <input class="color-input" type="color" value={colorStr} onchange={onColorChange} />
      <span class="color-hex">{colorStr}</span>
    </div>
  {/if}

  <!-- Sky / ground colors (hemisphere) -->
  {#if isHemisphere(lightDef)}
    <div class="row">
      <span class="tf-label">sky</span>
      <input class="color-input" type="color" value={skyColorStr} onchange={onSkyColorChange} />
      <span class="color-hex">{skyColorStr}</span>
    </div>
    <div class="row">
      <span class="tf-label">ground</span>
      <input class="color-input" type="color" value={groundColorStr} onchange={onGroundColorChange} />
      <span class="color-hex">{groundColorStr}</span>
    </div>
  {/if}

  <!-- Intensity -->
  <div class="row">
    <span class="tf-label">intens</span>
    <input
      class="tf-input flex1"
      type="text"
      value={intensityFocused ? intensityDraft : fmt(lightDef.intensity ?? 1)}
      oninput={e => {
        intensityDraft = (e.target as HTMLInputElement).value;
      }}
      onfocus={() => {
        intensityDraft = fmt(lightDef.intensity ?? 1);
        intensityFocused = true;
      }}
      onblur={commitIntensity}
      onkeydown={e => {
        if (e.key === 'Enter') {
          commitIntensity();
          (e.target as HTMLInputElement).blur();
        }
        if (e.key === 'Escape') {
          intensityFocused = false;
          (e.target as HTMLInputElement).blur();
        }
      }}
    />
  </div>

  <!-- Position (non-ambient) -->
  {#if hasPosition(lightDef)}
    <Vec3Input label="pos" labelWidth="42px" values={lightPosition} onchange={onapplyposition} />
  {/if}

  <!-- Distance / Decay (point + spot) -->
  {#if hasDistanceDecay(lightDef)}
    {#each ['distance', 'decay'] as const as key}
      <div class="row">
        <span class="tf-label">{key}</span>
        <input
          class="tf-input flex1"
          type="text"
          value={extraDisplayVal(key)}
          oninput={e => {
            extraDraft = (e.target as HTMLInputElement).value;
          }}
          onfocus={() => onExtraFocus(key)}
          onblur={onExtraBlur}
          onkeydown={onExtraKeydown}
        />
      </div>
    {/each}
  {/if}

  <!-- Angle / Penumbra (spot) -->
  {#if isSpot(lightDef)}
    {#each ['angle', 'penumbra'] as const as key}
      <div class="row">
        <span class="tf-label">{key}</span>
        <input
          class="tf-input flex1"
          type="text"
          value={extraDisplayVal(key)}
          oninput={e => {
            extraDraft = (e.target as HTMLInputElement).value;
          }}
          onfocus={() => onExtraFocus(key)}
          onblur={onExtraBlur}
          onkeydown={onExtraKeydown}
        />
      </div>
    {/each}
  {/if}

  <!-- Width / Height (rectArea) -->
  {#if isRectArea(lightDef)}
    {#each ['width', 'height'] as const as key}
      <div class="row">
        <span class="tf-label">{key}</span>
        <input
          class="tf-input flex1"
          type="text"
          value={extraDisplayVal(key)}
          oninput={e => {
            extraDraft = (e.target as HTMLInputElement).value;
          }}
          onfocus={() => onExtraFocus(key)}
          onblur={onExtraBlur}
          onkeydown={onExtraKeydown}
        />
      </div>
    {/each}
  {/if}

  <!-- Cast shadow (shadow-casting lights) -->
  {#if hasShadow(lightDef)}
    <div class="row">
      <span class="tf-label">shadow</span>
      <input type="checkbox" checked={(lightDef as any).castShadow ?? false} onchange={onShadowChange} />
    </div>
  {/if}

  <button class="action-btn delete-btn" onclick={ondelete}>delete light</button>
</div>

<style>
  .light-panel {
    border-top: 1px solid #333;
    margin-top: 6px;
    padding-top: 8px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .node-header {
    display: flex;
    align-items: center;
    gap: 5px;
    margin-bottom: 4px;
  }

  .node-id {
    font-size: 12px;
    color: #ccc;
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .badge {
    font-size: 10px;
    padding: 0 3px;
    flex-shrink: 0;
  }

  .type-badge {
    color: #adf;
    border: 1px solid #36a;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 3px;
  }

  .tf-label {
    width: 42px;
    font-size: 11px;
    color: #888;
    flex-shrink: 0;
  }

  .tf-input {
    flex: 1;
    min-width: 0;
    background: #111;
    color: #ddd;
    border: 1px solid #444;
    padding: 1px 3px;
    font: 11px monospace;
  }

  .tf-input:focus {
    outline: none;
    border-color: #7a7;
  }

  .flex1 {
    flex: 1;
    min-width: 0;
  }

  .color-input {
    width: 28px;
    height: 18px;
    border: 1px solid #444;
    padding: 0;
    cursor: pointer;
    background: none;
    flex-shrink: 0;
  }

  .color-hex {
    font-size: 11px;
    color: #888;
    font-family: monospace;
  }

  .action-btn {
    background: #1a1a1a;
    color: #e8e8e8;
    border: 1px solid #555;
    padding: 3px 6px;
    cursor: pointer;
    font: 11px monospace;
    text-align: center;
    margin-top: 2px;
  }

  .action-btn:hover {
    background: #252525;
  }

  .delete-btn {
    border-color: #633;
    color: #f88;
  }

  .delete-btn:hover {
    background: #2a1a1a;
  }
</style>
