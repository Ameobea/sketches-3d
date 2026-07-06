<script lang="ts">
  import { ControlPanel, type ControlPanelState } from 'src/viz/UI/ControlPanel';
  import { controlCurrentValue, controlToSetting } from 'src/geoscript/controlsUi';
  import type { RenderedControl } from 'src/geoscript/runner/types';
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
  const uniqueControls = $derived.by(() => {
    const seen = new Set<string>();
    const out: RenderedControl[] = [];
    for (const c of info.controls) {
      if (!seen.has(c.handleId)) {
        seen.add(c.handleId);
        out.push(c);
      }
    }
    return out;
  });
  const settings = $derived(uniqueControls.map(c => controlToSetting(c, c.handleId)));

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
</script>

<div class="object-inputs-panel">
  <ControlPanel {settings} bind:state={panelState} onChange={handleChange} title="inputs" width={252} />
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
