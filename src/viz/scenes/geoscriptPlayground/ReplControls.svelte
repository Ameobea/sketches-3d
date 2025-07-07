<script lang="ts">
  let {
    isRunning,
    isEditorCollapsed = false,
    run,
    toggleEditorCollapsed,
    goHome,
  }: {
    isRunning: boolean;
    isEditorCollapsed?: boolean;
    run: () => void;
    toggleEditorCollapsed: () => void;
    goHome: () => void;
  } = $props();
</script>

<div class="run-controls" class:collapsed={isEditorCollapsed}>
  <button disabled={isRunning} onclick={run}>
    {#if isRunning}running...{:else}run{/if}
  </button>
  <button class="show-code-btn" onclick={toggleEditorCollapsed}>
    {#if isEditorCollapsed}show editor{:else}hide editor{/if}
  </button>
  <button class="home-button" onclick={goHome}>home</button>
</div>

<style>
  .run-controls {
    display: flex;
    gap: 8px;
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
  }

  .home-button {
    margin-left: auto;
  }
</style>
