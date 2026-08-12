import * as THREE from 'three';
import { mount } from 'svelte';

import type { Viz } from 'src/viz';
import type { SceneConfig } from '..';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import GeotoyApp from 'src/geotoy/GeotoyApp.svelte';
import { buildMeshPipeline } from 'src/geotoy/modes/mesh/pipeline';
import type { Composition, CompositionVersion, User } from 'src/geoscript/geotoyAPIClient';
import type { MaterialOverrideMode, GeotoyRenderHarnessCtx } from './types';
import { buildGeotoyKeymap } from './keymap';
import { WorkerManager } from 'src/geoscript/workerManager';
import type { EvalRequest } from './evalResult';

const locations = {
  spawn: {
    pos: new THREE.Vector3(48.17740050559579, 23.920086905508146, 8.603910511800485),
    rot: new THREE.Vector3(-0.022, 1.488, 0),
  },
};

export interface GeoscriptPlaygroundUserData {
  workerManager: WorkerManager | null;
  initialComposition: { comp: Composition; version: CompositionVersion } | null;
  renderMode?: boolean;
  /** Transient render only: auto-frame the camera to fit all rendered geometry before capturing. */
  transientAutoFrame?: boolean;
  /** Transient render only: swap all meshes to a debug material (normal / wireframe) before capturing. */
  renderMaterialOverride?: MaterialOverrideMode;
  /** Transient render only: fail the render (`window.onRenderError`) on a run error instead of
   *  capturing a blank frame, so the CLI reports the geoscript error / wasm panic. */
  failRenderOnError?: boolean;
  /** `geotoy eval`: serialize the run's outputs to JSON (`window.onEvalReady`) instead of rendering. */
  evalRequest?: EvalRequest;
  me?: User | null | undefined;
}

export const processLoadedScene = async (
  viz: Viz,
  _loadedWorld: THREE.Group,
  vizConf: VizConfig,
  userData: GeoscriptPlaygroundUserData | undefined = undefined
): Promise<SceneConfig> => {
  const workerManager: WorkerManager = userData?.workerManager ?? (await new WorkerManager());

  const quality = userData?.renderMode ? GraphicsQuality.High : vizConf.graphics.quality;

  let ctx = $state<GeotoyRenderHarnessCtx | null>(null);

  const pipelineController = buildMeshPipeline(viz, quality, userData?.renderMode ?? false);

  if (userData?.renderMode) {
    let didRender = false;
    const fail = (msg: string) => {
      didRender = true;
      (window as any).onRenderError?.(msg);
    };

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

    viz.setRenderOverride(timeDiffSeconds => {
      const outcome = ctx?.getLastRunOutcome();
      if (!outcome || didRender) {
        return;
      }

      // A run error (geoscript error or wasm panic) yields no geometry; surface it to the
      // CLI instead of capturing a blank frame / empty eval. Gated to transient renders so
      // the prod thumbnail path still tolerates broken saved compositions.
      if (outcome.type === 'err' && (userData.failRenderOnError || userData.evalRequest)) {
        fail(outcome.err ?? 'Geoscript run failed');
        return;
      }

      if (userData.evalRequest) {
        didRender = true;
        ctx!
          .buildEvalResultJson(userData.evalRequest)
          .then(json => {
            (window as any).__geotoyEvalResult = json;
            (window as any).onEvalReady?.(json);
          })
          .catch(err => (window as any).onRenderError?.(err instanceof Error ? err.message : String(err)));
        return;
      }

      if (!ctx?.getAreAllMaterialsLoaded()) {
        return;
      }

      if (userData.transientAutoFrame) {
        ctx.autoFrameForRender();
      }

      if (userData.renderMaterialOverride) {
        ({
          normal: ctx.toggleNormalMat,
          wireframe: ctx.toggleWireframe,
          'wireframe-xray': ctx.toggleWireframeXray,
        })[userData.renderMaterialOverride]();
      }

      viz.renderer.shadowMap.needsUpdate = true;
      viz.scene.traverse(o => {
        if (o instanceof THREE.DirectionalLight && o.castShadow) {
          o.shadow.needsUpdate = true;
        }
      });
      pipelineController.renderFrame(timeDiffSeconds);
      if (shaderErrors.length) {
        const joined = shaderErrors.join('\n\n');
        fail(joined.length > 8192 ? `${joined.slice(0, 8192)}\n… (truncated)` : joined);
        return;
      }
      didRender = true;
      (window as any).onRenderReady?.();
    }, false);
  }

  if (!userData?.renderMode && localStorage.getItem('geoscript-axis-helpers') !== 'false') {
    const axisHelper = new THREE.AxesHelper(100);
    axisHelper.position.set(0, 0, 0);
    viz.scene.add(axisHelper);
  }

  mount(GeotoyApp, {
    target: document.getElementById('viz-container')!,
    props: {
      viz,
      workerManager,
      setHarnessCtx: (newCtx: GeotoyRenderHarnessCtx) => {
        ctx = newCtx;
      },
      userData,
      pipelineController,
    },
  });

  return {
    locations,
    spawnLocation: 'spawn',
    viewMode: {
      type: 'orbit',
      pos: new THREE.Vector3(10, 10, 10),
      target: new THREE.Vector3(0, 0, 0),
    },
    // Label-only (argless → no-op actions) for the PauseMenu's shortcut listing;
    // dispatch is handled by the geotoy keymap module.
    customControlsEntries: buildGeotoyKeymap(),
  };
};
