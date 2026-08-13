<script lang="ts">
  import type { StatusMetric } from 'src/geotoy/modes/mode';

  let { err, metrics }: { err: string | null; metrics: StatusMetric[] | null } = $props();
</script>

<div class="run-output">
  {#if err}
    <div class="error">{err}</div>
  {/if}
  {#if metrics}
    <ul>
      {#each metrics as m (m.label)}
        <li>
          <span class="label">{m.label}</span>
          <span class="value">{m.value}</span>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .run-output {
    display: flex;
    flex-direction: column;
    max-height: 260px;
    overflow-y: auto;
    overflow-x: hidden;
    background: #141414;
    border: 1px solid #444;
    border-bottom: none;
  }

  .error {
    color: #f44;
    font-size: 12px;
    padding: 8px;
    white-space: pre-wrap;
    overflow-wrap: break-word;
    font-family: inherit;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 6px 8px;
  }

  li {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    font-size: 11px;
    line-height: 1.6;
  }

  .label {
    color: #888;
  }

  .value {
    color: #ddd;
  }
</style>
