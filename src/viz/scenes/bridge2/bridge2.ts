import * as THREE from 'three';

import type { SceneConfig } from '..';
import type { VizState } from '../../../viz';
import { buildCustomBasicShader } from '../../../viz/shaders/customBasicShader';
import { buildCustomShader } from '../../../viz/shaders/customShader';
import { generateNormalMapFromTexture, loadTexture } from '../../../viz/textureLoading';
import { delay, getMesh, smoothstep } from '../../../viz/util';
import { initWebSynth } from '../../../viz/webSynth';
import { CustomSky as Sky } from '../../CustomSky';
import BackgroundColorShader from '../../shaders/bridge2/background/color.frag?raw';
import BridgeTopRoughnessShader from '../../shaders/bridge2/bridge_top/roughness.frag?raw';
import BridgeMistColorShader from '../../shaders/bridge2/bridge_top_mist/color.frag?raw';
import PlatformColorShader from '../../shaders/bridge2/platform/color.frag?raw';
import PlatformRoughnessShader from '../../shaders/bridge2/platform/roughness.frag?raw';
import Rock1RoughnessShader from '../../shaders/bridge2/rock1/roughness.frag?raw';
import TowerGlowColorShader from '../../shaders/bridge2/tower_glow/color.frag?raw';
import TowerGlowVertexShader from '../../shaders/bridge2/tower_glow/vertex.vert?raw';
import UpperRidgesColorShader from '../../shaders/bridge2/upper_ridges/color.frag?raw';

const locations = {
  spawn: {
    pos: new THREE.Vector3(-35.7557428208542067, 3, -0.57513478883080035),
    rot: new THREE.Vector3(-0.06, -1.514, 0),
  },
  gouge: {
    pos: new THREE.Vector3(45.97780066444547, 3.851205414533615, 0.1445978383268002),
    rot: new THREE.Vector3(-0.638, 1.556, 0),
  },
  bridgeEnd: {
    pos: new THREE.Vector3(79.57039064060402, 5.851205414533615, -0.7764391342190088),
    rot: new THREE.Vector3(-0.06, -1.514, 0),
  },
  platform: {
    pos: new THREE.Vector3(209.57039064060402, -0.851205414533615, -0.7764391342190088),
    rot: new THREE.Vector3(-0.06, -1.514, 0),
  },
  repro: {
    pos: new THREE.Vector3(167.87898623908666, 1.9848349975478469, -2.1751690172419376),
    rot: new THREE.Vector3(0.11799999999999987, -1.5439999999999945, 0),
  },
  monolith: {
    pos: new THREE.Vector3(390.19000244140625, -2.6853251457214355, -22.77198028564453),
    rot: new THREE.Vector3(0.06800000000000045, -1.9240000000000457, 0),
  },
  perch: {
    pos: new THREE.Vector3(251.73886108398438, 76.28307342529297, -25.859113693237305),
    rot: new THREE.Vector3(-0.4240000000000003, -2.1679999999999904, 0),
  },
  puz: {
    pos: new THREE.Vector3(144.1893768310547, 3.646326065063477, -181.00643920898438),
    rot: new THREE.Vector3(-0.08400000000000019, -1.455999999999995, 0),
  },
};

const SUN_AZIMUTH = 167;

const loadTextures = async (/* pillarMap: THREE.Texture */) => {
  const loader = new THREE.ImageBitmapLoader();

  const bridgeTextureP = loadTexture(loader, 'https://ameo.link/u/abu.jpg', {
    format: THREE.RedFormat,
  });

  const bridgeTextureNormalP = bridgeTextureP.then(bridgeTexture =>
    generateNormalMapFromTexture(bridgeTexture)
  );
  const bridgeCombinedDiffuseNormalTextureP = bridgeTextureP.then(bridgeTexture =>
    generateNormalMapFromTexture(bridgeTexture, {}, true)
  );

  const monolithTextureP = loadTexture(loader, 'https://ameo.link/u/ac1.jpg', {
    format: THREE.RedFormat,
  });
  const monolithTextureCombinedDiffuseNormalP = monolithTextureP.then(monolithTexture =>
    generateNormalMapFromTexture(monolithTexture, {}, true)
  );

  const monolithRingTextureP = loadTexture(loader, 'https://ameo.link/u/ac0.jpg', {
    format: THREE.RedFormat,
  });
  const monolithRingCombinedDiffuseNormalTextureP = monolithRingTextureP.then(monolithRingTexture =>
    generateNormalMapFromTexture(monolithRingTexture, {}, true)
  );

  // const platformTexURL = 'https://ameo.link/u/ac9.jpg'; // orig
  // const platformTexURL = 'https://ameo.link/u/acn.jpg'; // tiled
  const platformTexURL = 'https://ameo.link/u/aco.jpg'; // grayscale
  const platformTextureP = loadTexture(loader, platformTexURL, {
    format: THREE.RedFormat,
    type: THREE.UnsignedByteType,
    magFilter: THREE.NearestMipMapNearestFilter,
  });
  const platformCombinedDiffuseAndNormalTextureP = platformTextureP.then(platformTexture =>
    generateNormalMapFromTexture(platformTexture, {}, true)
  );

  const platformRidgesTextureP = loadTexture(
    loader,
    'https://ameo.link/u/b7dcc1c85adb2f53bb9567c712c30e36f236392b.jpg',
    {
      // format: THREE.RedFormat, // TODO: Support grayscale
      type: THREE.UnsignedByteType,
    }
  );
  const platformRidgesCombinedDiffuseAndNormalTextureP = platformRidgesTextureP.then(platformRidgesTexture =>
    generateNormalMapFromTexture(platformRidgesTexture, {}, true)
  );

  // const upperRidgesTextureP = loadTexture(
  //   loader,
  //   'https://ameo.link/u/6221e21a2c76e901332ebdace5069f2a9c972f1d.jpg',
  //   // 'https://ameo.link/u/aff.jpg',
  //   {
  //     // format: THREE.RedFormat, // TODO: Support grayscale
  //     type: THREE.UnsignedByteType,
  //   }
  // );
  // const upperRidgesCombinedDiffuseAndNormalTextureP = upperRidgesTextureP.then(upperRidgesTexture =>
  //   generateNormalMapFromTexture(upperRidgesTexture, {}, true)
  // );

  const platformLeftWallTextureP = loadTexture(loader, 'https://ameo.link/u/ae4.jpg');
  const platformLeftWallCombinedDiffuseAndNormalTextureP = platformLeftWallTextureP.then(
    platformLeftWallTexture => generateNormalMapFromTexture(platformLeftWallTexture, {}, true)
  );

  // const pillarNormalMapP = generateNormalMapFromTexture(pillarMap);

  const platformBuildingCombinedDiffuseAndNormalTextureP = loadTexture(
    loader,
    'https://ameo.link/u/afh.png'
  ).then(platformBuildingTexture => generateNormalMapFromTexture(platformBuildingTexture, {}, true));

  const [
    bridgeTexture,
    bridgeTextureNormal,
    bridgeCombinedDiffuseNormalTexture,
    monolithTexture,
    monolithTextureCombinedDiffuseNormal,
    monolithRingTexture,
    monolithRingCombinedDiffuseNormalTexture,
    platformTexture,
    platformCombinedDiffuseAndNormalTexture,
    platformRidgesTexture,
    platformRidgesCombinedDiffuseAndNormalTexture,
    // upperRidgesTexture,
    // upperRidgesCombinedDiffuseAndNormalTexture,
    platformLeftWallTexture,
    platformLeftWallCombinedDiffuseAndNormalTexture,
    // pillarNormalMap,
    platformBuildingCombinedDiffuseAndNormalTexture,
  ] = await Promise.all([
    bridgeTextureP,
    bridgeTextureNormalP,
    bridgeCombinedDiffuseNormalTextureP,
    monolithTextureP,
    monolithTextureCombinedDiffuseNormalP,
    monolithRingTextureP,
    monolithRingCombinedDiffuseNormalTextureP,
    platformTextureP,
    platformCombinedDiffuseAndNormalTextureP,
    platformRidgesTextureP,
    platformRidgesCombinedDiffuseAndNormalTextureP,
    // upperRidgesTextureP,
    // upperRidgesCombinedDiffuseAndNormalTextureP,
    platformLeftWallTextureP,
    platformLeftWallCombinedDiffuseAndNormalTextureP,
    // pillarNormalMapP,
    platformBuildingCombinedDiffuseAndNormalTextureP,
  ]);

  return {
    bridgeTexture,
    bridgeTextureNormal,
    bridgeCombinedDiffuseNormalTexture,
    monolithTexture,
    monolithTextureCombinedDiffuseNormal,
    monolithRingTexture,
    monolithRingCombinedDiffuseNormalTexture,
    platformTexture,
    platformCombinedDiffuseAndNormalTexture,
    platformRidgesTexture,
    platformRidgesCombinedDiffuseAndNormalTexture,
    // upperRidgesTexture,
    // upperRidgesCombinedDiffuseAndNormalTexture,
    platformLeftWallTexture,
    platformLeftWallCombinedDiffuseAndNormalTexture,
    // pillarNormalMap,
    platformBuildingCombinedDiffuseAndNormalTexture,
  };
};

export const processLoadedScene = async (viz: VizState, loadedWorld: THREE.Group): Promise<SceneConfig> => {
  const dLight = new THREE.DirectionalLight(0xcfcfcf, 1.5);

  const ambientlight = new THREE.AmbientLight(0xfac969, 0.33);
  viz.scene.add(ambientlight);

  dLight.castShadow = true;
  const baseDirectionalLightIntensity = 1;
  dLight.intensity = baseDirectionalLightIntensity;

  dLight.position.set(210, 80, -400);
  dLight.target.position.set(117, 0, 0);
  viz.scene.add(dLight.target);
  dLight.matrixWorldNeedsUpdate = true;
  dLight.updateMatrixWorld();
  dLight.target.updateMatrixWorld();

  dLight.shadow.mapSize.width = 2048 * 2;
  dLight.shadow.mapSize.height = 2048 * 2;

  dLight.shadow.camera.near = 200;
  dLight.shadow.camera.far = 800;
  dLight.shadow.camera.left = -300;
  dLight.shadow.camera.right = 300;
  dLight.shadow.camera.top = 200;
  dLight.shadow.camera.bottom = -100;

  // base.light.shadow.autoUpdate = false;
  // base.light.shadow.needsUpdate = true;

  // ??
  dLight.shadow.bias = 0.0019;

  const baseDirectionalLightColor = 0xe66332;
  dLight.color = new THREE.Color(baseDirectionalLightColor);

  viz.scene.add(dLight);

  // directional light helper
  // const helper = new THREE.DirectionalLightHelper(dLight, 5);
  // viz.scene.add(helper);

  // const helper2 = new THREE.CameraHelper(dLight.shadow.camera);
  // viz.scene.add(helper2);

  const baseFogColor = 0x442222;
  const fog = new THREE.FogExp2(baseFogColor, 0.02);
  viz.scene.fog = fog;

  const bridgeTop = getMesh(loadedWorld, 'bridge_top');
  const mat = bridgeTop.material as THREE.MeshStandardMaterial;
  mat.emissiveMap = null;
  mat.emissive = new THREE.Color(0x0);

  bridgeTop.material = buildCustomShader(
    { color: new THREE.Color(0x121212) },
    { roughnessShader: BridgeTopRoughnessShader },
    {}
  );

  // const pillar1 = getMesh(loadedWorld, 'pillar1');
  // const pillarMap = (pillar1.material as THREE.MeshStandardMaterial).map!;
  // pillarMap.magFilter = THREE.NearestFilter;
  // pillarMap.minFilter = THREE.NearestMipMapLinearFilter;
  // pillarMap.repeat.set(4, 4);
  const pillarMap = null;

  const {
    bridgeTexture,
    bridgeTextureNormal,
    bridgeCombinedDiffuseNormalTexture,
    monolithTexture,
    monolithTextureCombinedDiffuseNormal,
    monolithRingTexture,
    monolithRingCombinedDiffuseNormalTexture,
    platformTexture,
    platformCombinedDiffuseAndNormalTexture,
    platformRidgesTexture,
    platformRidgesCombinedDiffuseAndNormalTexture,
    // upperRidgesTexture,
    // upperRidgesCombinedDiffuseAndNormalTexture,
    platformLeftWallTexture,
    platformLeftWallCombinedDiffuseAndNormalTexture,
    // pillarNormalMap,
    platformBuildingCombinedDiffuseAndNormalTexture,
  } = await loadTextures();

  const archesMaterial = buildCustomShader(
    {
      color: new THREE.Color(0xcccccc),
      roughness: 0.8,
      metalness: 0.9,
      roughnessMap: bridgeTexture,
      normalMap: bridgeTextureNormal,
      normalScale: 0.4,
      uvTransform: new THREE.Matrix3().scale(3.2, 3.6),
    },
    {},
    { readRoughnessMapFromRChannel: true }
  );
  const arches = getMesh(loadedWorld, 'arch');
  arches.material = archesMaterial;
  const brokenArches = getMesh(loadedWorld, 'broken_arch');
  brokenArches.material = archesMaterial;

  const fins = getMesh(loadedWorld, 'fins');
  fins.material = buildCustomShader(
    {
      color: new THREE.Color(0x888888),
      roughness: 1.2,
      metalness: 0.9,
      roughnessMap: bridgeTexture,
      uvTransform: new THREE.Matrix3().scale(5, 5),
    },
    {},
    { readRoughnessMapFromRChannel: true }
  );

  const bridge = getMesh(loadedWorld, 'bridge');
  bridge.material = buildCustomShader(
    {
      color: new THREE.Color(0xcccccc),
      roughness: 0.9,
      metalness: 0.9,
      map: bridgeCombinedDiffuseNormalTexture,
      // normalMap: bridgeTextureNormal,
      normalScale: 3,
      uvTransform: new THREE.Matrix3().scale(10, 10),
    },
    { roughnessShader: BridgeTopRoughnessShader },
    { tileBreaking: { type: 'neyret' }, usePackedDiffuseNormalGBA: true }
  );
  viz.registerDistanceMaterialSwap(bridge, new THREE.MeshBasicMaterial({ color: 0xcccccc }), 200);

  const bridgeBars = getMesh(loadedWorld, 'bridge_bars')!;
  bridgeBars.material = buildCustomShader(
    {
      color: new THREE.Color(0xcccccc),
      roughness: 0.4,
      metalness: 0.98,
      map: bridgeCombinedDiffuseNormalTexture,
      normalScale: 2,
      uvTransform: new THREE.Matrix3().scale(0.8, 0.8),
    },
    {},
    { usePackedDiffuseNormalGBA: true }
  );

  const bridgeSupportsMaterial = buildCustomShader(
    { color: new THREE.Color(0x111111), roughness: 0.9, metalness: 0.9 },
    {},
    {}
  );
  const bridgeSupports = getMesh(loadedWorld, 'bridge_supports');
  bridgeSupports.material = bridgeSupportsMaterial;

  const bridgeTopMist = getMesh(loadedWorld, 'bridge_top_mistnocollide');
  const bridgeTopMistMat = buildCustomShader(
    { metalness: 0, alphaTest: 0.05, transparent: true },
    { colorShader: BridgeMistColorShader },
    { disableToneMapping: true }
  );
  bridgeTopMist.material = bridgeTopMistMat;
  // bridgeTopMist.material.blending = THREE.AdditiveBlending;
  viz.registerBeforeRenderCb(curTimeSeconds => bridgeTopMistMat.setCurTimeSeconds(curTimeSeconds));
  viz.registerDistanceMaterialSwap(
    bridgeTopMist,
    new THREE.MeshBasicMaterial({ color: new THREE.Color(0x0), transparent: true, opacity: 0 }),
    150
  );

  const monolithMaterial = buildCustomShader(
    {
      color: new THREE.Color(0x666666),
      map: monolithTextureCombinedDiffuseNormal,
      // normalMap: monolithTextureNormal,
      normalScale: 4,
      uvTransform: new THREE.Matrix3().scale(30, 30),
      roughness: 0.95,
      metalness: 0.2,
      fogMultiplier: 0.8,
      mapDisableDistance: 110,
    },
    {},
    { tileBreaking: { type: 'neyret' }, usePackedDiffuseNormalGBA: true }
  );
  // const monolithFarMat = new THREE.MeshBasicMaterial({ color: new THREE.Color(0x666666) });
  const monolithFarMat = buildCustomShader(
    {
      roughness: 0.95,
      metalness: 0.2,
      fogMultiplier: 0.8,
      color: new THREE.Color(0x333333),
    },
    {},
    {}
  );

  const monolithRingMaterial = buildCustomShader(
    {
      color: new THREE.Color(0x666666),
      map: monolithRingCombinedDiffuseNormalTexture,
      // normalMap: monolithRingTextureNormal,
      normalScale: 1.2,
      uvTransform: new THREE.Matrix3().scale(64, 64),
      roughness: 0.99,
      metalness: 0.5,
      fogMultiplier: 0.8,
      mapDisableDistance: 110,
    },
    {},
    {
      tileBreaking: { type: 'neyret', patchScale: 1 },
      usePackedDiffuseNormalGBA: true,
    }
  );
  // const monolithRingFarMat = new THREE.MeshBasicMaterial({ color: new THREE.Color(0x353535) });
  const monolithRingFarMat = buildCustomShader(
    {
      fogMultiplier: 0.8,
      metalness: 0.5,
      roughness: 0.99,
      color: new THREE.Color(0x222222),
    },
    {},
    {}
  );

  for (const child of loadedWorld.children) {
    if (!child.name.startsWith('monolith')) {
      continue;
    }

    if (child.name.includes('_ring')) {
      (child as THREE.Mesh).material = monolithRingMaterial;
      viz.registerDistanceMaterialSwap(child as THREE.Mesh, monolithRingFarMat, 200);
      continue;
    }

    (child as THREE.Mesh).material = monolithMaterial;
    viz.registerDistanceMaterialSwap(child as THREE.Mesh, monolithFarMat, 200);
  }

  const background = getMesh(loadedWorld, 'background');
  const backgroundMat = buildCustomBasicShader(
    { color: new THREE.Color(0x090909), alphaTest: 0.001, transparent: true, fogMultiplier: 0.6 },
    { colorShader: BackgroundColorShader }
  );
  background.material = backgroundMat;

  const platformMaterial = buildCustomShader(
    {
      color: new THREE.Color(0xffffff),
      map: platformCombinedDiffuseAndNormalTexture,
      normalScale: 1.8,
      uvTransform: new THREE.Matrix3().scale(0.8, 0.8),
      roughness: 1,
      metalness: 0.1,
      fogMultiplier: 0.5,
      mapDisableDistance: null,
      fogShadowFactor: 0.8,
    },
    { roughnessShader: PlatformRoughnessShader, colorShader: PlatformColorShader },
    {
      tileBreaking: { type: 'neyret', patchScale: 2 },
      usePackedDiffuseNormalGBA: true,
      disableToneMapping: true,
      useGeneratedUVs: true,
    }
  );
  viz.registerBeforeRenderCb(curTimeSeconds => platformMaterial.setCurTimeSeconds(curTimeSeconds));
  const platform = getMesh(loadedWorld, 'platform');
  platform.material = platformMaterial;

  const platformRidgeMaterial = buildCustomShader(
    {
      color: new THREE.Color(0x777777),
      fogMultiplier: 0.8,
      roughness: 1,
      metalness: 0.1,
      map: platformRidgesCombinedDiffuseAndNormalTexture,
      uvTransform: new THREE.Matrix3().scale(0.2, 0.2),
      normalScale: 2.8,
      mapDisableDistance: null,
      fogShadowFactor: 0.6,
    },
    {},
    { usePackedDiffuseNormalGBA: true, useGeneratedUVs: true }
  );
  ['platform_ridges_2'].forEach(name => {
    const mesh = getMesh(loadedWorld, name);
    mesh.material = platformRidgeMaterial;
  });

  const platformBuildingMaterial = buildCustomShader(
    {
      color: new THREE.Color(0x888888),
      fogMultiplier: 0.7,
      fogShadowFactor: 0.5,
      roughness: 1,
      map: platformBuildingCombinedDiffuseAndNormalTexture,
      mapDisableDistance: null,
      uvTransform: new THREE.Matrix3().scale(0.02, 0.03),
    },
    {},
    { usePackedDiffuseNormalGBA: true, useGeneratedUVs: true }
  );
  ['platform_building'].forEach(name => {
    const mesh = getMesh(loadedWorld, name)!;
    mesh.material = platformBuildingMaterial;
  });

  const upperRidgesMaterial = buildCustomShader(
    {
      color: new THREE.Color(0x595959),
      fogMultiplier: 0.8,
      roughness: 0.99,
      metalness: 0,
      map: platformBuildingCombinedDiffuseAndNormalTexture,
      uvTransform: new THREE.Matrix3().scale(0.4, 0.2),
      normalScale: 2.8,
      mapDisableDistance: null,
      fogShadowFactor: 0.5,
    },
    { colorShader: UpperRidgesColorShader },
    {
      usePackedDiffuseNormalGBA: true,
      useGeneratedUVs: true,
      tileBreaking: { type: 'neyret', patchScale: 0.8 },
    }
  );
  ['platform_ridges_3', 'platform_ridges_4'].forEach(name => {
    const mesh = getMesh(loadedWorld, name);
    mesh.material = upperRidgesMaterial;
  });
  getMesh(loadedWorld, 'platform_ridges')!.material = upperRidgesMaterial;
  viz.registerBeforeRenderCb(curTimeSeconds => upperRidgesMaterial.setCurTimeSeconds(curTimeSeconds));

  const spotLight = new THREE.SpotLight(0x750d16, 2, 0, Math.PI / 4, 0.8);
  spotLight.position.set(223, 56, 0);
  spotLight.target.position.set(426, -8, 0);
  spotLight.matrixWorldNeedsUpdate = true;
  viz.scene.add(spotLight.target);
  spotLight.castShadow = false;
  viz.scene.add(spotLight);

  viz.registerAfterRenderCb(() => {
    const distanceToSpotLight = viz.camera.position.x - spotLight.position.x;
    if (distanceToSpotLight < -70) {
      spotLight.visible = false;
    } else {
      spotLight.visible = true;
      const spotlightActivation = smoothstep(-70, -10, distanceToSpotLight);
      spotLight.intensity = 2 * spotlightActivation;
    }
  });

  const platformLeftWall = getMesh(loadedWorld, 'platform_left_wall');
  const platformLeftWallMaterial = buildCustomShader(
    {
      color: new THREE.Color(0x444444),
      fogMultiplier: 0.4,
      map: platformLeftWallCombinedDiffuseAndNormalTexture,
      normalScale: 2.8,
      uvTransform: new THREE.Matrix3().scale(40, 4),
    },
    {},
    { usePackedDiffuseNormalGBA: true }
  );
  platformLeftWall.material = platformLeftWallMaterial;

  const secretSpotLight = new THREE.SpotLight(0x2a7ef5, 2, 8, Math.PI / 4, 0.8, 0);
  secretSpotLight.position.set(255.1, 12, -165);
  secretSpotLight.target.position.set(255.1, 0, -165);
  secretSpotLight.matrixWorldNeedsUpdate = true;
  viz.scene.add(secretSpotLight.target);
  secretSpotLight.castShadow = false;
  viz.scene.add(secretSpotLight);

  const secretRewardGeometry = new THREE.TorusKnotGeometry(1, 0.3, 100, 16);
  const secretRewardMaterial = new THREE.MeshStandardMaterial({
    color: 0x2a7ef5,
    roughness: 0.5,
    metalness: 0.5,
    emissive: 0x2a7ef5,
    emissiveIntensity: 0.5,
  });
  const secretReward = new THREE.Mesh(secretRewardGeometry, secretRewardMaterial);
  secretReward.position.set(255.1, 6, -167.6);
  viz.scene.add(secretReward);

  // What a lovely place to store state
  let gotReward = false;

  viz.registerAfterRenderCb((_curTimeSeconds, tDiffSeconds) => {
    if (gotReward) {
      return;
    }

    const distanceToSecretSpotLight = viz.camera.position.distanceTo(secretSpotLight.position);
    if (distanceToSecretSpotLight < 20) {
      secretSpotLight.visible = true;
      secretReward.visible = true;

      // spin the reward
      secretReward.rotateY(tDiffSeconds * 1.5);
    } else {
      secretSpotLight.visible = false;
      secretReward.visible = false;
    }

    const distanceToSecret = viz.camera.position.distanceTo(secretReward.position);
    let dubSound: Promise<THREE.PositionalAudio> | null = null;

    if (distanceToSecret < 5 && !dubSound) {
      const dubSoundURL = 'https://ameo.link/u/ae6.ogg';
      const dubSoundLoader = new THREE.AudioLoader();

      dubSound = new Promise<THREE.PositionalAudio>(resolve => {
        dubSoundLoader.load(dubSoundURL, buffer => {
          const dubSound = new THREE.PositionalAudio(new THREE.AudioListener());
          dubSound.setBuffer(buffer);
          dubSound.setRefDistance(10);
          dubSound.setLoop(false);
          dubSound.setVolume(0.5);
          resolve(dubSound);
        });
      });
    }

    if (distanceToSecret < 2) {
      gotReward = true;
      secretReward.visible = false;
      secretSpotLight.visible = false;

      dubSound!.then(dubSound => {
        dubSound.play();
      });
    }
  });

  const rock1 = getMesh(loadedWorld, 'rock1');
  const rock1Mat = buildCustomShader(
    {
      color: new THREE.Color(0x0a0a0a),
      roughness: 0.82,
      metalness: 0,
      normalMap: bridgeTextureNormal,
      normalScale: 1.5,
      uvTransform: new THREE.Matrix3().scale(1.2, 1.2),
      roughnessMap: bridgeTexture,
      fogMultiplier: 0.5,
    },
    { roughnessShader: Rock1RoughnessShader },
    { readRoughnessMapFromRChannel: true }
  );
  rock1.material = rock1Mat;
  rock1.userData.convexhull = true;
  const rock1GoldTrim = getMesh(loadedWorld, 'rock1_gold_trim');
  rock1GoldTrim.userData.nocollide = true;
  rock1GoldTrim.material = buildCustomShader(
    {
      color: new THREE.Color(0x3e3f3e),
      roughness: 0.5,
      metalness: 0.82,
      uvTransform: new THREE.Matrix3().scale(1.2, 1.2),
      roughnessMap: bridgeTexture,
      fogMultiplier: 0.5,
    },
    { roughnessShader: Rock1RoughnessShader },
    { readRoughnessMapFromRChannel: true }
  );

  const towerMaterial = buildCustomShader({ color: new THREE.Color(0x0) }, {}, { enableFog: false });
  const tower = getMesh(loadedWorld, 'tower');
  tower.material = towerMaterial;

  const towerGlowMaterial = buildCustomBasicShader(
    { transparent: true, name: 'towerGlow', alphaTest: 0.001 },
    { colorShader: TowerGlowColorShader, vertexShader: TowerGlowVertexShader },
    { enableFog: false }
  );
  towerGlowMaterial.side = THREE.DoubleSide;
  const towerGlow = new THREE.Mesh(tower.geometry, towerGlowMaterial);
  towerGlow.position.copy(tower.position);
  towerGlow.name = 'towerGlow';
  viz.registerBeforeRenderCb(curTimeSeconds => towerGlowMaterial.setCurTimeSeconds(curTimeSeconds));
  viz.scene.add(towerGlow);

  const sky = buildSky();
  viz.scene.add(sky);

  const _pillars = new Array(6).fill(null).map((_, i) => {
    const name = `pillar${i + 1}`;
    const obj = getMesh(loadedWorld, name);
    obj.removeFromParent();
    return obj;
  });

  const skyMaterial = sky.material as THREE.ShaderMaterial;
  const sun = new THREE.Vector3();
  const darkLightColor = new THREE.Color(0x8f1116);
  const darkFogColor = new THREE.Color(0x200207);
  viz.registerBeforeRenderCb(() => {
    const playerX = viz.camera.position.x;
    const skyDarkenFactor = smoothstep(100, 300, playerX);

    // sky
    const sunElevation = 9.2 - skyDarkenFactor * 5;
    const phi = THREE.MathUtils.degToRad(90 - sunElevation);
    const theta = THREE.MathUtils.degToRad(SUN_AZIMUTH);

    sun.setFromSphericalCoords(1, phi, theta);

    skyMaterial.uniforms['sunPosition'].value.copy(sun);

    // light
    dLight.intensity = baseDirectionalLightIntensity - skyDarkenFactor * 0.2;
    dLight.color.setHex(
      darkLightColor
        .clone()
        .lerp(new THREE.Color(baseDirectionalLightColor), 1 - skyDarkenFactor)
        .getHex()
    );

    // fog
    fog.color.setHex(
      darkFogColor
        .clone()
        .lerp(new THREE.Color(baseFogColor), 1 - skyDarkenFactor)
        .getHex()
    );
  });

  delay(1000)
    .then(() => initWebSynth({ compositionIDToLoad: 64 }))
    .then(async ctx => {
      await delay(1000);

      const connectables: {
        [key: string]: {
          inputs: { [key: string]: { node: any; type: string } };
          outputs: { [key: string]: { node: any; type: string } };
        };
      } = ctx.getState().viewContextManager.patchNetwork.connectables.toJS();
      const synthDesigner = connectables['5be967b3-409b-e297-2d21-20111e4d3f2c']!;
      const midiInputs = synthDesigner.inputs.midi.node.inputCbs;
      // midiInputs.onAttack(35, 255);

      if (!localStorage.getItem('globalVolume')) {
        (window as any).setGlobalVolume(50);
      }

      const computeScaleAndShift = (inputRange: [number, number], outputRange: [number, number]) => {
        const inputRangeSize = inputRange[1] - inputRange[0];
        const firstMultiplier = inputRangeSize === 0 ? 0 : 1 / inputRangeSize;
        const firstOffset = -inputRange[0];
        const secondMultiplier = outputRange[1] - outputRange[0];
        const secondOffset = outputRange[0];

        return { firstOffset, multiplier: firstMultiplier * secondMultiplier, secondOffset };
      };

      const scaleAndShiftNode = connectables[3];
      scaleAndShiftNode.inputs.scale.node.setIsOverridden(true);
      scaleAndShiftNode.inputs.pre_scale_shift.node.setIsOverridden(true);
      scaleAndShiftNode.inputs.scale.node.setIsOverridden(true);
      scaleAndShiftNode.inputs.post_scale_shift.node.setIsOverridden(true);

      let lastMaxCutoff = 0;
      const setCutoff = (maxCutoff: number) => {
        if (maxCutoff === lastMaxCutoff) {
          return;
        }
        lastMaxCutoff = maxCutoff;

        const minCutoff = 10;
        const { firstOffset, multiplier, secondOffset } = computeScaleAndShift(
          [-1, 1],
          [minCutoff, maxCutoff]
        );
        scaleAndShiftNode.inputs.pre_scale_shift.node.manualControl.offset.value = firstOffset;
        scaleAndShiftNode.inputs.scale.node.manualControl.offset.value = multiplier;
        scaleAndShiftNode.inputs.post_scale_shift.node.manualControl.offset.value = secondOffset;
      };

      setCutoff(10);

      const monolithTowerPos = tower.position.clone();
      viz.registerAfterRenderCb(() => {
        const distanceToMonolithTower = viz.camera.position.distanceTo(monolithTowerPos);

        let activation = smoothstep(80, 384, distanceToMonolithTower);
        activation = Math.pow(activation, 2.3);
        const maxCutoff = 8 + (1 - activation) * 3200;
        setCutoff(maxCutoff);
      });
    });

  // THREE.ColorManagement.legacyMode = false;
  // viz.renderer.outputEncoding = THREE.sRGBEncoding;
  // viz.renderer.physicallyCorrectLights = true;
  ambientlight.intensity = 0.6;
  dLight.intensity = 2.8;

  viz.renderer.toneMapping = THREE.ACESFilmicToneMapping;
  viz.renderer.toneMappingExposure = 1.4;

  return {
    locations,
    debugPos: true,
    spawnLocation: 'spawn',
    // spawnLocation: 'monolith',
    gravity: 29,
    player: {
      jumpVelocity: 10.8,
      colliderCapsuleSize: {
        height: 1.5,
        radius: 0.35,
      },
      movementAccelPerSecond: {
        onGround: 5.2,
        inAir: 2.2,
      },
    },
  };
};

const buildSky = () => {
  const sky = new Sky();
  sky.scale.setScalar(450000);

  const sun = new THREE.Vector3();
  const effectController = {
    turbidity: 0.8,
    rayleigh: 2.378,
    mieCoefficient: 0.01,
    mieDirectionalG: 0.7,
    elevation: 1.2,
    azimuth: SUN_AZIMUTH,
  };

  const skyMaterial = sky.material as THREE.ShaderMaterial;
  const uniforms = skyMaterial.uniforms;
  uniforms['turbidity'].value = effectController.turbidity;
  uniforms['rayleigh'].value = effectController.rayleigh;
  uniforms['mieCoefficient'].value = effectController.mieCoefficient;
  uniforms['mieDirectionalG'].value = effectController.mieDirectionalG;

  const phi = THREE.MathUtils.degToRad(90 - effectController.elevation);
  const theta = THREE.MathUtils.degToRad(effectController.azimuth);

  sun.setFromSphericalCoords(1, phi, theta);

  uniforms['sunPosition'].value.copy(sun);
  skyMaterial.uniformsNeedUpdate = true;
  skyMaterial.needsUpdate = true;

  return sky;
};
