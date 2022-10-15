import * as THREE from 'three';
import {
  EffectComposer,
  RenderPass,
  BloomEffect,
  OutlineEffect,
  EffectPass,
  SMAAEffect,
  BlendFunction,
  KernelSize,
} from 'postprocessing';

import type { VizState } from '../..';
import type { SceneConfig } from '..';
import { delay, getMesh, mix, smoothstep } from '../../util';
import { buildCustomBasicShader } from '../../shaders/customBasicShader';
import { buildCustomShader } from '../../shaders/customShader';
import { generateNormalMapFromTexture, loadTexture } from '../../textureLoading';
import { buildMuddyGoldenLoopsMat } from '../../materials/MuddyGoldenLoops/MuddyGoldenLoops';
import { initWebSynth } from 'src/viz/webSynth';
import { InventoryItem } from 'src/viz/inventory/Inventory';
import { GodraysPass } from 'three-good-godrays';
import { MainRenderPass, ClearDepthPass, DepthPass } from 'src/viz/passes/depthPrepass';

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

  const towerCeilingTextureP = loadTexture(
    loader,
    'https://ameo-imgen.ameo.workers.dev/img-samples/000008.1761839491.png'
  );
  const towerCeilingTextureCombinedDiffuseNormalTextureP = towerCeilingTextureP.then(towerCeilingTexture =>
    generateNormalMapFromTexture(towerCeilingTexture, {}, true)
  );

  const towerComputerPillarP = loadTexture(
    loader,
    'https://ameo-imgen.ameo.workers.dev/img-samples/000008.1759340770.png'
  );
  const towerComputerPillarCombinedDiffuseNormalTextureP = towerComputerPillarP.then(towerComputerPillar =>
    generateNormalMapFromTexture(towerComputerPillar, {}, true)
  );

  const towerComputerBorderP = loadTexture(
    loader,
    'https://ameo-imgen.ameo.workers.dev/img-samples/000008.3862004810.png'
  );
  const towerComputerBorderCombinedDiffuseNormalTextureP = towerComputerBorderP.then(towerComputerBorder =>
    generateNormalMapFromTexture(towerComputerBorder, {}, true)
  );

  const dungeonWallTextureP = loadTexture(
    loader,
    // 'https://ameo-imgen.ameo.workers.dev/img-samples/000008.3723778949.png'
    'https://ameo-imgen.ameo.workers.dev/img-samples/000008.1999177113.png'
  );
  const dungeonWallTextureCombinedDiffuseNormalTextureP = dungeonWallTextureP.then(dungeonWallTexture =>
    generateNormalMapFromTexture(dungeonWallTexture, {}, true)
  );

  const dungeonCeilingTextureP = loadTexture(
    loader,
    'https://ameo-imgen.ameo.workers.dev/img-samples/000005.2204019256.png'
  );
  const dungeonCeilingTextureCombinedDiffuseNormalTextureP = dungeonCeilingTextureP.then(
    dungeonCeilingTexture => generateNormalMapFromTexture(dungeonCeilingTexture, {}, true)
  );

  const muddyGoldenLoopsMatP = buildMuddyGoldenLoopsMat(loader);

  const furnaceTextureP = loadTexture(
    loader,
    // 'https://ameo-imgen.ameo.workers.dev/img-samples/000008.2657780184.png'
    // 'https://ameo.link/u/ajp.png'
    'https://ameo.link/u/ajq.jpg'
    // 'https://ameo-imgen.ameo.workers.dev/img-samples/000008.2061435413.png'
  );
  const furnaceTextureCombinedDiffuseNormalTextureP = furnaceTextureP.then(furnaceTexture =>
    generateNormalMapFromTexture(
      furnaceTexture,
      { magFilter: THREE.NearestFilter, minFilter: THREE.NearestFilter },
      true
    )
  );

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
    towerCeilingTextureCombinedDiffuseNormalTexture,
    muddyGoldenLoopsMat,
    towerComputerPillarCombinedDiffuseNormalTexture,
    towerComputerBorderCombinedDiffuseNormalTexture,
    dungeonWallTextureCombinedDiffuseNormalTexture,
    dungeonCeilingTextureCombinedDiffuseNormalTexture,
    furnaceTextureCombinedDiffuseNormalTexture,
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
    towerCeilingTextureCombinedDiffuseNormalTextureP,
    muddyGoldenLoopsMatP,
    towerComputerPillarCombinedDiffuseNormalTextureP,
    towerComputerBorderCombinedDiffuseNormalTextureP,
    dungeonWallTextureCombinedDiffuseNormalTextureP,
    dungeonCeilingTextureCombinedDiffuseNormalTextureP,
    furnaceTextureCombinedDiffuseNormalTextureP,
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
    towerCeilingTextureCombinedDiffuseNormalTexture,
    muddyGoldenLoopsMat,
    towerComputerPillarCombinedDiffuseNormalTexture,
    towerComputerBorderCombinedDiffuseNormalTexture,
    dungeonWallTextureCombinedDiffuseNormalTexture,
    dungeonCeilingTextureCombinedDiffuseNormalTexture,
    furnaceTextureCombinedDiffuseNormalTexture,
  };
};

export const processLoadedScene = async (viz: VizState, loadedWorld: THREE.Group): Promise<SceneConfig> => {
  // render a pass with no fragment shader just to populate the depth buffer
  const depthPassMaterial = new THREE.MeshBasicMaterial();
  const depthPass = new DepthPass(viz.scene, viz.camera, depthPassMaterial);
  depthPass.renderToScreen = false;

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
    towerCeilingTextureCombinedDiffuseNormalTexture,
    muddyGoldenLoopsMat,
    towerComputerPillarCombinedDiffuseNormalTexture,
    towerComputerBorderCombinedDiffuseNormalTexture,
    dungeonWallTextureCombinedDiffuseNormalTexture,
    dungeonCeilingTextureCombinedDiffuseNormalTexture,
    furnaceTextureCombinedDiffuseNormalTexture,
  } = await loadTextures();

  const ambientLight = new THREE.AmbientLight(0xffffff, 0);
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
  dLight.name = 'pink_dlight';
  dLight.castShadow = true;

  const BaseDLightY = 50;
  dLight.position.set(0, BaseDLightY, -250);
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
  dLight.shadow.camera.right = 885;
  dLight.shadow.camera.top = 200;
  dLight.shadow.camera.bottom = -100.0;

  // dLight.shadow.bias = 0.00019;

  viz.scene.add(dLight);

  // helper
  // const dLightHelper = new THREE.DirectionalLightHelper(dLight, 5);
  // viz.scene.add(dLightHelper);

  // const dLightCameraHelper = new THREE.CameraHelper(dLight.shadow.camera);
  // viz.scene.add(dLightCameraHelper);

  viz.camera.near = 0.22;
  viz.camera.far = 2500;
  viz.camera.updateProjectionMatrix();

  ambientLight.intensity = 0.32;
  dLight.intensity = 2.4;

  viz.renderer.shadowMap.autoUpdate = true;
  viz.renderer.shadowMap.needsUpdate = true;

  viz.registerBeforeRenderCb((curTimeSeconds: number) => {
    dLight.position.y = BaseDLightY + Math.sin(curTimeSeconds * 0.05) * 50 - 10;
  });

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
      color: new THREE.Color(0x000000),
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
      ambientLightScale: 1.4,
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

  const towerCeilingMat = buildCustomShader(
    {
      color: new THREE.Color(0x646464),
      metalness: 0.001,
      roughness: 0.97,
      map: towerCeilingTextureCombinedDiffuseNormalTexture,
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

    if (obj.name.startsWith('tower_ceiling')) {
      const mesh = obj as THREE.Mesh;
      mesh.material = towerCeilingMat;
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

  const plinthPointLight = new THREE.PointLight(0xffaa88, 1.4, 40, 0.77);
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

  const composer = new EffectComposer(viz.renderer);

  composer.addPass(depthPass);

  const mainRenderPass = new MainRenderPass(viz.scene, viz.camera);
  composer.addPass(mainRenderPass);

  const torch = new THREE.Group();
  const torchRod = getMesh(loadedWorld, 'torch_rod');
  torchRod.removeFromParent();
  torch.position.copy(torchRod.position);
  const torchTop = getMesh(loadedWorld, 'torch_top');
  torchTop.removeFromParent();
  const torchTopMat = buildCustomShader(
    {
      color: new THREE.Color(0xf66f1c),
      transparent: true,
      opacity: 0.5,
      roughness: 0.1,
      metalness: 0,
    },
    {}
  );
  torchTop.material = torchTopMat;
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
  torchTop.position.copy(origTorchTopPos.sub(torch.position));
  viz.scene.add(torch);

  const torchOutlineEffect = new OutlineEffect(viz.scene, viz.camera, {
    edgeStrength: 1.8,
    blendFunction: BlendFunction.SCREEN,
    pulseSpeed: 0.0,
    visibleEdgeColor: 0xffffff,
    hiddenEdgeColor: 0x22090a,
    blur: false,
    xRay: false,
  });
  torchOutlineEffect.selection.add(torchTop);
  torchOutlineEffect.selection.add(torchRod);
  const torchOutlinePass = new EffectPass(viz.camera, torchOutlineEffect);
  composer.addPass(torchOutlinePass);

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
    torchOutlinePass.enabled = distanceToTorch < 12;

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

  class TorchItem extends InventoryItem {
    private obj: THREE.Group;

    private beforeRenderCb: (() => void) | null = null;

    constructor(obj: THREE.Group) {
      super();
      this.obj = obj;
      this.obj.scale.set(0.15, 0.15, 0.15);
      this.obj.position.set(0.2, -0.22, -0.5);

      // Rotate it a bit towards -z
      this.obj.rotateY(Math.PI / 4);
      // Rotate a bit towards -y
      this.obj.rotateX(-Math.PI / 8);
    }

    public onSelected(): void {
      equipmentScene.add(this.obj);

      viz.scene.add(plinthPointLight);

      const downOffset = new THREE.Vector3(0, 0.1, 0);
      this.beforeRenderCb = () => {
        const lightOffset = viz.camera
          .getWorldDirection(new THREE.Vector3())
          .multiplyScalar(0.2)
          .add(downOffset);
        plinthPointLight.position.copy(viz.camera.position.clone().add(lightOffset));
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

  const pickUpTorch = () => {
    torchPickedUp = true;
    viz.scene.remove(torch);
    useIcon.remove();
    plinthPointLight.removeFromParent();

    viz.inventory.addItem(new TorchItem(torch));
  };

  (window as any).torchMe = () => pickUpTorch();

  document.addEventListener('keydown', e => {
    if (e.key === 'e') {
      if (!useIconVisible) {
        return;
      }

      if (torchPickedUp) {
        return;
      }

      pickUpTorch();
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

  // computer pillars room

  const towerComputerRoomPillars: THREE.Mesh[] = [];
  const computerPillarsMat = buildCustomShader(
    {
      color: new THREE.Color(0xaaaaaa),
      metalness: 0.81,
      roughness: 0.77,
      map: towerComputerPillarCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(2, 2),
      mapDisableDistance: null,
      normalScale: 8,
      ambientLightScale: 2,
    },
    {},
    {
      usePackedDiffuseNormalGBA: true,
      disabledDirectionalLightIndices: [0],
      // useGeneratedUVs: true,
      // tileBreaking: { type: 'neyret', patchScale: 2 },
    }
  );
  loadedWorld.traverse(obj => {
    if (obj.name.startsWith('tower_computer_pillar')) {
      (obj as THREE.Mesh).material = computerPillarsMat;
      towerComputerRoomPillars.push(obj as THREE.Mesh);
    }
  });

  const towerComputerBorderMat = buildCustomShader(
    {
      color: new THREE.Color(0xaaaaaa),
      metalness: 0.81,
      roughness: 0.77,
      map: towerComputerPillarCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(0.08, 0.08),
      mapDisableDistance: null,
      normalScale: 8,
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
  const towerComputerRoomBorder = loadedWorld.getObjectByName('tower_computer_room_border') as THREE.Mesh;
  towerComputerRoomBorder.material = towerComputerBorderMat;

  viz.registerBeforeRenderCb(() => {
    const ambientDimFactor = 1 - smoothstep(-950, -875, viz.camera.position.x);
    const dimmedAmbientLightIntensity = 0.08;
    const baseAmbientLightIntensity = 0.32;
    const ambientLightIntensity = mix(
      baseAmbientLightIntensity,
      dimmedAmbientLightIntensity,
      ambientDimFactor
    );
    ambientLight.intensity = ambientLightIntensity;

    // const baseTorchRange =
  });

  // dungeon

  const dungeonFloorMat = muddyGoldenLoopsMat;
  const dungeonFloor = getMesh(loadedWorld, 'dungeon_floor');
  console.log(dungeonFloor);
  dungeonFloor.material = dungeonFloorMat;

  const dungeonEntrySpotLight = new THREE.SpotLight(0xff8866, 1.3, 19, Math.PI / 10.4, 0.9);
  dungeonEntrySpotLight.position.set(-929, -18, 366.388);
  dungeonEntrySpotLight.target.position.set(-930, -30, 366.388);
  dungeonEntrySpotLight.castShadow = false;
  dungeonEntrySpotLight.updateMatrixWorld();
  dungeonEntrySpotLight.target.updateMatrixWorld();
  viz.scene.add(dungeonEntrySpotLight.target);
  viz.scene.add(dungeonEntrySpotLight);

  const dungeonWall = getMesh(loadedWorld, 'dungeon_wall');
  const dungeonWallMat = buildCustomShader(
    {
      color: new THREE.Color(0x666666),
      metalness: 0.18,
      roughness: 0.92,
      map: dungeonWallTextureCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(0.1, 0.1),
      mapDisableDistance: null,
      normalScale: 2.2,
      ambientLightScale: 1,
    },
    {},
    {
      usePackedDiffuseNormalGBA: true,
      disabledDirectionalLightIndices: [0],
      useGeneratedUVs: true,
      tileBreaking: { type: 'neyret', patchScale: 2 },
    }
  );
  dungeonWall.material = dungeonWallMat;

  const dungeonCeilingMat = buildCustomShader(
    {
      color: new THREE.Color(0x777777),
      metalness: 0.88,
      roughness: 0.52,
      map: dungeonCeilingTextureCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(0.1, 0.1),
      mapDisableDistance: null,
      normalScale: 2.2,
      ambientLightScale: 1,
    },
    {},
    {
      usePackedDiffuseNormalGBA: true,
      disabledDirectionalLightIndices: [0],
      disabledSpotLightIndices: [0, 1],
      useGeneratedUVs: true,
      // tileBreaking: { type: 'neyret', patchScale: 2 },
    }
  );
  const dungeonCeiling = getMesh(loadedWorld, 'dungeon_ceiling');
  dungeonCeiling.material = dungeonCeilingMat;

  const furnaceMat = buildCustomShader(
    {
      color: new THREE.Color(0x7b7b7b),
      metalness: 0.98,
      roughness: 0.98,
      map: furnaceTextureCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(0.1, 0.1),
      mapDisableDistance: null,
      normalScale: 0.5,
      ambientLightScale: 1,
    },
    {},
    {
      usePackedDiffuseNormalGBA: {
        lut: new Uint8Array(
          // prettier-ignore
          [15,6,9,255,30,13,13,255,30,23,18,255,42,20,28,255,31,28,33,255,48,22,19,255,35,25,46,255,44,28,26,255,33,33,16,255,46,30,18,255,44,35,30,255,53,36,40,255,66,33,19,255,56,40,33,255,40,48,14,255,56,43,10,255,63,39,31,255,56,47,38,255,50,49,48,255,66,45,27,255,59,48,46,255,55,49,59,255,68,45,45,255,35,59,12,255,66,50,38,255,54,57,9,255,70,53,47,255,61,56,44,255,64,57,35,255,71,54,61,255,67,58,51,255,79,55,45,255,72,59,48,255,66,58,80,255,73,61,21,255,78,59,42,255,73,61,60,255,88,57,41,255,83,62,58,255,70,66,61,255,80,64,56,255,75,67,55,255,71,68,71,255,51,76,10,255,82,69,10,255,84,66,54,255,76,71,40,255,79,68,62,255,65,75,36,255,87,68,40,255,70,74,55,255,63,76,68,255,84,69,76,255,82,72,54,255,85,70,69,255,93,69,52,255,76,77,20,255,102,66,41,255,85,72,62,255,80,75,66,255,60,82,55,255,89,73,66,255,84,75,72,255,94,73,62,255,92,75,60,255,89,78,71,255,88,80,67,255,93,78,68,255,105,76,55,255,82,80,101,255,91,80,81,255,75,88,5,255,95,80,77,255,86,82,90,255,90,83,75,255,85,84,81,255,92,84,45,255,102,81,64,255,96,83,68,255,99,82,75,255,84,88,58,255,97,84,59,255,91,86,67,255,102,82,71,255,94,84,81,255,97,84,73,255,89,87,76,255,96,85,78,255,97,88,78,255,100,86,92,255,96,89,83,255,104,87,74,255,95,89,89,255,102,89,81,255,103,89,84,255,103,89,78,255,114,87,68,255,105,90,72,255,102,91,77,255,105,90,90,255,102,92,85,255,99,94,85,255,121,87,63,255,102,93,90,255,103,93,83,255,87,100,47,255,100,95,82,255,110,91,83,255,112,91,76,255,91,99,85,255,107,94,84,255,110,94,81,255,98,97,93,255,103,96,91,255,107,96,89,255,110,95,88,255,109,96,92,255,110,97,80,255,105,97,100,255,108,98,85,255,103,100,72,255,117,96,75,255,106,99,88,255,103,100,91,255,106,99,96,255,110,99,89,255,108,100,92,255,107,101,84,255,114,99,88,255,110,100,94,255,112,100,100,255,118,100,85,255,108,102,97,255,116,101,94,255,110,103,91,255,125,99,84,255,114,103,89,255,113,103,92,255,113,103,95,255,110,104,95,255,107,106,96,255,110,104,115,255,116,103,99,255,113,104,101,255,113,105,99,255,118,104,85,255,110,106,102,255,116,103,107,255,120,104,93,255,117,105,96,255,111,108,100,255,116,107,99,255,114,108,98,255,117,107,93,255,113,107,107,255,117,108,97,255,133,104,79,255,122,107,92,255,119,108,102,255,116,109,105,255,121,108,106,255,117,110,103,255,116,111,98,255,123,108,100,255,121,109,97,255,113,112,107,255,119,111,101,255,116,112,103,255,121,111,103,255,120,111,109,255,121,112,115,255,118,113,108,255,122,113,99,255,121,113,108,255,115,115,107,255,125,112,107,255,121,114,106,255,129,113,99,255,121,115,104,255,124,114,103,255,121,115,112,255,133,113,93,255,121,116,108,255,126,116,97,255,128,115,106,255,121,117,106,255,125,116,108,255,121,117,117,255,121,118,113,255,124,118,103,255,126,117,112,255,127,117,115,255,125,118,112,255,126,119,110,255,130,118,104,255,126,119,107,255,127,117,128,255,124,120,111,255,130,120,109,255,127,120,116,255,130,120,113,255,124,122,118,255,129,120,123,255,123,124,116,255,129,119,146,255,126,123,116,255,137,120,112,255,129,122,114,255,127,123,113,255,133,122,118,255,131,124,113,255,127,125,111,255,130,123,122,255,131,124,118,255,133,125,110,255,128,125,127,255,135,124,114,255,131,126,121,255,132,126,117,255,130,128,120,255,136,126,119,255,129,128,124,255,132,129,118,255,135,128,123,255,139,127,127,255,135,130,122,255,143,128,110,255,133,132,112,255,138,130,119,255,135,129,137,255,136,130,130,255,135,132,128,255,133,133,125,255,137,133,118,255,141,133,124,255,138,134,124,255,141,133,129,255,138,134,127,255,138,136,135,255,139,138,132,255,141,136,156,255,138,140,128,255,152,136,129,255,145,138,131,255,147,139,125,255,144,140,130,255,146,140,137,255,147,140,145,255,147,145,138,255,146,148,147,255,150,149,129,255,154,148,143,255,152,150,138,255,160,159,152,255,163,159,163,255,162,169,154,255]
        ),
      },
      disabledDirectionalLightIndices: [0],
      disabledSpotLightIndices: [0, 1],
      useGeneratedUVs: true,
      randomizeUVOffset: true,
      // tileBreaking: { type: 'neyret', patchScale: 2 },
    }
  );
  const furnaceBarsMat = buildCustomShader(
    {
      color: new THREE.Color(0x494949),
      metalness: 0.98,
      roughness: 0.98,
      map: towerPlinthArchTextureCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(0.1, 0.1),
      mapDisableDistance: null,
      normalScale: 0.6,
      ambientLightScale: 1,
    },
    {},
    {
      usePackedDiffuseNormalGBA: true,
      disabledDirectionalLightIndices: [0],
      disabledSpotLightIndices: [0, 1],
      useGeneratedUVs: true,
      randomizeUVOffset: true,
      // tileBreaking: { type: 'neyret', patchScale: 2 },
    }
  );

  const furnacePositions: THREE.Vector3[] = [];
  loadedWorld.traverse(obj => {
    if (obj.name.startsWith('furnace_bars') && obj instanceof THREE.Mesh) {
      obj.material = furnaceBarsMat;
      obj.userData.convexhull = true;
    } else if (obj.name.startsWith('furnace') && obj instanceof THREE.Mesh) {
      obj.material = furnaceMat;
      furnacePositions.push(obj.position.clone());
      // obj.visible = false;
    }
  });

  const furnaceInteriorColor = 0xf4630a;
  const furnaceInteriorGeometry = new THREE.BoxGeometry(7.5, 7.5, 7.5);
  const furnaceInteriorMaterial = new THREE.MeshBasicMaterial({ color: furnaceInteriorColor });
  const furnaceInteriorsInstancedMesh = new THREE.InstancedMesh(
    furnaceInteriorGeometry,
    furnaceInteriorMaterial,
    furnacePositions.length
  );
  furnacePositions.forEach((furnacePosition, i) => {
    const furnaceSide = furnacePosition.z > 400 ? 'left' : 'right';
    let [x, y, z] = furnacePosition.toArray();
    y -= 2;
    z += furnaceSide === 'left' ? 0.1 : -0.1;
    furnaceInteriorsInstancedMesh.setMatrixAt(i, new THREE.Matrix4().makeTranslation(x, y - 6, z));

    const spotlight = new THREE.SpotLight(furnaceInteriorColor, 7, 20, Math.PI / 2.5, 1, 0.8);
    spotlight.position.set(x, y - 4.7, z + (furnaceSide === 'left' ? -2.7 : 2.7));
    // spotlight.position.copy(furnacePosition);
    spotlight.target.position.set(x, y - 4.9, z + (furnaceSide === 'left' ? -5.4 : 5.4));

    spotlight.target.updateMatrixWorld();
    spotlight.castShadow = false;
    // spotlight.visible = false;
    viz.scene.add(spotlight);
    viz.scene.add(spotlight.target);

    // helper
    // const spotlightHelper = new THREE.SpotLightHelper(spotlight);
    // viz.scene.add(spotlightHelper);
  });

  viz.scene.add(furnaceInteriorsInstancedMesh);
  // furnaceInteriorsInstancedMesh.visible = false;

  let outsideVisible: boolean | null = null;
  viz.registerAfterRenderCb(() => {
    const outsideShouldBeVisible = viz.camera.position.x >= -932;
    if (outsideShouldBeVisible === outsideVisible) {
      return;
    }
    outsideVisible = outsideShouldBeVisible;

    if (outsideShouldBeVisible) {
      chasm.visible = true;
      chasmBottoms.visible = true;
      dLight.visible = true;
      bridge.visible = true;
      building.visible = true;
      lightSlats.visible = true;
      backlightPanel.visible = true;
      towerStairs.visible = true;

      towerComputerRoomBorder.visible = false;
      dungeonCeiling.visible = false;
      dungeonWall.visible = false;
      dungeonFloor.visible = false;
      towerComputerRoomPillars.forEach(pillar => (pillar.visible = false));
    } else {
      chasm.visible = false;
      chasmBottoms.visible = false;
      dLight.visible = false;
      bridge.visible = false;
      building.visible = false;
      lightSlats.visible = false;
      backlightPanel.visible = false;
      towerStairs.visible = false;

      towerComputerRoomBorder.visible = true;
      dungeonCeiling.visible = true;
      dungeonWall.visible = true;
      dungeonFloor.visible = true;
      towerComputerRoomPillars.forEach(pillar => (pillar.visible = true));
    }
  });

  // POST-PROCESSING

  const godraysPass = new GodraysPass(dLight, viz.camera, {
    color: dLight.color,
    density: 1 / 80,
    maxDensity: 0.8,
    distanceAttenuation: 0.4,
    raymarchSteps: 86,
    blur: { kernelSize: KernelSize.SMALL, variance: 0.25 },
  });
  godraysPass.renderToScreen = false;
  composer.addPass(godraysPass);

  const bloomEffect = new BloomEffect({
    intensity: 2,
    mipmapBlur: true,
    luminanceThreshold: 0.33,
    blendFunction: BlendFunction.ADD,
    luminanceSmoothing: 0.05,
    radius: 0.86,
  });
  const bloomPass = new EffectPass(viz.camera, bloomEffect);
  bloomPass.dithering = false;

  composer.addPass(bloomPass);

  const equipmentScene = new THREE.Scene();
  equipmentScene.add(new THREE.AmbientLight(0xffffff, 1));
  equipmentScene.add(new THREE.DirectionalLight(0xffffff, 1));
  const equipmentCamera = new THREE.PerspectiveCamera(75, window.innerWidth / window.innerHeight, 0.01, 100);
  equipmentCamera.position.set(0, 0, 0);
  equipmentCamera.lookAt(0, 0, -1);
  viz.registerResizeCb(() => {
    equipmentCamera.aspect = window.innerWidth / window.innerHeight;
    equipmentCamera.updateProjectionMatrix();
  });
  const equipmentPass = new RenderPass(equipmentScene, equipmentCamera, undefined);
  equipmentPass.clear = false;

  composer.addPass(new ClearDepthPass());
  composer.addPass(equipmentPass);

  const aaPass = new EffectPass(viz.camera, new SMAAEffect());
  composer.addPass(aaPass);

  viz.setRenderOverride(tDiffSeconds => composer.render(tDiffSeconds));

  viz.registerResizeCb(() => {
    composer.setSize(viz.renderer.domElement.width, viz.renderer.domElement.height);
  });

  viz.renderer.toneMapping = THREE.ACESFilmicToneMapping;
  viz.renderer.toneMappingExposure = 1;

  const customDepthMaterial = new THREE.MeshDepthMaterial({
    depthPacking: THREE.RGBADepthPacking,
  });
  customDepthMaterial.depthWrite = false;
  customDepthMaterial.depthTest = false;
  viz.scene.traverse(obj => {
    if (obj instanceof THREE.Mesh) {
      obj.customDepthMaterial = customDepthMaterial;
    }
  });
  loadedWorld.traverse(obj => {
    if (obj instanceof THREE.Mesh) {
      obj.customDepthMaterial = customDepthMaterial;
    }
  });

  delay(1000)
    .then(() => initWebSynth({ compositionIDToLoad: 72 }))
    .then(async ctx => {
      await delay(1000);

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
        onGround: 15.2,
        inAir: 2.2,
      },
    },
    debugPos: true,
  };
};
