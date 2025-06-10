import * as THREE from 'three';
import { N8AOPostPass } from 'n8ao';
import { mount } from 'svelte';

import type { Viz } from 'src/viz';
import type { SceneConfig } from '..';
import { initManifoldWasm } from 'src/viz/wasmComp/manifold';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import ReplUi, { type ReplCtx } from './ReplUI.svelte';
import { buildGrayFossilRockMaterial } from 'src/viz/materials/GrayFossilRock/GrayFossilRockMaterial';

const locations = {
  spawn: {
    pos: new THREE.Vector3(48.17740050559579, 23.920086905508146, 8.603910511800485),
    rot: new THREE.Vector3(-0.022, 1.488, 0),
  },
};

const initRepl = async (
  viz: Viz,
  repl: typeof import('src/viz/wasmComp/geoscript_repl'),
  setReplCtx: (ctx: ReplCtx) => void,
  matPromise: Promise<THREE.Material>
) => {
  const ctxPtr = repl.geoscript_repl_init();
  const _ui = mount(ReplUi, {
    target: document.getElementById('viz-container')!,
    props: { viz, ctxPtr, repl, setReplCtx, baseMat: await matPromise },
  });
};

export const processLoadedScene = async (
  viz: Viz,
  _loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  const loader = new THREE.ImageBitmapLoader();
  const matPromise = buildGrayFossilRockMaterial(
    loader,
    { uvTransform: new THREE.Matrix3().scale(0.2, 0.2), color: 0xcccccc, mapDisableDistance: null },
    {},
    { useGeneratedUVs: false, useTriplanarMapping: true, tileBreaking: undefined }
  );
  const manifoldInitPromise = initManifoldWasm();

  const ambientLight = new THREE.AmbientLight(0xffffff, 1.8);
  viz.scene.add(ambientLight);

  const dirLight = new THREE.DirectionalLight(0xffffff, 5);
  dirLight.position.set(-20, 50, 0);
  viz.scene.add(dirLight);

  dirLight.castShadow = true;
  dirLight.shadow.mapSize.width = 2048 * 2;
  dirLight.shadow.mapSize.height = 2048 * 2;
  dirLight.shadow.radius = 4;
  dirLight.shadow.blurSamples = 16;
  viz.renderer.shadowMap.type = THREE.VSMShadowMap;
  dirLight.shadow.bias = -0.0001;

  dirLight.shadow.camera.near = 8;
  dirLight.shadow.camera.far = 300;
  dirLight.shadow.camera.left = -300;
  dirLight.shadow.camera.right = 380;
  dirLight.shadow.camera.top = 94;
  dirLight.shadow.camera.bottom = -140;

  const [repl] = await Promise.all([
    import('../../wasmComp/geoscript_repl').then(async engine => {
      await engine.default();
      return engine;
    }),
    manifoldInitPromise,
  ]);

  configureDefaultPostprocessingPipeline(
    viz,
    vizConf.graphics.quality,
    (composer, viz, quality) => {
      if (vizConf.graphics.quality > GraphicsQuality.Low) {
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
        n8aoPass.configuration.halfRes = vizConf.graphics.quality <= GraphicsQuality.Medium;
        n8aoPass.setQualityMode(
          {
            [GraphicsQuality.Low]: 'Performance',
            [GraphicsQuality.Medium]: 'Low',
            [GraphicsQuality.High]: 'Medium',
          }[vizConf.graphics.quality]
        );
      }
    },
    undefined,
    undefined,
    undefined,
    true
  );

  const axisHelper = new THREE.AxesHelper(100);
  axisHelper.position.set(0, 0, 0);
  viz.scene.add(axisHelper);

  const updateCanvasSize = () => {
    const controlsHeight = Math.max(250, 0.25 * window.innerHeight);
    const canvasHeight = Math.max(window.innerHeight - controlsHeight, 300);
    viz.renderer.setSize(window.innerWidth, canvasHeight, true);
    viz.camera.aspect = window.innerWidth / canvasHeight;
    viz.camera.updateProjectionMatrix();
  };
  viz.registerResizeCb(updateCanvasSize);
  updateCanvasSize();

  let ctx: ReplCtx | null = null;
  initRepl(
    viz,
    repl,
    (newCtx: ReplCtx) => {
      ctx = newCtx;
    },
    matPromise
  );

  return {
    locations,
    spawnLocation: 'spawn',
    viewMode: {
      type: 'orbit',
      pos: new THREE.Vector3(10, 10, 10),
      target: new THREE.Vector3(0, 0, 0),
    },
    customControlsEntries: [
      { key: '.', action: () => ctx?.centerView(), label: 'center view' },
      { key: 'w', action: () => ctx?.toggleWireframe(), label: 'toggle wireframe' },
    ],
  };
};
