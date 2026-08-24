import * as THREE from 'three';
import { mount } from 'svelte';

import type { Viz } from 'src/viz';
import type { SceneConfig } from '..';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import GeotoyApp from 'src/geotoy/GeotoyApp.svelte';
import { buildMeshPipeline } from 'src/geotoy/modes/mesh/pipeline';
import type { Composition, CompositionVersion, User } from 'src/geoscript/geotoyAPIClient';
import type { MaterialOverrideMode } from 'src/geotoy/types';
import { buildGeotoyKeymap } from 'src/geotoy/modules/keymapTable';
import { WorkerManager } from 'src/geoscript/workerManager';
import type { EvalRequest } from 'src/geotoy/modes/mesh/evalResult';

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

  const pipelineController = buildMeshPipeline(viz, quality, userData?.renderMode ?? false);

  // Nothing in a geoscript scene animates on its own, so present only on change. A shader that
  // reads `curTimeSeconds` keeps rendering because the governor hashes that uniform, not
  // through any registration here. The render harness drives frames explicitly, so it stays
  // ungoverned.
  if (!userData?.renderMode) {
    viz.enableFrameGovernor();
  }

  if (!userData?.renderMode && localStorage.getItem('geoscript-axis-helpers') !== 'false') {
    const axisHelper = new THREE.AxesHelper(100);
    axisHelper.position.set(0, 0, 0);
    viz.scene.add(axisHelper);
  }

  mount(GeotoyApp, {
    target: document.getElementById('viz-container')!,
    props: { viz, workerManager, userData, pipelineController },
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
