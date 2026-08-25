<script lang="ts">
  import { ControlPanel, type ControlPanelState } from 'src/viz/UI/ControlPanel';
  import ImageLevelsSection from 'src/viz/UI/ImageLevelsSection.svelte';
  import RampControlsSection from 'src/viz/UI/RampControlsSection.svelte';
  import { controlCurrentValue, controlKeyHandleId, controlToSetting } from 'src/geoscript/controlsUi';
  import type { RenderedControl } from 'src/geoscript/runner/types';
  import type { ImageLevelsJson, RampSpecJson } from 'src/geoscript/geotoyAPIClient';
  import type { InputValueJson } from './types';
  import { reifyInput } from './inputInjection';
  import type { ObjectInputsInfo } from './levelEditorPanelTypes';

  interface Props {
    info: ObjectInputsInfo;
    nodeId: string | null;
    onchange: (handleId: string, value: InputValueJson) => void;
  }

  let { info, nodeId, onchange }: Props = $props();

  // Level-def inputs are keyed by bare name; collapse duplicate declarations across modules.
  // `uv_params` controls have no level-editor widget and aren't exposed as object inputs.
  const uniqueControls = $derived.by(() => {
    const seen = new Set<string>();
    const out: RenderedControl[] = [];
    for (const c of info.controls) {
      if (c.kind !== 'uv_params' && !seen.has(c.handleId)) {
        seen.add(c.handleId);
        out.push(c);
      }
    }
    return out;
  });
  const settings = $derived(uniqueControls.map(c => controlToSetting(c, c.handleId)).filter(s => s !== null));

  const inputJsonToPanelValue = (c: RenderedControl, v: InputValueJson): any => {
    const w = reifyInput(v);
    switch (c.kind) {
      case 'float':
      case 'int':
        return w.value?.[0] ?? 0;
      case 'bool':
        return (w.value?.[0] ?? 0) !== 0;
      case 'color':
        return [w.value?.[0] ?? 0, w.value?.[1] ?? 0, w.value?.[2] ?? 0];
      case 'select':
        return w.str_value ?? '';
      case 'ramp':
      case 'image_levels':
        return v.value;
      case 'uv_params':
        return null; // filtered out of uniqueControls
    }
  };

  const panelValueToInputJson = (kind: RenderedControl['kind'], value: any): InputValueJson => {
    switch (kind) {
      case 'float':
        return { type: 'float', value: value as number };
      case 'int':
        return { type: 'int', value: Math.round(value as number) };
      case 'bool':
        return { type: 'bool', value: !!value };
      case 'color':
        return { type: 'color', value: value as [number, number, number] };
      case 'select':
        return { type: 'select', value: value as string };
      case 'spline':
        // Splines never route through the ControlPanel; edits flow via the viewport overlay.
        return { type: 'spline', value: value as [number, number, number][] };
      case 'ramp':
        return { type: 'ramp', value: value as RampSpecJson };
      case 'image_levels':
        return { type: 'image_levels', value: value as ImageLevelsJson };
      case 'uv_params':
        throw new Error('uv_params controls are not exposed as level-def inputs');
    }
  };

  // Reseed panel state when the selected node changes; between edits the panel owns its
  // state optimistically so slider drags stay responsive across debounced rebuilds.
  let panelState = $state<ControlPanelState>({});
  let seededFor: string | null = null;
  $effect(() => {
    if (seededFor === nodeId) return;
    seededFor = nodeId;
    const next: ControlPanelState = {};
    for (const c of uniqueControls) {
      const ov = info.overrides[c.handleId];
      next[c.handleId] = ov !== undefined ? inputJsonToPanelValue(c, ov) : controlCurrentValue(c);
    }
    panelState = next;
  });

  const handleChange = (key: string, value: any) => {
    const c = uniqueControls.find(c => c.handleId === key);
    if (c) onchange(key, panelValueToInputJson(c.kind, value));
  };

  // The sections key rows by module-qualified `controlKey`; state here is by bare name.
  const onSectionChange = (key: string, value: RampSpecJson | ImageLevelsJson) => {
    const handleId = controlKeyHandleId(key);
    panelState = { ...panelState, [handleId]: value };
    handleChange(handleId, value);
  };
</script>

<div class="object-inputs-panel">
  {#if settings.length > 0}
    <ControlPanel {settings} bind:state={panelState} onChange={handleChange} title="inputs" width={252} />
  {/if}
  <RampControlsSection
    controls={uniqueControls}
    getSpec={key => (panelState[controlKeyHandleId(key)] as RampSpecJson | undefined) ?? null}
    onChange={onSectionChange}
  />
  <ImageLevelsSection
    controls={uniqueControls}
    getValue={key => (panelState[controlKeyHandleId(key)] as ImageLevelsJson | undefined) ?? null}
    onChange={onSectionChange}
  />
</div>

<style>
  .object-inputs-panel {
    border-top: 1px solid #333;
    margin-top: 6px;
    padding-top: 6px;
  }

  .object-inputs-panel :global(.control-panel) {
    background: transparent;
    border: none;
  }
</style>
