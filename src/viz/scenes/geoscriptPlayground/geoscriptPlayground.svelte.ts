import * as THREE from 'three';
import { N8AOPostPass } from 'n8ao';
import { mount } from 'svelte';

import type { Viz } from 'src/viz';
import type { SceneConfig } from '..';
import {
  configureDefaultPostprocessingPipeline,
  type PostprocessingPipelineController,
} from 'src/viz/postprocessing/defaultPostprocessing';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import ReplUi from './ReplUI.svelte';
import type { Composition, CompositionVersion, User } from 'src/geoscript/geotoyAPIClient';
import type { MaterialOverrideMode, ReplCtx } from './types';
import { buildGeotoyKeymap } from './keymap';
import { WorkerManager } from 'src/geoscript/workerManager';
import type { EvalRequest } from './evalResult';

const locations = {
  spawn: {
    pos: new THREE.Vector3(48.17740050559579, 23.920086905508146, 8.603910511800485),
    rot: new THREE.Vector3(-0.022, 1.488, 0),
  },
};

const initRepl = async (
  viz: Viz,
  workerManager: WorkerManager,
  setReplCtx: (ctx: ReplCtx) => void,
  userData: GeoscriptPlaygroundUserData | undefined = undefined,
  onSizeChange: (size: number, isCollapsed: boolean, orientation: 'vertical' | 'horizontal') => void,
  pipelineController: PostprocessingPipelineController | null
) => {
  mount(ReplUi, {
    target: document.getElementById('viz-container')!,
    props: {
      viz,
      workerManager,
      setReplCtx,
      userData,
      onSizeChange,
      pipelineController,
    },
  });
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

  let ctx = $state<ReplCtx | null>(null);

  let pipelineController: PostprocessingPipelineController | null = configureDefaultPostprocessingPipeline({
    viz,
    quality,
    addMiddlePasses: (composer, viz, _quality) => {
      if (quality > GraphicsQuality.Low && (window.innerWidth > 800 || userData?.renderMode)) {
        const n8aoPass = new N8AOPostPass(
          viz.scene,
          viz.camera,
          viz.renderer.domElement.width,
          viz.renderer.domElement.height
        );
        composer.addPass(n8aoPass);
        n8aoPass.gammaCorrection = false;
        n8aoPass.configuration.intensity = 2;
        n8aoPass.configuration.aoRadius = 5;
        n8aoPass.configuration.halfRes = quality <= GraphicsQuality.Medium;
        n8aoPass.setQualityMode(
          {
            [GraphicsQuality.Low]: 'Performance',
            [GraphicsQuality.Medium]: 'Low',
            [GraphicsQuality.High]: 'Medium',
          }[quality]
        );
      }
    },
    autoUpdateShadowMap: !userData?.renderMode,
    toneMapping: { mode: 'neutral', exposure: 1 },
    pomExitBuffers: true,
  });

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
      pipelineController?.renderFrame(timeDiffSeconds);
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

  let layoutOrientation = $state<'vertical' | 'horizontal'>(
    (localStorage.getItem('geoscriptLayoutOrientation') as 'vertical' | 'horizontal') || 'vertical'
  );
  let controlsSize = $state(
    layoutOrientation === 'horizontal'
      ? Number(localStorage.getItem('geoscript-repl-width')) || Math.max(400, 0.35 * window.innerWidth)
      : Number(localStorage.getItem('geoscript-repl-height')) || Math.max(250, 0.25 * window.innerHeight)
  );
  let isEditorCollapsed = $state(window.innerWidth < 768);

  const updateCanvasSize = () => {
    if (userData?.renderMode) {
      return;
    }

    let canvasWidth: number;
    let canvasHeight: number;
    if (layoutOrientation === 'horizontal') {
      const newWidth = isEditorCollapsed ? 36 : controlsSize;
      canvasWidth = Math.max(window.innerWidth - newWidth, 0);
      canvasHeight = window.innerHeight;
    } else {
      const newHeight = isEditorCollapsed ? 36 : controlsSize;
      canvasWidth = window.innerWidth;
      canvasHeight = Math.max(window.innerHeight - newHeight, 0);
    }

    if (pipelineController) {
      pipelineController.effectComposer.setSize(canvasWidth, canvasHeight, true);
    } else {
      viz.renderer.setSize(canvasWidth, canvasHeight, true);
    }

    if (viz.camera instanceof THREE.PerspectiveCamera) {
      viz.camera.aspect = canvasWidth / canvasHeight;
    } else if (viz.camera instanceof THREE.OrthographicCamera) {
      const halfH = (viz.camera.top - viz.camera.bottom) / 2;
      const aspect = canvasHeight > 0 ? canvasWidth / canvasHeight : 1;
      viz.camera.left = -halfH * aspect;
      viz.camera.right = halfH * aspect;
    }
    viz.camera.updateProjectionMatrix();
  };
  viz.registerResizeCb(updateCanvasSize);
  updateCanvasSize();

  await initRepl(
    viz,
    workerManager,
    (newCtx: ReplCtx) => {
      ctx = newCtx;
    },
    userData,
    (newSize: number, newIsCollapsed: boolean, newOrientation: 'vertical' | 'horizontal') => {
      controlsSize = newSize;
      isEditorCollapsed = newIsCollapsed;
      layoutOrientation = newOrientation;
      updateCanvasSize();
    },
    pipelineController
  );

  return {
    locations,
    spawnLocation: 'spawn',
    viewMode: {
      type: 'orbit',
      pos: new THREE.Vector3(10, 10, 10),
      target: new THREE.Vector3(0, 0, 0),
    },
    customControlsEntries: buildGeotoyKeymap(() => ctx),
  };
};
