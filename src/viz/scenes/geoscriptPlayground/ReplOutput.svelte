<script lang="ts">
  import type { RunStats } from './types';
  import { IntFormatter } from './types';

  let { err, runStats }: { err: string | null; runStats: RunStats | null } = $props();
</script>

{#if err}
  <div class="error">{err}</div>
{/if}
{#if runStats}
  <div class="run-stats">
    <span class="ran-successfully" style="color: #12cc12">Program ran successfully</span>
    <ul>
      <li>Runtime: {runStats.runtimeMs.toFixed(2)} ms</li>
      {#if runStats.renderedMeshCount > 0 || runStats.renderedPathCount === 0}
        <li>Rendered Meshes: {IntFormatter.format(runStats.renderedMeshCount)}</li>
      {/if}
      {#if runStats.renderedPathCount > 0}
        <li>Rendered Paths: {IntFormatter.format(runStats.renderedPathCount)}</li>
      {/if}
      <li>Total Vertices: {IntFormatter.format(runStats.totalVtxCount)}</li>
      <li>Total Faces: {IntFormatter.format(runStats.totalFaceCount)}</li>
    </ul>
  </div>
{/if}

<style>
  .error {
    color: red;
    background: #222;
    padding: 16px 8px;
    margin-top: 8px;
    overflow-y: auto;
    overflow-x: hidden;
    max-height: 200px;
    white-space: pre-wrap;
    overflow-wrap: break-word;
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
  }

  .run-stats {
    margin-top: 8px;
    padding: 8px;
  }

  .ran-successfully {
    font-size: 15px;
  }

  li {
    font-size: 15px;
    line-height: 1.4;
  }

  ul {
    padding-left: 24px;
    margin-top: 16px;
  }

  @media (max-width: 600px) {
    .error {
      font-size: 12px;
      padding: 4px 4px;
      max-height: 150px;
      margin-top: 4px;
    }

    .run-stats {
      padding: 2px 0;
      margin-top: -2px;

      .ran-successfully {
        font-size: 12px;
      }

      ul {
        display: none;
      }
    }
  }
</style>
