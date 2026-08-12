<script lang="ts">
  import type { Viz } from 'src/viz';
  import type { PostprocessingPipelineController } from 'src/viz/postprocessing/defaultPostprocessing';
  import type { WorkerManager } from 'src/geoscript/workerManager';
  import ReplUI from 'src/viz/scenes/geoscriptPlayground/ReplUI.svelte';
  import type { GeoscriptPlaygroundUserData } from 'src/viz/scenes/geoscriptPlayground/geoscriptPlayground.svelte';
  import type { ReplCtx } from 'src/viz/scenes/geoscriptPlayground/types';

  // Geotoy app shell (docs/geotoy-app-shell-refactor.md). Currently a pass-through host
  // for ReplUI; modules/panels/modes get peeled out of it into this directory one at a time.
  let {
    viz,
    workerManager,
    setReplCtx,
    userData,
    onSizeChange,
    pipelineController,
  }: {
    viz: Viz;
    workerManager: WorkerManager;
    setReplCtx: (ctx: ReplCtx) => void;
    userData?: GeoscriptPlaygroundUserData;
    onSizeChange: (size: number, isCollapsed: boolean, orientation: 'vertical' | 'horizontal') => void;
    pipelineController: PostprocessingPipelineController | null;
  } = $props();
</script>

<ReplUI {viz} {workerManager} {setReplCtx} {userData} {onSizeChange} {pipelineController} />
