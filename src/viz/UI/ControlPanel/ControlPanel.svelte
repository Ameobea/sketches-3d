<script lang="ts">
  import './theme.css';
  import type { ControlPanelSetting, ControlPanelState } from './types';
  import Range from './controls/Range.svelte';
  import NumberInput from './controls/Number.svelte';
  import Checkbox from './controls/Checkbox.svelte';
  import Select from './controls/Select.svelte';
  import Color from './controls/Color.svelte';
  import Text from './controls/Text.svelte';
  import Button from './controls/Button.svelte';

  interface Props {
    settings: ControlPanelSetting[];
    state: ControlPanelState;
    onChange?: (label: string, value: any, fullState: ControlPanelState) => void;
    title?: string;
    width?: number;
  }

  let { settings, state = $bindable(), onChange, title, width = 300 }: Props = $props();

  const keyOf = (s: ControlPanelSetting) => s.key ?? s.label;

  // Seed state keys that only exist as per-setting `initial` defaults.
  let seeded = false;
  $effect(() => {
    if (seeded) return;
    seeded = true;
    const patch: ControlPanelState = {};
    for (const s of settings) {
      if (s.type === 'button' || keyOf(s) in state) continue;
      if (s.initial !== undefined) patch[keyOf(s)] = s.initial;
    }
    if (Object.keys(patch).length) state = { ...state, ...patch };
  });

  const commit = (label: string, value: any) => {
    state = { ...state, [label]: value };
    onChange?.(label, value, state);
  };
</script>

<div class="control-panel" style:width="{width}px">
  {#if title}
    <div class="cp-title">{title}</div>
  {/if}
  {#each settings as setting (keyOf(setting))}
    {@const k = keyOf(setting)}
    <div class="cp-row">
      {#if setting.type === 'button'}
        <Button {setting} />
      {:else}
        <span class="cp-label" title={setting.label}>{setting.label}</span>
        <div class="cp-control">
          {#if setting.type === 'range'}
            <Range {setting} value={state[k]} onChange={v => commit(k, v)} />
          {:else if setting.type === 'number'}
            <NumberInput {setting} value={state[k]} onChange={v => commit(k, v)} />
          {:else if setting.type === 'checkbox'}
            <Checkbox value={state[k]} onChange={v => commit(k, v)} />
          {:else if setting.type === 'select'}
            <Select {setting} value={state[k]} onChange={v => commit(k, v)} />
          {:else if setting.type === 'color'}
            <Color value={state[k]} onChange={v => commit(k, v)} />
          {:else if setting.type === 'text'}
            <Text value={state[k]} onChange={v => commit(k, v)} />
          {/if}
        </div>
      {/if}
    </div>
  {/each}
</div>

<style>
  .control-panel {
    display: inline-block;
    padding: 10px 14px 8px;
    background: var(--cp-bg1);
  }

  .cp-title {
    text-align: center;
    text-transform: uppercase;
    color: var(--cp-text2);
    margin-bottom: 6px;
  }

  .cp-row {
    display: flex;
    align-items: center;
    height: 25px;
    gap: 2%;
  }

  .cp-label {
    flex: 0 0 36%;
    color: var(--cp-text1);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .cp-control {
    flex: 1 1 60%;
    display: flex;
    align-items: center;
    min-width: 0;
  }
</style>
