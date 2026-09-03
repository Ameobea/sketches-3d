import * as THREE from 'three';

import type { Viz } from 'src/viz';
import type { PostprocessingPipelineController } from 'src/viz/postprocessing/defaultPostprocessing';
import type { GeoscriptPlaygroundUserData } from 'src/viz/scenes/geoscriptPlayground/geoscriptPlayground.svelte';
import { buildEvalResultJson } from 'src/geotoy/modes/mesh/evalResult';
import type { TreeDef } from 'src/geoscript/geotoyAPIClient';
import type { GeoscriptExecution, RunOutcome } from 'src/geotoy/modules/execution.svelte';
import type { MeshScene } from 'src/geotoy/modes/mesh/meshScene.svelte';
import type { ConstEvalCacheStats, RunPhases } from 'src/geoscript/runner/types';

export interface BenchRequest {
  /** Timed iterations, after the boot run and warmup. */
  iterations: number;
  /** Untimed iterations after the boot run (JIT warmup; the boot run already loaded async deps). */
  warmup: number;
  /** `cold` clears every cross-run cache (const-eval, module exports, Clipper2 memos) before each run. */
  mode: 'cold' | 'warm';
  /** Also wait for materials and render one frame after each timed run. */
  render: boolean;
}

interface BenchSample {
  wall: number;
  phases: RunPhases;
  asyncDepRetries: number;
  constEvalCache: ConstEvalCacheStats;
  materials?: number;
  frame?: number;
}

interface RenderHarnessDeps {
  viz: Viz;
  pipelineController: PostprocessingPipelineController;
  userData: GeoscriptPlaygroundUserData;
  execution: GeoscriptExecution<any>;
  meshScene: MeshScene;
  getTree: () => TreeDef;
  getTabs: () => { id: string; kind: string; name: string }[];
}

/**
 * Headless render/eval driver (`?render=true`: thumbnail generator + geotoy_cli).
 * Awaits the boot run and material loads, stages the frame (auto-frame / debug material
 * override), renders exactly one frame, and reports through the window contract:
 * `onRenderReady` / `onRenderError` / `onEvalReady` + `__geotoyEvalResult`. A bench request
 * instead re-runs the composition in place and reports timings via `onBenchReady`.
 */
export const startRenderHarness = ({
  viz,
  pipelineController,
  userData,
  execution,
  meshScene,
  getTree,
  getTabs,
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

  /** Re-runs the booted composition inside this page so the wasm instance, JIT tiers, and
   *  loaded async deps carry over; `performance` marks delimit each run for the trace. */
  const runBench = async (req: BenchRequest, boot: RunOutcome<any> & { type: 'ok' }) => {
    const bootMs = performance.now();
    const sample = async (label: string): Promise<BenchSample> => {
      performance.mark(`${label}:start`);
      const t0 = performance.now();
      const o = req.mode === 'cold' ? await execution.runUncached() : await execution.run();
      const wall = performance.now() - t0;
      performance.mark(`${label}:end`);
      performance.measure(label, `${label}:start`, `${label}:end`);
      if (!o || o.type !== 'ok') {
        throw new Error(o?.type === 'err' ? o.err : 'run did not settle');
      }
      const s = o.result.stats;
      const out: BenchSample = {
        wall,
        phases: s.phases,
        asyncDepRetries: s.asyncDepRetries,
        constEvalCache: s.constEvalCache,
      };
      if (req.render) {
        let t = performance.now();
        await meshScene.materialRuntime.untilAllLoaded();
        out.materials = performance.now() - t;
        t = performance.now();
        await renderOneFrame();
        out.frame = performance.now() - t;
      }
      return out;
    };

    const warmupRuns: BenchSample[] = [];
    for (let i = 0; i < req.warmup; i++) {
      warmupRuns.push(await sample(`bench:warmup:${i}`));
    }
    // The render service starts CDP tracing here so the trace covers only timed runs.
    await (window as any).onBenchPhase?.('timed');
    const runs: BenchSample[] = [];
    for (let i = 0; i < req.iterations; i++) {
      runs.push(await sample(`bench:timed:${i}`));
    }

    const tabs = getTabs();
    const tabName = (id: string) => tabs.find(t => t.id === id)?.name ?? id;
    const bs = boot.result.stats;
    return {
      ok: true,
      mode: req.mode,
      iterations: req.iterations,
      warmup: req.warmup,
      render: req.render,
      activeTab: tabName(boot.input.tabId),
      activeTabKind: tabs.find(t => t.id === boot.input.tabId)?.kind ?? null,
      tabsRun: (boot.input.runTabIds as string[]).map(tabName),
      // Navigation start → boot run settled (page load, wasm compile, deps, first cold run).
      bootMs,
      boot: { phases: bs.phases, asyncDepRetries: bs.asyncDepRetries, constEvalCache: bs.constEvalCache },
      warmupRuns,
      runs,
      stats: {
        meshes: bs.renderedMeshCount,
        paths: bs.renderedPathCount,
        lights: bs.renderedLightCount,
        textures: bs.renderedTextureCount,
        vertices: bs.totalVtxCount,
        faces: bs.totalFaceCount,
      },
      asyncDeps: bs.asyncDeps,
      userAgent: navigator.userAgent,
    };
  };

  void (async () => {
    const outcome = await execution.nextSettled();

    // A run error (geoscript error or wasm panic) yields no geometry; surface it to the
    // CLI instead of capturing a blank frame / empty eval. Gated to transient renders so
    // the prod thumbnail path still tolerates broken saved compositions.
    if (
      outcome.type !== 'ok' &&
      (userData.failRenderOnError || userData.evalRequest || userData.benchRequest)
    ) {
      fail(outcome.type === 'err' ? outcome.err : 'Execution interrupted');
      return;
    }

    if (userData.benchRequest && outcome.type === 'ok') {
      try {
        const json = JSON.stringify(await runBench(userData.benchRequest, outcome));
        (window as any).__geotoyBenchResult = json;
        (window as any).onBenchReady?.(json);
      } catch (err) {
        fail(err instanceof Error ? err.message : String(err));
      }
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
    await meshScene.settleView();

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
