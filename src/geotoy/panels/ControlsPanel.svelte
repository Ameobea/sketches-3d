<script lang="ts">
  import { ControlPanel, type ControlPanelState, type SettingAction } from 'src/viz/UI/ControlPanel';
  import ImageLevelsSection from 'src/viz/UI/ImageLevelsSection.svelte';
  import RampControlsSection from 'src/viz/UI/RampControlsSection.svelte';
  import SplineControlsSection from 'src/viz/UI/SplineControlsSection.svelte';
  import type { RenderedControl } from 'src/geoscript/runner/types';
  import type { ImageLevelsJson, RampSpecJson } from 'src/geoscript/geotoyAPIClient';
  import {
    controlCurrentValue,
    controlKey,
    controlToSetting,
    type SplinePanelCtx,
  } from 'src/geoscript/controlsUi';
  import { topLeftOffset, TopLeftSlot } from 'src/geotoy/modules/topLeftOverlay.svelte';
  import type { ControlValue } from 'src/geoscript/geotoyAPIClient';
  import type { TreeState } from 'src/geotoy/modules/treeState.svelte';

  let {
    controls,
    treeState,
    moduleNameToNodeId,
    onEdit,
    onBakeDefault,
    onResetControl,
    spline,
  }: {
    controls: RenderedControl[];
    treeState: TreeState;
    moduleNameToNodeId: Record<string, string>;
    onEdit: () => void;
    /** Write the stored value into the source `default=` and drop the override. */
    onBakeDefault: (c: RenderedControl) => void;
    /** Drop the stored override so the source `default=` applies again. */
    onResetControl: (c: RenderedControl) => void;
    spline?: SplinePanelCtx;
  } = $props();

  const keyOf = controlKey;

  const actionsFor = (c: RenderedControl): SettingAction[] =>
    targets.has(keyOf(c))
      ? [
          {
            label: 'set as default',
            title: 'Write this value into the source as `default=` and clear the stored override',
            disabled: !c.hasOverride,
            action: () => onBakeDefault(c),
          },
          {
            label: 'reset to default',
            title: 'Drop the stored override; the source `default=` applies again',
            disabled: !c.hasOverride,
            action: () => onResetControl(c),
          },
        ]
      : [];

  const settings = $derived(
    controls
      .map(c => {
        const s = controlToSetting(c, keyOf(c));
        return s && { ...s, modified: c.hasOverride, actions: actionsFor(c) };
      })
      .filter(s => s !== null)
  );

  const targets = $derived.by(() => {
    const m = new Map<string, { nodeId: string; handleId: string; kind: RenderedControl['kind'] }>();
    for (const c of controls) {
      const nodeId = c.sourceModule ? moduleNameToNodeId[c.sourceModule] : undefined;
      if (nodeId) m.set(keyOf(c), { nodeId, handleId: c.handleId, kind: c.kind });
    }
    return m;
  });

  // Rebuild displayed state only when a fresh run reports new controls; between runs the
  // panel owns its edits optimistically (via bind) so dragging stays responsive.
  let panelState = $state<ControlPanelState>({});
  let lastControls: RenderedControl[] | null = null;
  $effect(() => {
    if (controls === lastControls) return;
    lastControls = controls;
    const next: ControlPanelState = {};
    for (const c of controls) next[keyOf(c)] = controlCurrentValue(c);
    panelState = next;
  });

  const handleChange = (key: string, value: any) => {
    const t = targets.get(key);
    if (!t) return;
    startUndo(key, t.nodeId, t.handleId);
    treeState.setControl(t.nodeId, t.handleId, toControlValue(t.kind, value));
    onEdit();
    clearTimeout(undoTimer);
    undoTimer = window.setTimeout(flushUndo, 400);
  };

  // Coalesce a burst of edits to one handle (e.g. a slider drag) into a single undo entry.
  let pending: { key: string; nodeId: string; handleId: string; before: ControlValue | null } | null = null;
  let undoTimer = 0;
  const startUndo = (key: string, nodeId: string, handleId: string) => {
    if (pending?.key === key) return;
    flushUndo();
    pending = { key, nodeId, handleId, before: treeState.captureControl(nodeId, handleId) };
  };
  const flushUndo = () => {
    if (!pending) return;
    const after = treeState.captureControl(pending.nodeId, pending.handleId);
    treeState.recordControlChange(pending.nodeId, pending.handleId, pending.before, after);
    pending = null;
  };

  function toControlValue(kind: RenderedControl['kind'], value: any): ControlValue {
    switch (kind) {
      case 'float':
        return { kind: 'float', value: value as number };
      case 'int':
        return { kind: 'int', value: Math.round(value as number) };
      case 'bool':
        return { kind: 'bool', value: !!value };
      case 'color':
        return { kind: 'color', value: value as [number, number, number] };
      case 'select':
        return { kind: 'select', value: value as string };
      case 'spline':
        // Splines never route through the ControlPanel; edits flow via the viewport overlay.
        return { kind: 'spline', value: value as [number, number, number][] };
      case 'ramp':
        return { kind: 'ramp', value: value as RampSpecJson };
      case 'image_levels':
        return { kind: 'image_levels', value: value as ImageLevelsJson };
    }
  }

  const top = $derived(topLeftOffset(TopLeftSlot.controls));

  // Ramp/levels edits bypass the ControlPanel's bind, so mirror them into panelState so
  // their sections re-render optimistically between runs.
  const onSectionChange = (key: string, value: RampSpecJson | ImageLevelsJson) => {
    panelState = { ...panelState, [key]: value };
    handleChange(key, value);
  };
</script>

<div class="controls-panel" style:top="{top}px" style:max-height="calc(100vh - {top}px - 8px)">
  {#if settings.length > 0}
    <ControlPanel {settings} bind:state={panelState} onChange={handleChange} title="inputs" width={260} />
  {/if}
  <RampControlsSection
    {controls}
    getSpec={key => (panelState[key] as RampSpecJson | undefined) ?? null}
    onChange={onSectionChange}
    actions={actionsFor}
  />
  <ImageLevelsSection
    {controls}
    getValue={key => (panelState[key] as ImageLevelsJson | undefined) ?? null}
    onChange={onSectionChange}
    actions={actionsFor}
  />
  {#if spline}
    <SplineControlsSection {controls} {spline} actions={actionsFor} />
  {/if}
</div>

<style>
  .controls-panel {
    position: fixed;
    left: 8px;
    z-index: 6;
    overflow-y: auto;
    box-shadow: 0 2px 10px rgba(0, 0, 0, 0.5);
  }
</style>
