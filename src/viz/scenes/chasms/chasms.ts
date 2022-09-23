import * as THREE from 'three';
import { EffectComposer, Pass } from 'three/examples/jsm/postprocessing/EffectComposer.js';
import { RenderPass } from 'three/examples/jsm/postprocessing/RenderPass.js';
import { UnrealBloomPass } from 'three/examples/jsm/postprocessing/UnrealBloomPass.js';
import { ShaderPass } from 'three/examples/jsm/postprocessing/ShaderPass.js';
import { FXAAShader } from 'three/examples/jsm/shaders/FXAAShader.js';
import { OutlinePass } from 'three/examples/jsm/postprocessing/OutlinePass.js';

import type { VizState } from '../..';
import type { SceneConfig } from '..';
import { delay, DEVICE_PIXEL_RATIO, getMesh } from '../../util';
import { buildCustomBasicShader } from '../../shaders/customBasicShader';
import { buildCustomShader } from '../../shaders/customShader';
import { generateNormalMapFromTexture, loadTexture } from '../../textureLoading';
import { buildMuddyGoldenLoopsMat } from '../../materials/MuddyGoldenLoops/MuddyGoldenLoops';
import { initWebSynth } from 'src/viz/webSynth';
import { InventoryItem } from 'src/viz/inventory/Inventory';

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
    pos: new THREE.Vector3(-901.1917114257812, -5.259220123291016, 361.83123779296875),
    rot: new THREE.Vector3(0.03599999999999978, 2.5459999999998093, 0),
  },
};

const loadTextures = async () => {
  const loader = new THREE.ImageBitmapLoader();

  const chasmGroundTextureP = loadTexture(loader, 'https://ameo.link/u/afl.jpg');
  const chasmGroundTextureCombinedDiffuseNormalTextureP = chasmGroundTextureP.then(chasmGroundTexture =>
    generateNormalMapFromTexture(chasmGroundTexture, {}, true)
  );

  const bridgeTextureP = loadTexture(
    loader,
    'https://ameo.link/u/afm.jpg'
    // 'https://ameo.link/u/aha.png'
  );
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
    'https://ameo-imgen.ameo.workers.dev/img-samples/000008.1930010254.png'
  );
  const tower2TextureCombinedDiffuseNormalTextureP = tower2TextureP.then(tower2Texture =>
    generateNormalMapFromTexture(tower2Texture, {}, true)
  );

  const pillarTextureP = loadTexture(loader, 'https://ameo.link/u/ahh.png');
  const pillarTextureCombinedDiffuseNormalTextureP = pillarTextureP.then(pillarTexture =>
    generateNormalMapFromTexture(pillarTexture, {}, true)
  );

  const towerStoneTextureP = loadTexture(
    loader,
    'https://ameo-imgen.ameo.workers.dev/img-samples/000005.722669912.png'
  );
  // const towerStoneTextureCombinedDiffuseNormalTextureP = towerStoneTextureP.then(towerStoneTexture =>
  //   generateNormalMapFromTexture(towerStoneTexture, {}, true)
  // );
  const towerStoneTextureNormalP = towerStoneTextureP.then(towerStoneTexture =>
    generateNormalMapFromTexture(towerStoneTexture, {}, false)
  );

  const towerDoorArchTextureP = loadTexture(
    loader,
    'https://ameo-imgen.ameo.workers.dev/img-samples/000008.3319456407.png'
  );
  const towerDoorArchTextureCombinedDiffuseNormalTextureP = towerDoorArchTextureP.then(towerDoorArchTexture =>
    generateNormalMapFromTexture(towerDoorArchTexture, {}, true)
  );

  const towerPlinthTextureP = loadTexture(
    loader,
    'https://ameo-imgen.ameo.workers.dev/img-samples/000008.923005600.png'
  );
  const towerPlinthTextureCombinedDiffuseNormalTextureP = towerPlinthTextureP.then(towerPlinthTexture =>
    generateNormalMapFromTexture(towerPlinthTexture, {}, true)
  );

  const towerFloorTextureP = loadTexture(
    loader,
    'https://ameo-imgen.ameo.workers.dev/img-samples/000008.2978949975.png'
  );
  const towerFloorTextureCombinedDiffuseNormalTextureP = towerFloorTextureP.then(towerFloorTexture =>
    generateNormalMapFromTexture(towerFloorTexture, {}, true)
  );

  const towerPlinthArchTextureP = loadTexture(
    loader,
    'https://ameo-imgen.ameo.workers.dev/img-samples/000005.4239735677.png'
  );
  const towerPlinthArchTextureCombinedDiffuseNormalTextureP = towerPlinthArchTextureP.then(
    towerPlinthArchTexture => generateNormalMapFromTexture(towerPlinthArchTexture, {}, true)
  );

  const towerPlinthPedestalTextureP = loadTexture(
    loader,
    'https://ameo-imgen.ameo.workers.dev/img-samples/000005.1476533049.png'
  );
  const towerPlinthPedestalTextureCombinedDiffuseNormalTextureP = towerPlinthPedestalTextureP.then(
    towerPlinthPedestalTexture => generateNormalMapFromTexture(towerPlinthPedestalTexture, {}, true)
  );

  const towerPlinthStatueTextureP = loadTexture(
    loader,
    'https://ameo-imgen.ameo.workers.dev/img-samples/000008.2614578713.png'
  );
  const towerPlinthStatueTextureCombinedDiffuseNormalTextureP = towerPlinthStatueTextureP.then(
    towerPlinthStatueTexture => generateNormalMapFromTexture(towerPlinthStatueTexture, {}, true)
  );

  const muddyGoldenLoopsMatP = buildMuddyGoldenLoopsMat(loader);

  const [
    chasmGroundTextureCombinedDiffuseNormalTexture,
    bridgeTextureCombinedDiffuseNormalTexture,
    skyPanoramaImg,
    tower2TextureCombinedDiffuseNormalTexture,
    pillarTextureCombinedDiffuseNormalTexture,
    towerStoneTextureNormal,
    towerDoorArchTextureCombinedDiffuseNormalTexture,
    towerPlinthTextureCombinedDiffuseNormalTexture,
    towerFloorTextureCombinedDiffuseNormalTexture,
    towerPlinthArchTextureCombinedDiffuseNormalTexture,
    towerPlinthPedestalTextureCombinedDiffuseNormalTexture,
    towerPlinthStatueTextureCombinedDiffuseNormalTexture,
    muddyGoldenLoopsMat,
  ] = await Promise.all([
    chasmGroundTextureCombinedDiffuseNormalTextureP,
    bridgeTextureCombinedDiffuseNormalTextureP,
    skyPanoramaImgP,
    tower2TextureCombinedDiffuseNormalTextureP,
    pillarTextureCombinedDiffuseNormalTextureP,
    towerStoneTextureNormalP,
    towerDoorArchTextureCombinedDiffuseNormalTextureP,
    towerPlinthTextureCombinedDiffuseNormalTextureP,
    towerFloorTextureCombinedDiffuseNormalTextureP,
    towerPlinthArchTextureCombinedDiffuseNormalTextureP,
    towerPlinthPedestalTextureCombinedDiffuseNormalTextureP,
    towerPlinthStatueTextureCombinedDiffuseNormalTextureP,
    muddyGoldenLoopsMatP,
  ]);

  return {
    chasmGroundTextureCombinedDiffuseNormalTexture,
    bridgeTextureCombinedDiffuseNormalTexture,
    skyPanoramaImg,
    tower2TextureCombinedDiffuseNormalTexture,
    pillarTextureCombinedDiffuseNormalTexture,
    towerStoneTextureNormal,
    towerDoorArchTextureCombinedDiffuseNormalTexture,
    towerPlinthTextureCombinedDiffuseNormalTexture,
    towerFloorTextureCombinedDiffuseNormalTexture,
    towerPlinthArchTextureCombinedDiffuseNormalTexture,
    towerPlinthPedestalTextureCombinedDiffuseNormalTexture,
    towerPlinthStatueTextureCombinedDiffuseNormalTexture,
    muddyGoldenLoopsMat,
  };
};

export const processLoadedScene = async (viz: VizState, loadedWorld: THREE.Group): Promise<SceneConfig> => {
  const {
    chasmGroundTextureCombinedDiffuseNormalTexture,
    bridgeTextureCombinedDiffuseNormalTexture,
    skyPanoramaImg,
    tower2TextureCombinedDiffuseNormalTexture,
    pillarTextureCombinedDiffuseNormalTexture,
    towerStoneTextureNormal,
    towerDoorArchTextureCombinedDiffuseNormalTexture,
    towerPlinthTextureCombinedDiffuseNormalTexture,
    towerFloorTextureCombinedDiffuseNormalTexture,
    towerPlinthArchTextureCombinedDiffuseNormalTexture,
    towerPlinthPedestalTextureCombinedDiffuseNormalTexture,
    towerPlinthStatueTextureCombinedDiffuseNormalTexture,
    muddyGoldenLoopsMat,
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
  dLight.shadow.camera.right = 880;
  dLight.shadow.camera.top = 100;
  dLight.shadow.camera.bottom = -100;

  dLight.shadow.bias = 0.0049;

  viz.scene.add(dLight);

  viz.camera.near = 0.22;
  viz.camera.far = 2500;
  viz.camera.updateProjectionMatrix();

  ambientLight.intensity = 0.25;
  dLight.intensity = 2.4;

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
      color: new THREE.Color(0x737373),
      map: chasmGroundTextureCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(100 * 1.6, 100 * 1.6),
      roughness: 0.99,
      metalness: 0.01,
      normalScale: 2,
      mapDisableDistance: null,
    },
    {},
    {
      usePackedDiffuseNormalGBA: true,
      useGeneratedUVs: false,
      tileBreaking: { type: 'neyret', patchScale: 2 },
      disabledSpotLightIndices: [0],
    }
  );

  const chasmBottoms = getMesh(loadedWorld, 'chasm_bottoms');
  chasmBottoms.material = buildCustomShader(
    {
      color: new THREE.Color(0x151515),
      roughness: 0.99,
      metalness: 0.01,
    },
    {},
    {}
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

  const lightSlats = getMesh(loadedWorld, 'light_slats');
  lightSlats.material = buildCustomBasicShader({ color: new THREE.Color(0x0) }, {}, {});
  lightSlats.userData.noReceiveShadow = true;

  const backlightPanel = getMesh(loadedWorld, 'backlight_panel');
  backlightPanel.userData.noLight = true;
  backlightPanel.material = buildCustomBasicShader({ color: new THREE.Color(DIRLIGHT_COLOR) }, {}, {});

  // pillars

  const pillarMat = buildCustomShader(
    {
      color: new THREE.Color(0x444444),
      metalness: 0.74,
      roughness: 0.92,
      map: pillarTextureCombinedDiffuseNormalTexture,
      normalScale: 4,
      uvTransform: new THREE.Matrix3().scale(10 / 2.4, 6 / 3).translate(0.5, 0.5),
      ambientLightScale: 2,
    },
    {},
    { usePackedDiffuseNormalGBA: true, randomizeUVOffset: true }
  );
  loadedWorld.traverse(node => {
    if (!node.name.startsWith('pillar')) {
      return;
    }

    const pillar = node as THREE.Mesh;
    pillar.material = pillarMat;
  });

  // tower

  const towerMat = buildCustomShader(
    {
      color: new THREE.Color(0x7b7b7b),
      metalness: 0.001,
      roughness: 0.98,
      map: tower2TextureCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(0.1, 0.1),
      mapDisableDistance: null,
      normalScale: 4,
      ambientLightScale: 1.6,
    },
    {},
    {
      usePackedDiffuseNormalGBA: true,
      disabledDirectionalLightIndices: [0],
      tileBreaking: { type: 'neyret', patchScale: 1 },
      useGeneratedUVs: true,
      disabledSpotLightIndices: [1],
    }
  );
  const tower = getMesh(loadedWorld, 'tower');
  tower.material = towerMat;
  tower.userData.noReceiveShadow = true;

  const towerSpotLight = new THREE.SpotLight(0x887766, 0.3, 500, 1.5, 0.5, 1);
  towerSpotLight.position.set(-960, 100, 368);
  towerSpotLight.target.position.set(-960, 0, 368);
  towerSpotLight.castShadow = false;
  viz.scene.add(towerSpotLight.target);
  viz.scene.add(towerSpotLight);

  const stairsMat = buildCustomShader(
    {
      color: new THREE.Color(0x777777),
      metalness: 0.001,
      roughness: 0.77,
      map: towerDoorArchTextureCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(0.04, 0.04),
      mapDisableDistance: null,
      normalScale: 4,
    },
    {},
    {
      usePackedDiffuseNormalGBA: true,
      disabledDirectionalLightIndices: [0],
      disabledSpotLightIndices: [1],
      useGeneratedUVs: true,
    }
  );
  const towerStairs = getMesh(loadedWorld, 'tower_stairs');
  towerStairs.material = stairsMat;
  towerStairs.userData.convexhull = true;

  loadedWorld.traverse(obj => {
    if (obj.name.startsWith('tower_stairs_upper')) {
      const mesh = obj as THREE.Mesh;
      mesh.material = stairsMat;
      mesh.userData.convexhull = true;
    }

    if (obj.name.startsWith('tower_wall')) {
      const mesh = obj as THREE.Mesh;
      mesh.material = towerMat;
    }
  });

  const towerDoorArch = getMesh(loadedWorld, 'tower_door_arch');
  towerDoorArch.material = buildCustomShader(
    {
      color: new THREE.Color(0x989898),
      metalness: 0.001,
      roughness: 0.77,
      map: towerDoorArchTextureCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(0.04, 0.04),
      mapDisableDistance: null,
      normalScale: 4,
    },
    {},
    {
      usePackedDiffuseNormalGBA: true,
      disabledDirectionalLightIndices: [0],
      disabledSpotLightIndices: [1],
      useGeneratedUVs: true,
    }
  );

  const towerFloorMat = buildCustomShader(
    {
      color: new THREE.Color(0x646464),
      metalness: 0.001,
      roughness: 0.97,
      map: towerFloorTextureCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(0.4, 0.4),
      mapDisableDistance: null,
      normalScale: 4,
      ambientLightScale: 1.6,
    },
    {},
    {
      usePackedDiffuseNormalGBA: true,
      disabledDirectionalLightIndices: [0],
      useGeneratedUVs: true,
      tileBreaking: { type: 'neyret', patchScale: 2 },
    }
  );

  const towerFloor1Floor = getMesh(loadedWorld, 'tower_floor1_floor');
  towerFloor1Floor.material = towerFloorMat;

  const towerEntryPlinth = getMesh(loadedWorld, 'tower_entry_plinth');
  const towerEntryPlinthMat = buildCustomShader(
    {
      color: new THREE.Color(0x323232),
      metalness: 0.001,
      roughness: 0.97,
      map: towerPlinthTextureCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(0.9, 0.9),
      mapDisableDistance: null,
      normalScale: 4,
      ambientLightScale: 2,
    },
    {},
    {
      usePackedDiffuseNormalGBA: true,
      disabledDirectionalLightIndices: [0],
      useGeneratedUVs: true,
      tileBreaking: { type: 'neyret', patchScale: 2 },
    }
  );
  towerEntryPlinth.material = towerEntryPlinthMat;

  const plinthPointLight = new THREE.PointLight(
    0xff8866,
    // 0xf328da,
    1.1,
    30
  );
  plinthPointLight.position.set(-917.95, 2, 366.388);
  plinthPointLight.castShadow = false;
  viz.scene.add(plinthPointLight);

  const plinthArchMat = buildCustomShader(
    {
      color: new THREE.Color(0x323232),
      metalness: 0.001,
      roughness: 0.77,
      map: towerPlinthArchTextureCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(0.05, 0.1),
      mapDisableDistance: null,
      normalScale: 4,
      ambientLightScale: 2,
    },
    {},
    {
      usePackedDiffuseNormalGBA: true,
      disabledDirectionalLightIndices: [0],
      useGeneratedUVs: true,
      // tileBreaking: { type: 'neyret', patchScale: 2 },
    }
  );
  const towerPlinthArch = getMesh(loadedWorld, 'tower_plinth_arch');
  towerPlinthArch.material = plinthArchMat;

  const plinthPedestalMat = buildCustomShader(
    {
      color: new THREE.Color(0x383838),
      metalness: 0.18,
      roughness: 0.92,
      map: towerPlinthPedestalTextureCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(0.2, 0.2),
      mapDisableDistance: null,
      normalScale: 2.2,
      ambientLightScale: 2,
    },
    {},
    {
      usePackedDiffuseNormalGBA: true,
      disabledDirectionalLightIndices: [0],
      useGeneratedUVs: true,
      // tileBreaking: { type: 'neyret', patchScale: 2 },
    }
  );
  const plinthPedestal = getMesh(loadedWorld, 'tower_plinth_pedestal');
  plinthPedestal.material = plinthPedestalMat;

  const towerStatueMat = buildCustomShader(
    {
      color: new THREE.Color(0x906c08),
      metalness: 0.48,
      roughness: 0.72,
      map: towerPlinthStatueTextureCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(2.2, 2.2),
      mapDisableDistance: null,
      normalScale: 4,
      ambientLightScale: 3,
    },
    {
      roughnessShader: `
        float getCustomRoughness(vec3 pos, vec3 normal, float baseRoughness, float curTimeSeconds, SceneCtx ctx) {
          float shinyness = pow(ctx.diffuseColor.r * 1.5, 1.2);
          shinyness = clamp(shinyness, 0.0, 1.0);
          return 1. - shinyness;
        }`,
    },
    {
      usePackedDiffuseNormalGBA: true,
      disabledDirectionalLightIndices: [0],
      // useGeneratedUVs: true,
      // tileBreaking: { type: 'neyret', patchScale: 2 },
      readRoughnessMapFromRChannel: true,
    }
  );
  const towerStatue = getMesh(loadedWorld, 'tower_plinth_statue');
  towerStatue.material = towerStatueMat;

  const bloomPass = new UnrealBloomPass(
    new THREE.Vector2(viz.renderer.domElement.width, viz.renderer.domElement.height),
    0.75,
    0.9,
    0.1689
  );
  const composer = new EffectComposer(viz.renderer);

  composer.addPass(new RenderPass(viz.scene, viz.camera));
  composer.addPass(bloomPass);

  const torch = new THREE.Group();
  const torchRod = getMesh(loadedWorld, 'torch_rod');
  torchRod.removeFromParent();
  torch.position.copy(torchRod.position);
  const torchTop = getMesh(loadedWorld, 'torch_top');
  torchTop.removeFromParent();
  torchTop.material = new THREE.MeshPhysicalMaterial({
    color: new THREE.Color(0xb7128d),
    transparent: false,
    transmission: 0.8,
    reflectivity: 0.9,
    roughness: 0.1,
    metalness: 0,
    ior: 1.9,
    specularIntensity: 0.9,
  });
  torchRod.position.set(0, 0, 0);
  torchRod.material = buildCustomShader(
    {
      color: new THREE.Color(0x323232),
      metalness: 0.81,
      roughness: 0.77,
      map: towerPlinthArchTextureCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(0.05, 0.1),
      mapDisableDistance: null,
      normalScale: 4,
      ambientLightScale: 2,
    },
    {},
    {
      usePackedDiffuseNormalGBA: true,
      // disabledDirectionalLightIndices: [0],
      useGeneratedUVs: true,
      // tileBreaking: { type: 'neyret', patchScale: 2 },
    }
  );
  torch.add(torchRod);
  torch.add(torchTop);
  const origTorchTopPos = torchTop.position.clone();
  torchTop.position.copy(torch.position.clone().sub(origTorchTopPos));
  viz.scene.add(torch);

  const torchOutline = new OutlinePass(
    new THREE.Vector2(window.innerWidth, window.innerHeight),
    viz.scene,
    viz.camera
  );
  torchOutline.edgeStrength = 1.8;
  torchOutline.edgeGlow = 0.5;
  torchOutline.edgeThickness = 0.6;
  torchOutline.visibleEdgeColor.set(0xffffff);
  torchOutline.selectedObjects = [torch];

  composer.addPass(torchOutline);
  viz.registerResizeCb(() => {
    torchOutline.setSize(window.innerWidth, window.innerHeight);
  });

  let torchPickedUp = false;

  const useIcon = document.createElement('div');
  useIcon.style.position = 'absolute';
  // center
  useIcon.style.left = '50%';
  useIcon.style.top = '50%';
  useIcon.style.transform = 'translate(-50%, -50%)';
  useIcon.innerHTML = 'PRESS E TO USE';
  useIcon.style.fontSize = '30px';
  useIcon.style.fontFamily = 'sans-serif';
  useIcon.style.color = '#4DE3E2cf';
  // animate opacity
  useIcon.style.transition = 'opacity 0.1s';
  useIcon.style.opacity = '0';
  useIcon.style.zIndex = '200';
  document.body.appendChild(useIcon);
  let useIconVisible = false;

  viz.registerAfterRenderCb(() => {
    if (torchPickedUp) {
      return;
    }

    const distanceToTorch = viz.camera.position.distanceTo(torch.position);
    torchOutline.enabled = distanceToTorch < 12;

    // compute angle between camera and torch
    const cameraToTorch = torch.position.clone().sub(viz.camera.position);
    const cameraToTorchAngle = viz.camera.getWorldDirection(new THREE.Vector3()).angleTo(cameraToTorch);

    const shouldShowUseIcon = distanceToTorch < 3 && cameraToTorchAngle < Math.PI / 4;
    if (shouldShowUseIcon && !useIconVisible) {
      useIcon.style.opacity = '1';
      useIconVisible = true;
    } else if (!shouldShowUseIcon && useIconVisible) {
      useIcon.style.opacity = '0';
      useIconVisible = false;
    }
  });

  const equipmentScene = new THREE.Scene();
  equipmentScene.add(new THREE.AmbientLight(0xffffff, 2.5));
  const equipmentCamera = new THREE.PerspectiveCamera(75, window.innerWidth / window.innerHeight, 0.01, 100);
  equipmentCamera.position.set(0, 0, 0);
  equipmentCamera.lookAt(0, 0, -1);
  viz.registerResizeCb(() => {
    equipmentCamera.aspect = window.innerWidth / window.innerHeight;
    equipmentCamera.updateProjectionMatrix();
  });
  const equipmentPass = new RenderPass(
    equipmentScene,
    equipmentCamera,
    undefined,
    new THREE.Color(0x000000),
    0
  );
  equipmentPass.clear = false;
  composer.addPass(equipmentPass);

  class TorchItem extends InventoryItem {
    private obj: THREE.Group;

    private beforeRenderCb: (() => void) | null = null;

    constructor(obj: THREE.Group) {
      super();
      this.obj = obj;
      this.obj.scale.set(0.15, 0.15, 0.15);
      this.obj.position.set(0.2, -0.22, -0.5);
      this.obj.traverse(obj => {
        if (obj instanceof THREE.Mesh) {
          obj.material.depthTest = false;
        }
      });

      // Rotate it a bit towards -z
      this.obj.rotateY(Math.PI / 4);
      // Rotate a bit towards -y
      this.obj.rotateX(-Math.PI / 8);
    }

    public onSelected(): void {
      equipmentScene.add(this.obj);

      viz.scene.add(plinthPointLight);

      // const debugCube = new THREE.Mesh(
      //   new THREE.BoxGeometry(0.1, 0.1, 0.1),
      //   new THREE.MeshBasicMaterial({ color: 0xff0000 })
      // );
      // viz.scene.add(debugCube);
      this.beforeRenderCb = () => {
        const lightOffset = viz.camera.getWorldDirection(new THREE.Vector3()).multiplyScalar(0.2);
        plinthPointLight.position.copy(viz.camera.position.clone().add(lightOffset));
        // debugCube.position.copy(plinthPointLight.position);
      };
      viz.registerBeforeRenderCb(this.beforeRenderCb);
    }

    public onDeselected(): void {
      this.obj.removeFromParent();

      if (this.beforeRenderCb) {
        viz.unregisterBeforeRenderCb(this.beforeRenderCb);
        this.beforeRenderCb = null;
        viz.scene.remove(plinthPointLight);
      }
    }
  }

  document.addEventListener('keydown', e => {
    if (e.key === 'e') {
      if (!useIconVisible) {
        return;
      }

      if (torchPickedUp) {
        return;
      }

      torchPickedUp = true;
      viz.scene.remove(torch);
      useIcon.remove();
      plinthPointLight.removeFromParent();

      viz.inventory.addItem(new TorchItem(torch));
      console.log(equipmentScene.children);
    }

    // check for number keys
    if (e.key >= '0' && e.key <= '9') {
      let index = parseInt(e.key, 10) - 1;
      if (index === -1) {
        index = 9;
      }

      viz.inventory.setActiveItem(index);
    }
  });

  // dungeon

  const dungeonFloorMat = muddyGoldenLoopsMat;
  const dungeonFloor = getMesh(loadedWorld, 'dungeon_floor');
  dungeonFloor.material = dungeonFloorMat;

  const dungeonEntrySpotLight = new THREE.SpotLight(0xff8866, 1.3, 19, Math.PI / 10.4, 0.9);
  dungeonEntrySpotLight.position.set(-929, -18, 366.388);
  dungeonEntrySpotLight.target.position.set(-930, -30, 366.388);
  dungeonEntrySpotLight.castShadow = false;
  dungeonEntrySpotLight.updateMatrixWorld();
  dungeonEntrySpotLight.target.updateMatrixWorld();
  viz.scene.add(dungeonEntrySpotLight.target);
  viz.scene.add(dungeonEntrySpotLight);

  // sl helper
  // const spotlightHelper = new THREE.SpotLightHelper(dungeonEntrySpotLight);
  // spotlightHelper.updateMatrixWorld();
  // viz.scene.add(spotlightHelper);

  const dungeonWall = getMesh(loadedWorld, 'dungeon_wall');
  const dungeonWallMat = buildCustomShader(
    {
      color: new THREE.Color(0x383838),
      metalness: 0.18,
      roughness: 0.92,
      map: towerPlinthPedestalTextureCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(0.2, 0.2),
      mapDisableDistance: null,
      normalScale: 2.2,
      ambientLightScale: 1,
    },
    {},
    {
      usePackedDiffuseNormalGBA: true,
      disabledDirectionalLightIndices: [0],
      useGeneratedUVs: true,
      // tileBreaking: { type: 'neyret', patchScale: 2 },
    }
  );
  dungeonWall.material = dungeonWallMat;

  // LIGHTING

  // ADD AA PASS
  const fxaaPass = new ShaderPass(FXAAShader);
  fxaaPass.material.uniforms['resolution'].value.set(
    1 / (viz.renderer.domElement.width * DEVICE_PIXEL_RATIO),
    1 / (viz.renderer.domElement.height * DEVICE_PIXEL_RATIO)
  );
  fxaaPass.renderToScreen = true;
  composer.addPass(fxaaPass);

  viz.setRenderOverride(tDiffSeconds => composer.render(tDiffSeconds));

  viz.registerResizeCb(() => {
    composer.setSize(viz.renderer.domElement.width, viz.renderer.domElement.height);
    bloomPass.setSize(viz.renderer.domElement.width, viz.renderer.domElement.height);
    fxaaPass.material.uniforms['resolution'].value.set(
      1 / (viz.renderer.domElement.width * DEVICE_PIXEL_RATIO),
      1 / (viz.renderer.domElement.height * DEVICE_PIXEL_RATIO)
    );
  });

  viz.renderer.toneMapping = THREE.ACESFilmicToneMapping;
  viz.renderer.toneMappingExposure = 1;

  delay(1000)
    .then(() => initWebSynth({ compositionIDToLoad: 72 }))
    .then(async ctx => {
      await delay(1000);

      console.log(ctx);
      ctx.setGlobalBpm(55);
      ctx.startAll();
    });

  return {
    locations,
    spawnLocation: 'spawn',
    gravity: 2,
    player: {
      jumpVelocity: 10.8,
      colliderCapsuleSize: {
        height: 1.99,
        radius: 0.45,
      },
      movementAccelPerSecond: {
        onGround: 7.2,
        inAir: 2.2,
      },
    },
    debugPos: true,
  };
};
