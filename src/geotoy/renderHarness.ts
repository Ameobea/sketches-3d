import * as THREE from 'three';

import type { Viz } from 'src/viz';
import type { PostprocessingPipelineController } from 'src/viz/postprocessing/defaultPostprocessing';
import type { GeoscriptPlaygroundUserData } from 'src/viz/scenes/geoscriptPlayground/geoscriptPlayground.svelte';
import { buildEvalResultJson } from 'src/viz/scenes/geoscriptPlayground/evalResult';
import type { TreeDef } from 'src/geoscript/geotoyAPIClient';
import type { GeoscriptExecution } from 'src/geotoy/modules/execution.svelte';
import type { MeshScene } from 'src/geotoy/modes/mesh/meshScene.svelte';

interface RenderHarnessDeps {
  viz: Viz;
  pipelineController: PostprocessingPipelineController;
  userData: GeoscriptPlaygroundUserData;
  execution: GeoscriptExecution<any>;
  meshScene: MeshScene;
  getTree: () => TreeDef;
}

/**
 * Headless render/eval driver (`?render=true`: thumbnail generator + geotoy_cli).
 * Awaits the boot run and material loads, stages the frame (auto-frame / debug material
 * override), renders exactly one frame, and reports through the window contract:
 * `onRenderReady` / `onRenderError` / `onEvalReady` + `__geotoyEvalResult`.
 */
export const startRenderHarness = ({
  viz,
  pipelineController,
  userData,
  execution,
  meshScene,
  getTree,
}: RenderHarnessDeps) => {
  const stats = document.getElementById('viz-stats');
  if (stats) {
    stats.style.display = 'none';
  }

  // GLSL compile/link failures don't throw — three only console.error()s them (with the
  // material name + a numbered source excerpt) — so tap console.error and fail the render
  // after the first frame, which is what compiles every visible program. Gated like run
  // errors so the prod thumbnail path still tolerates broken saved materials.
  const shaderErrors: string[] = [];
  if (userData.failRenderOnError) {
    const origConsoleError = console.error.bind(console);
    console.error = (...args: unknown[]) => {
      origConsoleError(...args);
      const msg = args.map(a => String(a)).join(' ');
      if (msg.includes('THREE.WebGLProgram')) {
        shaderErrors.push(msg);
      }
    };
  }

  // Persistent per-frame override; the `false` keeps `viz.postprocessingController`
  // alive. It suppresses all rendering except the single frame requested below.
  let frameRequested: (() => void) | null = null;
  viz.setRenderOverride(timeDiffSeconds => {
    if (!frameRequested) {
      return;
    }
    const done = frameRequested;
    frameRequested = null;

    // Orbit scenes never resolve the physics startup barriers, so the pipeline's own
    // shadow update path never runs; force shadow renders for the captured frame.
    viz.renderer.shadowMap.needsUpdate = true;
    viz.scene.traverse(o => {
      if (o instanceof THREE.DirectionalLight && o.castShadow) {
        o.shadow.needsUpdate = true;
      }
    });
    pipelineController.renderFrame(timeDiffSeconds);
    done();
  }, false);
  const renderOneFrame = () => new Promise<void>(resolve => (frameRequested = resolve));

  const fail = (msg: string) => (window as any).onRenderError?.(msg);

  void (async () => {
    const outcome = await execution.nextSettled();

    // A run error (geoscript error or wasm panic) yields no geometry; surface it to the
    // CLI instead of capturing a blank frame / empty eval. Gated to transient renders so
    // the prod thumbnail path still tolerates broken saved compositions.
    if (outcome.type !== 'ok' && (userData.failRenderOnError || userData.evalRequest)) {
      fail(outcome.type === 'err' ? outcome.err : 'Execution interrupted');
      return;
    }

    if (userData.evalRequest) {
      try {
        const json = await buildEvalResultJson({
          repl: execution.repl,
          ctxPtr: execution.ctxPtr!,
          renderedObjects: meshScene.renderedObjects,
          tree: getTree(),
          stats: execution.runStats,
          req: userData.evalRequest,
        });
        (window as any).__geotoyEvalResult = json;
        (window as any).onEvalReady?.(json);
      } catch (err) {
        fail(err instanceof Error ? err.message : String(err));
      }
      return;
    }

    await meshScene.materialRuntime.untilAllLoaded();

    if (userData.transientAutoFrame) {
      meshScene.focus(null);
    }
    if (userData.renderMaterialOverride) {
      ({
        normal: meshScene.toggleNormalMat,
        wireframe: meshScene.toggleWireframe,
        'wireframe-xray': meshScene.toggleWireframeXray,
      })[userData.renderMaterialOverride]();
    }

    await renderOneFrame();

    if (shaderErrors.length) {
      const joined = shaderErrors.join('\n\n');
      fail(joined.length > 8192 ? `${joined.slice(0, 8192)}\n… (truncated)` : joined);
      return;
    }
    (window as any).onRenderReady?.();
  })();
};
