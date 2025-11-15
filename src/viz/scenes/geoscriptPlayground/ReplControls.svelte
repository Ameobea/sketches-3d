<script lang="ts">
  import ActionsMenu from './ActionsMenu.svelte';
  import type { RecordingState } from './recording';

  let {
    isRunning,
    isEditorCollapsed = false,
    run,
    cancel,
    toggleEditorCollapsed,
    goHome,
    err,
    onExport,
    onRecord,
    recordingState,
    clearLocalChanges,
    toggleAxisHelpers,
    toggleLightHelpers,
    toggleMaterialEditorOpen,
    isDirty,
    togglePreludeEjected,
    preludeEjected,
  }: {
    isRunning: boolean;
    isEditorCollapsed?: boolean;
    run: () => void;
    cancel: () => void;
    toggleEditorCollapsed: () => void;
    goHome: () => void;
    err: string | null;
    onExport: () => void;
    onRecord: () => void;
    recordingState: RecordingState;
    clearLocalChanges: () => void;
    toggleAxisHelpers: () => void;
    toggleLightHelpers: () => void;
    toggleMaterialEditorOpen: () => void;
    isDirty: boolean;
    togglePreludeEjected: () => void;
    preludeEjected: boolean;
  } = $props();
</script>

<div class="run-controls" class:collapsed={isEditorCollapsed}>
  <button class={{ collapsed: isEditorCollapsed }} disabled={isRunning} onclick={run}>
    {#if isRunning}running...{:else}run{/if}
  </button>
  {#if isRunning}
    <button class={{ collapsed: isEditorCollapsed }} onclick={cancel} style="margin-left: 8px">cancel</button>
  {/if}
  {#if recordingState === 'recording'}
    <span class="recording-indicator" title="video recording in progress">🔴</span>
  {:else if recordingState === 'initializing'}
    <span
      class="recording-indicator"
      title="initializing video recording"
      style="filter: grayscale(100%) opacity(0.5);"
    >
      🔴
    </span>
  {/if}
  <button
    class={['show-code-btn', isEditorCollapsed ? 'collapsed' : undefined]}
    onclick={toggleEditorCollapsed}
    style="min-width: 128px"
  >
    {#if isEditorCollapsed}show editor{:else}hide editor{/if}
  </button>
  {#if isDirty && !err}
    <span class="dirty" title="unsaved changes">*</span>
  {/if}
  {#if isEditorCollapsed && err}
    <div class="error">error; open editor for details</div>
  {/if}
  <div class="right-controls">
    <button class={['home-button', isEditorCollapsed ? 'collapsed' : undefined]} onclick={goHome}>
      home
    </button>
    {#if !isEditorCollapsed}
      <ActionsMenu>
        <button onclick={toggleMaterialEditorOpen}>edit materials</button>
        <button onclick={onExport}>export scene</button>
        <button onclick={onRecord}>
          {{ initializing: 'initializing', 'not-recording': 'start recording', recording: 'stop-recording' }[
            recordingState
          ]}
        </button>
        <button onclick={clearLocalChanges}>clear local changes</button>
        <button onclick={toggleAxisHelpers}>toggle axis helpers</button>
        <button onclick={toggleLightHelpers}>toggle light helpers</button>
        <button onclick={togglePreludeEjected}>{preludeEjected ? 'un-eject' : 'eject'} prelude</button>
        <button onclick={() => void window.open('/geotoy/docs', '_blank')}>open docs</button>
        <button
          onclick={() => void window.open('https://github.com/Ameobea/sketches-3d/issues/new', '_blank')}
        >
          report bug
        </button>
      </ActionsMenu>
    {/if}
  </div>
</div>

<style>
  button {
    padding: 4px 8px;
  }

  :global(.menu button) {
    width: 100%;
    text-align: left;
    padding: 6px 8px;
    font-size: 14px;
  }

  button.collapsed {
    font-size: 14px;
    padding: 8px 16px;
  }

  .run-controls {
    display: flex;
    gap: 0px;
    align-items: center;
  }

  .dirty {
    color: red;
    font-size: 12px;
    margin-left: 8px;
    margin-right: 8px;
    line-height: 0;
  }

  .error {
    color: red;
    font-size: 12px;
    line-height: 1;
    padding: 0 2px;
  }

  .run-controls:not(.collapsed) button:first-child {
    min-width: 100px;
  }

  .show-code-btn {
    display: none;
  }

  .collapsed .show-code-btn {
    display: block;
  }

  @media (max-width: 768px) {
    .show-code-btn {
      display: block;
    }

    .collapsed {
      max-height: 40px;
    }

    :global(.menu button) {
      font-size: 12px;
      padding: 3px 6px;
    }

    .run-controls:not(.collapsed) button:first-child {
      min-width: 80px;
    }
  }

  .right-controls {
    margin-left: auto;
    display: flex;
    align-items: center;
  }

  .recording-indicator {
    margin-left: 8px;
  }
</style>
