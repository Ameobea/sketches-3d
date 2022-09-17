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
    pos: new THREE.Vector3(26, 15, 65),
    rot: new THREE.Vector3(-0.06, -1.514, 0),
  },
  bridge: {
    pos: new THREE.Vector3(67.64376831054688, 24.068376541137695, 247.71884155273438),
    rot: new THREE.Vector3(-0.16, -0.02, 0),
  },
  conn: {
    pos: new THREE.Vector3(-753.6656494140625, -16.441192626953125, 364.31439208984375),
    rot: new THREE.Vector3(0.18199999999999983, 1.4839999999999047, 0),
  },
  tower: {
    pos: new THREE.Vector3(-897.6182861328125, -16.432735443115234, 374.9806823730469),
    rot: new THREE.Vector3(0.011999999999998751, 0.45999999999988295, 0),
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

  const skyPanoramaImgP = loader.loadAsync(
    // 'https://ameo-imgen.ameo.workers.dev/skys/sky2__equirectangular_projection.png'
    // 'https://ameo-imgen.ameo.workers.dev/skys/4221853866_dark_black_horizon_with_dark_red_clouds_floating_above_it___the_bottom_half_is_completely_is_black_and_indistinct__there_are_no_stars_in_the_pitch_black_night_sky__ethereal_surreal_style__equirectangular_projection.png'
    'https://ameo-imgen.ameo.workers.dev/skys/upscaled2.jpg'
  );

  const tower2TextureP = loadTexture(
    loader,
    // 'https://ameo.link/u/ah6.jpg'
    'https://ameo.link/u/ah7.png'
  );
  const tower2TextureCombinedDiffuseNormalTextureP = tower2TextureP.then(tower2Texture =>
    generateNormalMapFromTexture(tower2Texture, {}, true)
  );

  const [
    chasmGroundTextureCombinedDiffuseNormalTexture,
    bridgeTextureCombinedDiffuseNormalTexture,
    skyPanoramaImg,
    tower2TextureCombinedDiffuseNormalTexture,
  ] = await Promise.all([
    chasmGroundTextureCombinedDiffuseNormalTextureP,
    bridgeTextureCombinedDiffuseNormalTextureP,
    skyPanoramaImgP,
    tower2TextureCombinedDiffuseNormalTextureP,
  ]);

  return {
    chasmGroundTextureCombinedDiffuseNormalTexture,
    bridgeTextureCombinedDiffuseNormalTexture,
    skyPanoramaImg,
    tower2TextureCombinedDiffuseNormalTexture,
  };
};

export const processLoadedScene = async (viz: VizState, loadedWorld: THREE.Group): Promise<SceneConfig> => {
  const {
    chasmGroundTextureCombinedDiffuseNormalTexture,
    bridgeTextureCombinedDiffuseNormalTexture,
    skyPanoramaImg,
    tower2TextureCombinedDiffuseNormalTexture,
  } = await loadTextures();

  const ambientLight = new THREE.AmbientLight(0xffffff, 0.5);
  viz.scene.add(ambientLight);

  const texture = new THREE.Texture(
    skyPanoramaImg as any,
    THREE.EquirectangularRefractionMapping,
    undefined,
    undefined,
    THREE.NearestFilter,
    THREE.NearestFilter
  );
  texture.needsUpdate = true;

  viz.scene.background = texture;

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
  dLight.shadow.camera.right = 950;
  dLight.shadow.camera.top = 100;
  dLight.shadow.camera.bottom = -100;

  dLight.shadow.bias = 0.0049;

  viz.scene.add(dLight);

  viz.camera.near = 0.18;
  viz.camera.far = 2500;
  viz.camera.updateProjectionMatrix();

  ambientLight.intensity = 0.25;
  dLight.intensity = 2.8;

  viz.renderer.shadowMap.autoUpdate = true;
  viz.renderer.shadowMap.needsUpdate = true;

  viz.registerBeforeRenderCb((curTimeSeconds: number) => {
    dLight.position.y = BaseDLightY + Math.sin(curTimeSeconds * 0.05) * 50;
  });

  // directional light helper
  // const helper = new THREE.DirectionalLightHelper(dLight, 5);
  // viz.scene.add(helper);

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
      disabledSpotLightIndices: [0],
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

  const building = getMesh(loadedWorld, 'building');
  building.material = buildCustomShader(
    {
      color: new THREE.Color(0x121212),
      metalness: 0.2,
      roughness: 0.97,
    },
    {},
    {}
  );

  const tower = getMesh(loadedWorld, 'tower');
  tower.material = buildCustomShader(
    {
      color: new THREE.Color(0x7b7b7b),
      metalness: 0.001,
      roughness: 0.87,
      map: tower2TextureCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(400 / 2, 100 / 2),
      mapDisableDistance: null,
      normalScale: 4,
    },
    {},
    {
      usePackedDiffuseNormalGBA: true,
      disabledDirectionalLightIndices: [0],
    }
  );
  tower.userData.noReceiveShadow = true;

  const towerSpotLight = new THREE.SpotLight(0xff6677, 0.5, 500, 1.5, 0.5, 1);
  towerSpotLight.position.set(-960, 300, 368);
  towerSpotLight.target.position.set(-960, 0, 368);
  towerSpotLight.castShadow = false;
  viz.scene.add(towerSpotLight.target);
  viz.scene.add(towerSpotLight);

  const lightSlats = getMesh(loadedWorld, 'light_slats');
  lightSlats.material = buildCustomBasicShader({ color: new THREE.Color(0x0) }, {}, {});
  lightSlats.userData.noReceiveShadow = true;

  const backlightPanel = getMesh(loadedWorld, 'backlight_panel');
  backlightPanel.userData.noLight = true;
  backlightPanel.material = buildCustomBasicShader({ color: new THREE.Color(DIRLIGHT_COLOR) }, {}, {});

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
