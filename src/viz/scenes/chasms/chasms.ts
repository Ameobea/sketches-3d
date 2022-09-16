import * as THREE from 'three';
import { EffectComposer } from 'three/examples/jsm/postprocessing/EffectComposer.js';
import { RenderPass } from 'three/examples/jsm/postprocessing/RenderPass.js';
import { UnrealBloomPass } from 'three/examples/jsm/postprocessing/UnrealBloomPass.js';
// import AA pass
import { ShaderPass } from 'three/examples/jsm/postprocessing/ShaderPass.js';
import { FXAAShader } from 'three/examples/jsm/shaders/FXAAShader.js';

import type { VizState } from '../..';
import type { SceneConfig } from '..';
import { DEVICE_PIXEL_RATIO, getMesh } from '../../util';
import { buildCustomBasicShader } from '../../shaders/customBasicShader';
import { buildCustomShader } from '../../shaders/customShader';
import { generateNormalMapFromTexture, loadTexture } from '../../textureLoading';

const locations = {
  spawn: {
    pos: new THREE.Vector3(26, 12, 65),
    rot: new THREE.Vector3(-0.06, -1.514, 0),
  },
  bridge: {
    pos: new THREE.Vector3(-29.781932830810547, 21.383197784423828, 227.98403930664062),
    rot: new THREE.Vector3(-0.18800000000000036, -0.12400000000002512, 0),
  },
};

const loadTextures = async () => {
  const loader = new THREE.ImageBitmapLoader();

  const chasmGroundTextureP = loadTexture(loader, 'https://ameo.link/u/afl.jpg');
  const chasmGroundTextureCombinedDiffuseNormalTextureP = chasmGroundTextureP.then(chasmGroundTexture =>
    generateNormalMapFromTexture(chasmGroundTexture, {}, true)
  );

  const bridgeTextureP = loadTexture(loader, 'https://ameo.link/u/afm.jpg');
  const bridgeTextureCombinedDiffuseNormalTextureP = bridgeTextureP.then(bridgeTexture =>
    generateNormalMapFromTexture(bridgeTexture, {}, true)
  );

  const [chasmGroundTextureCombinedDiffuseNormalTexture, bridgeTextureCombinedDiffuseNormalTexture] =
    await Promise.all([
      chasmGroundTextureCombinedDiffuseNormalTextureP,
      bridgeTextureCombinedDiffuseNormalTextureP,
    ]);

  return {
    chasmGroundTextureCombinedDiffuseNormalTexture,
    bridgeTextureCombinedDiffuseNormalTexture,
  };
};

export const processLoadedScene = async (viz: VizState, loadedWorld: THREE.Group): Promise<SceneConfig> => {
  const { chasmGroundTextureCombinedDiffuseNormalTexture, bridgeTextureCombinedDiffuseNormalTexture } =
    await loadTextures();

  const ambientLight = new THREE.AmbientLight(0xffffff, 0.5);
  viz.scene.add(ambientLight);

  const DIRLIGHT_COLOR = 0xf73173;
  const dLight = new THREE.DirectionalLight(DIRLIGHT_COLOR, 0.5);
  dLight.castShadow = true;

  const BaseDLightY = 50;
  dLight.position.set(0, BaseDLightY, -200);
  dLight.target.position.set(0, 0, 0);
  viz.scene.add(dLight.target);
  dLight.matrixWorldNeedsUpdate = true;
  dLight.updateMatrixWorld();
  dLight.target.updateMatrixWorld();

  dLight.shadow.mapSize.width = 2048 * 2;
  dLight.shadow.mapSize.height = 2048 * 2;

  dLight.shadow.camera.near = 1;
  dLight.shadow.camera.far = 900;
  dLight.shadow.camera.left = -550;
  dLight.shadow.camera.right = 550;
  dLight.shadow.camera.top = 100;
  dLight.shadow.camera.bottom = -100;

  dLight.shadow.bias = 0.0049;

  viz.scene.add(dLight);

  const SkyDLightColor = 0xff8888;
  const skyDLight = new THREE.DirectionalLight(SkyDLightColor, 0.2);
  skyDLight.castShadow = true;

  skyDLight.position.set(0, 500, 0);

  skyDLight.shadow.mapSize.width = 2048 * 1;
  skyDLight.shadow.mapSize.height = 2048 * 1;

  skyDLight.shadow.camera.near = 1;
  skyDLight.shadow.camera.far = 800;
  skyDLight.shadow.camera.left = -400;
  skyDLight.shadow.camera.right = 400;
  skyDLight.shadow.camera.top = 400;
  skyDLight.shadow.camera.bottom = -400;

  skyDLight.shadow.bias = 0.0049;

  skyDLight.shadow.autoUpdate = false;
  skyDLight.shadow.needsUpdate = true;

  // viz.scene.add(skyDLight);

  ambientLight.intensity = 0.25;
  dLight.intensity = 2.8;

  viz.renderer.shadowMap.autoUpdate = true;
  viz.renderer.shadowMap.needsUpdate = true;

  viz.registerBeforeRenderCb((curTimeSeconds: number) => {
    dLight.position.y = BaseDLightY + Math.sin(curTimeSeconds * 0.05) * 50;
  });

  // directional light helper
  const helper = new THREE.DirectionalLightHelper(dLight, 5);
  viz.scene.add(helper);

  // const helper2 = new THREE.CameraHelper(dLight.shadow.camera);
  // viz.scene.add(helper2);

  const chasm = getMesh(loadedWorld, 'chasm');
  chasm.material = buildCustomShader(
    {
      color: new THREE.Color(0x808080),
      map: chasmGroundTextureCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(100, 100),
      roughness: 0.99,
      metalness: 0.21,
      normalScale: 2,
      mapDisableDistance: null,
    },
    {},
    {
      usePackedDiffuseNormalGBA: true,
      tileBreaking: { type: 'neyret', patchScale: 2 },
    }
  );

  const bridge = getMesh(loadedWorld, 'bridge');
  bridge.material = buildCustomShader(
    {
      color: new THREE.Color(0x808080),
      map: bridgeTextureCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(8, 8),
    },
    {},
    { usePackedDiffuseNormalGBA: true }
  );

  const lightSlats = getMesh(loadedWorld, 'light_slats');
  lightSlats.material = buildCustomBasicShader({ color: new THREE.Color(0x0) }, {}, {});
  lightSlats.userData.noReceiveShadow = true;

  const backlightPanel = getMesh(loadedWorld, 'backlight_panel');
  backlightPanel.userData.noLight = true;
  backlightPanel.material = buildCustomBasicShader({ color: new THREE.Color(DIRLIGHT_COLOR) }, {}, {});

  const sky = getMesh(loadedWorld, 'sky');
  sky.material = buildCustomBasicShader({ color: new THREE.Color(0x222222) }, {}, {});
  // sky.userData.noLight = true;

  const bloomPass = new UnrealBloomPass(
    new THREE.Vector2(viz.renderer.domElement.width, viz.renderer.domElement.height),
    0.75,
    0.9,
    0.1689
  );
  const composer = new EffectComposer(viz.renderer);

  composer.addPass(new RenderPass(viz.scene, viz.camera));
  composer.addPass(bloomPass);
  // ADD AA PASS
  const fxaaPass = new ShaderPass(FXAAShader);
  fxaaPass.material.uniforms['resolution'].value.set(
    1 / (viz.renderer.domElement.width * DEVICE_PIXEL_RATIO),
    1 / (viz.renderer.domElement.height * DEVICE_PIXEL_RATIO)
  );
  fxaaPass.renderToScreen = true;
  composer.addPass(fxaaPass);

  viz.registerResizeCb(() => {
    composer.setSize(viz.renderer.domElement.width, viz.renderer.domElement.height);
    bloomPass.setSize(viz.renderer.domElement.width, viz.renderer.domElement.height);
    fxaaPass.material.uniforms['resolution'].value.set(
      1 / (viz.renderer.domElement.width * DEVICE_PIXEL_RATIO),
      1 / (viz.renderer.domElement.height * DEVICE_PIXEL_RATIO)
    );
  });

  viz.registerAfterRenderCb(() => {
    composer.render();
  });

  viz.renderer.toneMapping = THREE.ACESFilmicToneMapping;
  viz.renderer.toneMappingExposure = 1;

  // viz.camera.near = 0.1;
  // viz.camera.far = 600;

  return {
    locations,
    spawnLocation: 'spawn',
    gravity: 2,
    player: {
      jumpVelocity: 10.8,
      colliderCapsuleSize: {
        height: 1.8,
        radius: 0.35,
      },
      movementAccelPerSecond: {
        onGround: 7.2,
        inAir: 2.2,
      },
    },
    debugPos: true,
  };
};
