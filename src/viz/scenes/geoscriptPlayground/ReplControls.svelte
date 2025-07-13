<script lang="ts">
  import ActionsMenu from './ActionsMenu.svelte';

  let {
    isRunning,
    isEditorCollapsed = false,
    run,
    toggleEditorCollapsed,
    goHome,
    err,
    onExport,
    toggleMaterialEditorOpen,
  }: {
    isRunning: boolean;
    isEditorCollapsed?: boolean;
    run: () => void;
    toggleEditorCollapsed: () => void;
    goHome: () => void;
    err: string | null;
    onExport: () => void;
    toggleMaterialEditorOpen: () => void;
  } = $props();
</script>

<div class="run-controls" class:collapsed={isEditorCollapsed}>
  <button class={{ collapsed: isEditorCollapsed }} disabled={isRunning} onclick={run}>
    {#if isRunning}running...{:else}run{/if}
  </button>
  <button
    class={['show-code-btn', isEditorCollapsed ? 'collapsed' : undefined]}
    onclick={toggleEditorCollapsed}
    style="min-width: 128px"
  >
    {#if isEditorCollapsed}show editor{:else}hide editor{/if}
  </button>
  {#if isEditorCollapsed && err}
    <div class="error">error; open editor for details</div>
  {/if}
  <div class="right-controls">
    <button class={['home-button', isEditorCollapsed ? 'collapsed' : undefined]} onclick={goHome}>
      home
    </button>
    <ActionsMenu>
      <button onclick={toggleMaterialEditorOpen}>edit materials</button>
      <button onclick={onExport}>export scene</button>
    </ActionsMenu>
  </div>
</div>

<style>
  button {
    padding: 4px 8px;
  }

  :global(.menu button) {
    width: 100%;
    text-align: left;
    padding: 8px 12px;
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
  }

  .right-controls {
    margin-left: auto;
    display: flex;
    align-items: center;
  }
</style>
