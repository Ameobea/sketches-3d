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
import type { ReplCtx } from './types';
import { buildGeotoyKeymap } from './keymap';
import { WorkerManager } from 'src/geoscript/workerManager';

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
  const _ui = mount(ReplUi, {
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
  me?: User | null | undefined;
}

export const processLoadedScene = async (
  viz: Viz,
  _loadedWorld: THREE.Group,
  vizConf: VizConfig,
  userData: GeoscriptPlaygroundUserData | undefined = undefined
): Promise<SceneConfig> => {
  const workerManager: WorkerManager = userData?.workerManager ?? (await new WorkerManager());

  const quality = vizConf.graphics.quality;

  let ctx = $state<ReplCtx | null>(null);

  let pipelineController: PostprocessingPipelineController | null = null;

  if (userData?.renderMode) {
    let didRender = false;
    viz.setRenderOverride(() => {
      if (!ctx?.getLastRunOutcome() || didRender || !ctx?.getAreAllMaterialsLoaded()) {
        return;
      }

      viz.renderer.render(viz.scene, viz.camera);
      didRender = true;
      (window as any).onRenderReady?.();
    });
  } else {
    pipelineController = configureDefaultPostprocessingPipeline({
      viz,
      quality,
      addMiddlePasses: (composer, viz, _quality) => {
        if (quality > GraphicsQuality.Low && window.innerWidth > 800) {
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
          // \/ this breaks rendering and makes the background black if enabled
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
      extraParams: undefined,
      postEffects: undefined,
      autoUpdateShadowMap: !userData?.renderMode,
      // enableAntiAliasing: !userData?.renderMode,
    });
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

    if (layoutOrientation === 'horizontal') {
      const newWidth = isEditorCollapsed ? 36 : controlsSize;
      const canvasWidth = Math.max(window.innerWidth - newWidth, 0);
      viz.renderer.setSize(canvasWidth, window.innerHeight, true);
      if (viz.camera.isPerspectiveCamera) {
        viz.camera.aspect = canvasWidth / window.innerHeight;
      }
    } else {
      const newHeight = isEditorCollapsed ? 36 : controlsSize;
      const canvasHeight = Math.max(window.innerHeight - newHeight, 0);
      viz.renderer.setSize(window.innerWidth, canvasHeight, true);
      if (viz.camera.isPerspectiveCamera) {
        viz.camera.aspect = window.innerWidth / canvasHeight;
      }
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
