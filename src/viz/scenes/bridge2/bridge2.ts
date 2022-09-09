import * as THREE from 'three';

import type { VizState } from '../../../viz';
import { initBaseScene, smoothstep } from '../../../viz/util';
import type { SceneConfig } from '..';
import { buildCustomShader } from '../../../viz/shaders/customShader';
import BridgeTopRoughnessShader from '../../shaders/bridge2/bridge_top/roughness.frag?raw';
import BridgeMistColorShader from '../../shaders/bridge2/bridge_top_mist/color.frag?raw';
import PlatformRoughnessShader from '../../shaders/bridge2/platform/roughness.frag?raw';
import PlatformColorShader from '../../shaders/bridge2/platform/color.frag?raw';
import BackgroundColorShader from '../../shaders/bridge2/background/color.frag?raw';
import TowerGlowVertexShader from '../../shaders/bridge2/tower_glow/vertex.vert?raw';
import TowerGlowColorShader from '../../shaders/bridge2/tower_glow/color.frag?raw';
import { CustomSky as Sky } from '../../CustomSky';
import { generateNormalMapFromTexture, loadTexture } from '../../../viz/textureLoading';
import { buildCustomBasicShader } from '../../../viz/shaders/customBasicShader';
import { getEngine } from '../../../viz/engine';

const locations = {
  spawn: {
    pos: new THREE.Vector3(-1.7557428208542067, 3, -0.57513478883080035),
    rot: new THREE.Vector3(-0.06, -1.514, 0),
  },
  gouge: {
    pos: new THREE.Vector3(45.97780066444547, 3.851205414533615, 0.1445978383268002),
    rot: new THREE.Vector3(-0.638, 1.556, 0),
  },
  bridgeEnd: {
    pos: new THREE.Vector3(79.57039064060402, 3.851205414533615, -0.7764391342190088),
    rot: new THREE.Vector3(-0.06, -1.514, 0),
  },
  platform: {
    pos: new THREE.Vector3(209.57039064060402, -0.851205414533615, -0.7764391342190088),
    rot: new THREE.Vector3(-0.06, -1.514, 0),
  },
  repro: {
    pos: new THREE.Vector3(167.87898623908666, 1.9848349975478469, -2.1751690172419376),
    rot: new THREE.Vector3(-0.10800000000000023, 0.09400000000000608, 0),
  },
  monolith: {
    pos: new THREE.Vector3(390.19000244140625, -2.6853251457214355, -22.77198028564453),
    rot: new THREE.Vector3(0.06800000000000045, -1.9240000000000457, 0),
  },
};

export const processLoadedScene = async (viz: VizState, loadedWorld: THREE.Group): Promise<SceneConfig> => {
  const base = initBaseScene(viz);
  base.light.castShadow = false;
  const baseDirectionalLightIntensity = 1;
  base.light.intensity = baseDirectionalLightIntensity;
  base.ambientlight.intensity = 0.1;
  base.light.position.set(20, 20, -80);
  const baseDirectionalLightColor = 0xfcbd63;
  base.light.color = new THREE.Color(baseDirectionalLightColor);

  const baseFogColor = 0x442222;
  const fog = new THREE.FogExp2(baseFogColor, 0.025);
  viz.scene.fog = fog;

  const bridgeTop = loadedWorld.getObjectByName('bridge_top')! as THREE.Mesh;
  const mat = bridgeTop.material as THREE.MeshStandardMaterial;
  mat.emissiveMap = null;
  mat.emissive = new THREE.Color(0x0);

  // This is necessary to deal with issue with GLTF exports and Three.js.
  //
  // Three.JS expects the UV map for light map to be in `uv2` but the GLTF
  // exporter puts it in `uv1`.
  //
  // TODO: Should handle in the custom shader
  const geometry = bridgeTop.geometry;
  geometry.attributes.uv2 = geometry.attributes.uv;

  bridgeTop.material = buildCustomShader(
    {
      color: new THREE.Color(0x121212),
      //  lightMap: texture,
      lightMapIntensity: 8,
    },
    { roughnessShader: BridgeTopRoughnessShader },
    {}
  );

  const loader = new THREE.ImageBitmapLoader();
  const bridgeTexture = await loadTexture(loader, 'https://ameo.link/u/abu.jpg', {
    format: THREE.RedFormat,
  });
  const bridgeTextureNormal = await generateNormalMapFromTexture(bridgeTexture);
  const bridgeCombinedDiffuseNormalTexture = await generateNormalMapFromTexture(bridgeTexture, {}, true);

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
  const arches = loadedWorld.getObjectByName('arch')! as THREE.Mesh;
  arches.material = archesMaterial;
  const brokenArches = loadedWorld.getObjectByName('broken_arch')! as THREE.Mesh;
  brokenArches.material = archesMaterial;

  const fins = loadedWorld.getObjectByName('fins')! as THREE.Mesh;
  fins.material = buildCustomShader(
    {
      color: new THREE.Color(0x333333),
      roughness: 1.2,
      metalness: 0.9,
      roughnessMap: bridgeTexture,
      uvTransform: new THREE.Matrix3().scale(5, 5),
    },
    {},
    { readRoughnessMapFromRChannel: true }
  );

  const bridge = loadedWorld.getObjectByName('bridge')! as THREE.Mesh;
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

  const bridgeBars = loadedWorld.getObjectByName('bridge_bars')! as THREE.Mesh;
  bridgeBars.material = buildCustomShader(
    {
      color: new THREE.Color(0xcccccc),
      roughness: 0.4,
      metalness: 0.98,
      map: bridgeTexture,
      normalMap: bridgeTextureNormal,
      normalScale: 2,
      uvTransform: new THREE.Matrix3().scale(0.8, 0.8),
    },
    {},
    {}
  );

  const bridgeSupportsMaterial = buildCustomShader(
    { color: new THREE.Color(0x111111), roughness: 0.9, metalness: 0.9 },
    {},
    {}
  );
  const bridgeSupports = loadedWorld.getObjectByName('bridge_supports')! as THREE.Mesh;
  bridgeSupports.material = bridgeSupportsMaterial;

  const bridgeTopMist = loadedWorld.getObjectByName('bridge_top_mistnocollide')! as THREE.Mesh;
  const bridgeTopMistMat = buildCustomShader(
    { metalness: 0, alphaTest: 0.05, transparent: true },
    { colorShader: BridgeMistColorShader },
    {}
  );
  bridgeTopMist.material = bridgeTopMistMat;
  viz.registerBeforeRenderCb(curTimeSeconds => bridgeTopMistMat.setCurTimeSeconds(curTimeSeconds));
  viz.registerDistanceMaterialSwap(
    bridgeTopMist,
    new THREE.MeshBasicMaterial({ color: new THREE.Color(0x0), transparent: true, opacity: 0 }),
    150
  );

  const monolithTexture = await loadTexture(loader, 'https://ameo.link/u/ac1.jpg', {
    format: THREE.RedFormat,
  });
  const monolithTextureCombinedDiffuseNormal = await generateNormalMapFromTexture(monolithTexture, {}, true);
  const monolithMaterial = buildCustomShader(
    {
      color: new THREE.Color(0x424242),
      map: monolithTextureCombinedDiffuseNormal,
      // normalMap: monolithTextureNormal,
      normalScale: 4,
      uvTransform: new THREE.Matrix3().scale(30, 30),
      roughness: 0.95,
      metalness: 0.2,
      fogMultiplier: 0.8,
      mapDisableDistance: 80,
    },
    {},
    { tileBreaking: { type: 'neyret' }, usePackedDiffuseNormalGBA: true }
  );
  // const monolithFarMat = new THREE.MeshBasicMaterial({ color: new THREE.Color(0x424242) });
  const monolithFarMat = buildCustomShader({ fogMultiplier: 0.8, color: new THREE.Color(0x424242) });

  const monolithRingTexture = await loadTexture(loader, 'https://ameo.link/u/ac0.jpg', {
    format: THREE.RedFormat,
  });
  const monolithRingCombinedDiffuseNormalTexture = await generateNormalMapFromTexture(
    monolithRingTexture,
    {},
    true
  );
  const monolithRingMaterial = buildCustomShader(
    {
      color: new THREE.Color(0x353535),
      map: monolithRingCombinedDiffuseNormalTexture,
      // normalMap: monolithRingTextureNormal,
      normalScale: 1.2,
      uvTransform: new THREE.Matrix3().scale(64, 64),
      roughness: 0.99,
      metalness: 0.5,
      fogMultiplier: 0.8,
      mapDisableDistance: 80,
    },
    {},
    { tileBreaking: { type: 'neyret', patchScale: 1 }, usePackedDiffuseNormalGBA: true }
  );
  // const monolithRingFarMat = new THREE.MeshBasicMaterial({ color: new THREE.Color(0x353535) });
  const monolithRingFarMat = buildCustomShader({ fogMultiplier: 0.8, color: new THREE.Color(0x353535) });

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

  const background = loadedWorld.getObjectByName('backgroundnocollide')! as THREE.Mesh;
  const backgroundMat = buildCustomBasicShader(
    { color: new THREE.Color(0x090909), alphaTest: 0.001, transparent: true, fogMultiplier: 0.6 },
    { colorShader: BackgroundColorShader }
  );
  background.material = backgroundMat;

  // const platformTexURL = 'https://ameo.link/u/ac9.jpg'; // orig
  // const platformTexURL = 'https://ameo.link/u/acn.jpg'; // tiled
  const platformTexURL = 'https://ameo.link/u/aco.jpg'; // grayscale
  const platformTexture = await loadTexture(loader, platformTexURL, {
    format: THREE.RedFormat,
    type: THREE.UnsignedByteType,
  });

  const platformCombinedDiffuseAndNormalTexture = await generateNormalMapFromTexture(
    platformTexture,
    {},
    true
  );
  const platformMaterial = buildCustomShader(
    {
      color: new THREE.Color(0xffffff),
      map: platformCombinedDiffuseAndNormalTexture,
      normalScale: 1.8,
      uvTransform: new THREE.Matrix3().scale(400, 400),
      roughness: 1,
      metalness: 0.1,
      fogMultiplier: 0.5,
      mapDisableDistance: null,
    },
    { roughnessShader: PlatformRoughnessShader, colorShader: PlatformColorShader },
    {
      tileBreaking: { type: 'neyret', patchScale: 2 },
      usePackedDiffuseNormalGBA: true,
    }
  );
  viz.registerBeforeRenderCb(curTimeSeconds => platformMaterial.setCurTimeSeconds(curTimeSeconds));
  const platform = loadedWorld.getObjectByName('platform')! as THREE.Mesh;
  platform.material = platformMaterial;

  const towerMaterial = buildCustomShader({ color: new THREE.Color(0x0) }, {}, { enableFog: false });
  const tower = loadedWorld.getObjectByName('tower')! as THREE.Mesh;
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

  const pillars = new Array(6).fill(null).map((_, i) => {
    const name = `pillar${i + 1}`;
    const obj = loadedWorld.getObjectByName(name)! as THREE.Mesh;
    return obj;
  });

  let combinedPillarTexture: THREE.Texture | null = null;
  for (const obj of pillars) {
    if (!combinedPillarTexture) {
      const pillarTexture = (obj.material as THREE.MeshStandardMaterial).map!;
      combinedPillarTexture = await generateNormalMapFromTexture(pillarTexture, {}, true);
      // TODO USE THIS once the custom shader is fixed for instancing
    }

    const mat = buildCustomShader(
      {
        map: combinedPillarTexture,
        color: new THREE.Color(0xffffff),
        fogMultiplier: 0.5,
      },
      {},
      { usePackedDiffuseNormalGBA: true }
    );
    // obj.material = mat;
  }

  const engine = await getEngine();
  const pillarCtxPtr = engine.create_pillar_ctx();
  // TODO: update every frame
  engine.compute_pillar_positions(pillarCtxPtr);

  for (let pillarIx = 0; pillarIx < pillars.length; pillarIx++) {
    const pillarMesh = pillars[pillarIx];
    const transforms = engine.get_pillar_transformations(pillarCtxPtr, pillarIx);
    (pillarMesh.material as THREE.MeshStandardMaterial).roughness = 0.95;
    const map = (pillarMesh.material as THREE.MeshStandardMaterial).map!;
    map.magFilter = THREE.NearestFilter;
    map.minFilter = THREE.NearestMipMapLinearFilter;
    const normalMap = await generateNormalMapFromTexture(map);
    (pillarMesh.material as THREE.MeshStandardMaterial).normalMap = normalMap;
    map.repeat.set(4, 4);
    const instancedMesh = new THREE.InstancedMesh(
      pillarMesh.geometry,
      pillarMesh.material,
      transforms.length / 16
    );
    instancedMesh.name = `pillar${pillarIx + 1}_nocollide`;
    const instanceMatrix = instancedMesh.instanceMatrix;
    instanceMatrix.set(transforms);
    loadedWorld.add(instancedMesh);
  }

  const skyMaterial = sky.material as THREE.ShaderMaterial;
  const sun = new THREE.Vector3();
  const darkLightColor = new THREE.Color(0x8f1116);
  const darkFogColor = new THREE.Color(0x200207);
  viz.registerBeforeRenderCb(() => {
    const playerX = viz.camera.position.x;
    const skyDarkenFactor = smoothstep(100, 300, playerX);

    // sky
    const sunElevation = 1.2 - skyDarkenFactor * 5;
    const phi = THREE.MathUtils.degToRad(90 - sunElevation);
    const theta = THREE.MathUtils.degToRad(180);

    sun.setFromSphericalCoords(1, phi, theta);

    skyMaterial.uniforms['sunPosition'].value.copy(sun);

    // light
    base.light.intensity = baseDirectionalLightIntensity - skyDarkenFactor * 0.2;
    base.light.color.setHex(
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

  return {
    locations,
    debugPos: true,
    spawnLocation: 'spawn',
    // spawnLocation: 'monolith',
    gravity: 2,
    player: {
      jumpVelocity: 10.8,
      colliderCapsuleSize: {
        height: 1.8,
        radius: 0.35,
      },
      movementAccelPerSecond: {
        onGround: 5,
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
    mieCoefficient: 0.005,
    mieDirectionalG: 0.7,
    elevation: 1.2,
    azimuth: 180,
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
