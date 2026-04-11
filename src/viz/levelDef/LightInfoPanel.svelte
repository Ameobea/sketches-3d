<script lang="ts">
  import type { LightDef } from './types';

  interface Props {
    lightDef: LightDef;
    /** Synced from Three.js light position (updated by TransformControls). */
    lightPosition: [number, number, number];
    onapplyposition: (pos: [number, number, number]) => void;
    onpropertychange: (update: Partial<LightDef>) => void;
    ondelete: () => void;
  }

  let { lightDef, lightPosition, onapplyposition, onpropertychange, ondelete }: Props = $props();

  const hasPosition = (def: LightDef): boolean => def.type !== 'ambient';
  const hasDistanceDecay = (def: LightDef): boolean =>
    def.type === 'point' || def.type === 'spot';
  const hasShadow = (def: LightDef): boolean => def.type !== 'ambient';
  const isSpot = (def: LightDef): boolean => def.type === 'spot';

  // --- Color ---
  const numToHex = (n: number): string => '#' + n.toString(16).padStart(6, '0');
  const hexToNum = (s: string): number => parseInt(s.slice(1), 16);

  const colorStr = $derived(numToHex(lightDef.color ?? 0xffffff));

  const onColorChange = (e: Event) => {
    const val = (e.target as HTMLInputElement).value;
    onpropertychange({ color: hexToNum(val) } as Partial<LightDef>);
  };

  // --- Intensity ---
  let intensityDraft = $state('');
  let intensityFocused = $state(false);
  $effect(() => { if (!intensityFocused) intensityDraft = fmt(lightDef.intensity ?? 1); });

  const commitIntensity = () => {
    const n = parseFloat(intensityDraft);
    if (!isNaN(n)) onpropertychange({ intensity: n } as Partial<LightDef>);
    else intensityDraft = fmt(lightDef.intensity ?? 1);
    intensityFocused = false;
  };

  // --- Position ---
  type Axis = 0 | 1 | 2;
  let posFocused: Axis | null = $state(null);
  let posDraft = $state('');

  const posDisplayVal = (axis: Axis): string => {
    if (posFocused === axis) return posDraft;
    return fmt(lightPosition[axis]);
  };

  const onPosFocus = (axis: Axis) => {
    posDraft = fmt(lightPosition[axis]);
    posFocused = axis;
  };

  const onPosBlur = () => {
    commitPos();
    posFocused = null;
  };

  const commitPos = () => {
    if (posFocused === null) return;
    const n = parseFloat(posDraft);
    if (isNaN(n)) return;
    const next: [number, number, number] = [...lightPosition];
    next[posFocused] = n;
    onapplyposition(next);
  };

  const onPosKeydown = (e: KeyboardEvent) => {
    if (e.key === 'Enter') { commitPos(); (e.target as HTMLInputElement).blur(); }
    if (e.key === 'Escape') { posFocused = null; (e.target as HTMLInputElement).blur(); }
  };

  // --- Light-type-specific fields ---
  type FieldKey = 'distance' | 'decay' | 'angle' | 'penumbra';

  const getFieldValue = (key: FieldKey): number => {
    const def = lightDef as any;
    return def[key] ?? defaultFor(key);
  };

  const defaultFor = (key: FieldKey): number => {
    if (key === 'distance') return 0;
    if (key === 'decay') return 2;
    if (key === 'angle') return Math.PI / 4;
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
    if (e.key === 'Enter') { commitExtra(); (e.target as HTMLInputElement).blur(); }
    if (e.key === 'Escape') { extraFocused = null; (e.target as HTMLInputElement).blur(); }
  };

  // --- Cast shadow ---
  const onShadowChange = (e: Event) => {
    onpropertychange({ castShadow: (e.target as HTMLInputElement).checked } as Partial<LightDef>);
  };

  const fmt = (n: number) => {
    const s = n.toFixed(4);
    return s.replace(/(\.\d*?)0+$/, '$1').replace(/\.$/, '.0');
  };

  const axisLabels: [string, string, string] = ['x', 'y', 'z'];
</script>

<div class="light-panel">
  <div class="node-header">
    <span class="node-id">{lightDef.id}</span>
    <span class="badge type-badge">{lightDef.type}</span>
  </div>

  <!-- Color -->
  <div class="row">
    <span class="tf-label">color</span>
    <input
      class="color-input"
      type="color"
      value={colorStr}
      onchange={onColorChange}
    />
    <span class="color-hex">{colorStr}</span>
  </div>

  <!-- Intensity -->
  <div class="row">
    <span class="tf-label">intens</span>
    <input
      class="tf-input flex1"
      type="text"
      value={intensityFocused ? intensityDraft : fmt(lightDef.intensity ?? 1)}
      oninput={(e) => { intensityDraft = (e.target as HTMLInputElement).value; }}
      onfocus={() => { intensityDraft = fmt(lightDef.intensity ?? 1); intensityFocused = true; }}
      onblur={commitIntensity}
      onkeydown={(e) => {
        if (e.key === 'Enter') { commitIntensity(); (e.target as HTMLInputElement).blur(); }
        if (e.key === 'Escape') { intensityFocused = false; (e.target as HTMLInputElement).blur(); }
      }}
    />
  </div>

  <!-- Position (non-ambient) -->
  {#if hasPosition(lightDef)}
    <div class="tf-row">
      <span class="tf-label">pos</span>
      {#each ([0, 1, 2] as const) as axis}
        <span class="axis-label">{axisLabels[axis]}</span>
        <input
          class="tf-input"
          type="text"
          value={posDisplayVal(axis)}
          oninput={(e) => { posDraft = (e.target as HTMLInputElement).value; }}
          onfocus={() => onPosFocus(axis)}
          onblur={onPosBlur}
          onkeydown={onPosKeydown}
        />
      {/each}
    </div>
  {/if}

  <!-- Distance / Decay (point + spot) -->
  {#if hasDistanceDecay(lightDef)}
    {#each (['distance', 'decay'] as const) as key}
      <div class="row">
        <span class="tf-label">{key}</span>
        <input
          class="tf-input flex1"
          type="text"
          value={extraDisplayVal(key)}
          oninput={(e) => { extraDraft = (e.target as HTMLInputElement).value; }}
          onfocus={() => onExtraFocus(key)}
          onblur={onExtraBlur}
          onkeydown={onExtraKeydown}
        />
      </div>
    {/each}
  {/if}

  <!-- Angle / Penumbra (spot) -->
  {#if isSpot(lightDef)}
    {#each (['angle', 'penumbra'] as const) as key}
      <div class="row">
        <span class="tf-label">{key}</span>
        <input
          class="tf-input flex1"
          type="text"
          value={extraDisplayVal(key)}
          oninput={(e) => { extraDraft = (e.target as HTMLInputElement).value; }}
          onfocus={() => onExtraFocus(key)}
          onblur={onExtraBlur}
          onkeydown={onExtraKeydown}
        />
      </div>
    {/each}
  {/if}

  <!-- Cast shadow (non-ambient) -->
  {#if hasShadow(lightDef)}
    <div class="row">
      <span class="tf-label">shadow</span>
      <input
        type="checkbox"
        checked={(lightDef as any).castShadow ?? false}
        onchange={onShadowChange}
      />
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
    border-radius: 2px;
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

  .tf-row {
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

  .axis-label {
    font-size: 10px;
    color: #666;
    width: 8px;
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
