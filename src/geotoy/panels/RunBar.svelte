<script lang="ts">
  import type { StatusMetric } from 'src/geotoy/modes/mode';

  let {
    isRunning,
    err,
    metrics,
    isDirty,
    expanded,
    recordingState,
    run,
    cancel,
    onToggleExpanded,
    onToggleSave,
    saveOpen,
    compactMetrics = false,
  }: {
    isRunning: boolean;
    err: string | null;
    metrics: StatusMetric[] | null;
    isDirty: boolean;
    expanded: boolean;
    recordingState: 'recording' | 'initializing' | 'not-recording';
    run: () => void;
    cancel: () => void;
    onToggleExpanded: () => void;
    onToggleSave: () => void;
    saveOpen: boolean;
    /** Narrow bars show only runtime + primary count; the rest is one tap away in the
     *  expansion. Clipping the full set mid-word reads as broken. */
    compactMetrics?: boolean;
  } = $props();

  const status = $derived(isRunning ? 'running' : err ? 'error' : metrics ? 'ok' : 'idle');
  const shownMetrics = $derived(metrics && (compactMetrics ? metrics.slice(0, 2) : metrics));
</script>

<button class="run" disabled={isRunning} onclick={run}>run</button>
{#if isRunning}
  <button class="cancel" onclick={cancel}>cancel</button>
{/if}

<button
  class="disclosure"
  aria-expanded={expanded}
  title={expanded ? 'collapse run output' : 'expand run output'}
  onclick={onToggleExpanded}
>
  {expanded ? '▾' : '▸'}
</button>
<span class={['status', status]}>{status}</span>
{#if !err && shownMetrics}
  <span class="metrics">
    {#each shownMetrics as m, i (m.label)}
      {#if i > 0}<span class="sep">·</span>{/if}
      <span>{m.short}</span>
    {/each}
  </span>
{/if}

<span class="tail">
  {#if recordingState !== 'not-recording'}
    <span
      class="recording"
      class:initializing={recordingState === 'initializing'}
      title={recordingState === 'recording' ? 'video recording in progress' : 'initializing video recording'}
    >
      🔴
    </span>
  {/if}
  {#if isDirty}
    <span class="dirty" title="unsaved changes">&#10033;</span>
  {/if}
  <button class="save" class:open={saveOpen} onclick={onToggleSave}>save</button>
</span>

<style>
  .run {
    background: #333;
    color: #f0f0f0;
    border: 1px solid #888;
    padding: 3px 12px;
    font-size: 12px;
    font-family: inherit;
    cursor: pointer;
    flex-shrink: 0;
  }

  .run:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .cancel {
    background: #2a2a2a;
    color: #ddd;
    border: 1px solid #777;
    padding: 3px 10px;
    font-size: 12px;
    font-family: inherit;
    cursor: pointer;
    flex-shrink: 0;
  }

  .disclosure {
    background: none;
    border: none;
    color: #888;
    font-size: 13px;
    padding: 2px 5px;
    margin: 0 -3px;
    cursor: pointer;
    flex-shrink: 0;
    line-height: 1;
  }

  .disclosure:hover {
    color: #eee;
  }

  .status {
    font-size: 11px;
    flex-shrink: 0;
  }

  .status.ok {
    color: #12cc12;
  }

  .status.error {
    color: #f44;
  }

  .status.running,
  .status.idle {
    color: #888;
  }

  /* The one shrinkable region of the bar: run/status/save all keep their size. */
  .metrics {
    display: flex;
    align-items: center;
    gap: 5px;
    flex: 0 1 auto;
    font-size: 11px;
    color: #888;
    overflow: hidden;
    white-space: nowrap;
    min-width: 0;
  }

  .sep {
    color: #444;
  }

  /* `margin-left: auto` here is what right-packs the whole tail of the bar. */
  .tail {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-left: auto;
    flex-shrink: 0;
  }

  .save {
    background: #2a2a2a;
    color: #eee;
    border: 1px solid #777;
    padding: 3px 14px;
    font-size: 12px;
    font-family: inherit;
    cursor: pointer;
  }

  .save:hover,
  .save.open {
    background: #333;
    border-color: #888;
  }

  .dirty {
    color: #f44;
    font-size: 12px;
    line-height: 1;
  }

  .recording {
    font-size: 10px;
  }

  .recording.initializing {
    filter: grayscale(100%) opacity(0.5);
  }
</style>
