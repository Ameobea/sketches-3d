<script lang="ts">
  import type { TextureMode } from 'src/geotoy/modes/texture/textureMode.svelte';
  import { PROBLEM_TEXT } from 'src/geotoy/modes/texture/previewTarget';

  let { mode, onPick, onShow2d }: { mode: TextureMode; onPick: () => void; onShow2d: () => void } = $props();

  const note = $derived(
    mode.previewProblem ? PROBLEM_TEXT[mode.previewProblem] : (mode.previewMaterialWarning ?? null)
  );
</script>

<div class="hud panel">
  <div class="row">
    <span class="title">3d preview</span>
    <span class="target">{mode.previewTargetLabel}</span>
  </div>
  <div class="row chips">
    <button class="chip" onclick={onPick}>pick object…</button>
    <button class="chip" onclick={mode.previewScene.focus}>center</button>
    <button class="chip" onclick={onShow2d} title="P">2d</button>
  </div>
  {#if note}
    <span class="note" class:problem={!!mode.previewProblem}>{note}</span>
  {/if}
</div>

<style>
  .hud {
    position: fixed;
    top: 6px;
    left: 6px;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    padding: 5px;
    user-select: none;
    max-width: 360px;
  }

  .panel {
    background: rgba(13, 13, 13, 0.9);
    border: 1px solid #2e2e2e;
  }

  .row {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 2px 3px;
  }

  .title {
    font-size: 11px;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: #0ff;
  }

  .target {
    font-size: 11px;
    color: #ddd;
  }

  .chips {
    gap: 2px;
    padding: 0;
  }

  .chip {
    background: #141414;
    border: 1px solid #2e2e2e;
    color: #888;
    font-size: 13px;
    font-family: inherit;
    padding: 3px 8px 4px 8px;
    cursor: pointer;
    line-height: 1;
  }

  .chip:hover {
    background: #1e1e1e;
    color: #fff;
  }

  .note {
    font-size: 11px;
    color: #999;
    line-height: 1.3;
    padding: 2px 3px;
  }

  .note.problem {
    color: #f66;
  }
</style>
