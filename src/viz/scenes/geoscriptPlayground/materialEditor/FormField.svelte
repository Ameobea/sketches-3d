<script lang="ts">
  import type { Snippet } from 'svelte';

  let { label, help, children }: { label: string; help?: string; children: Snippet } = $props();
</script>

<div class="form-field">
  <!-- svelte-ignore a11y_label_has_associated_control -->
  <label>
    {label}
    {#if help}
      <span class="help-icon" title={help}>?</span>
    {/if}
  </label>
  <div class="control">
    {@render children?.()}
  </div>
</div>

<style>
  .form-field {
    display: grid;
    grid-template-columns: 160px 1fr;
    align-items: center;
    gap: 8px;
    margin-bottom: 12px;
  }
  label {
    text-align: right;
    font-size: 12px;
  }
  .help-icon {
    display: inline-block;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #555;
    color: #fff;
    text-align: center;
    font-size: 12px;
    line-height: 16px;
    cursor: help;
    margin-left: 4px;
  }
  .control {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  :global(.control input[type='text']),
  :global(.control select) {
    background-color: #1a1a1a;
    color: #eee;
    border: 1px solid #444;
    padding: 4px 6px;
    font-size: 12px;
    font-family: inherit;
    width: 100%;
    box-sizing: border-box;
  }

  :global(.control input[type='range']) {
    flex-grow: 1;
  }

  @media (max-width: 600px) {
    .form-field {
      grid-template-columns: 80px 1fr;
    }
  }
</style>
