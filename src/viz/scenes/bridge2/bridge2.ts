import * as THREE from 'three';

import type { VizState } from '../../../viz';
import { initBaseScene, smoothstep } from '../../../viz/util';
import type { SceneConfig } from '..';
import { buildCustomShader } from '../../../viz/shaders/customShader';
import BridgeTopRoughnessShader from '../../shaders/bridge2/bridge_top/roughness.frag?raw';
import BridgeMistColorShader from '../../shaders/bridge2/bridge_top_mist/color.frag?raw';
import PlatformRoughnessShader from '../../shaders/bridge2/platform/roughness.frag?raw';
import PlatformColorShader from '../../shaders/bridge2/platform/color.frag?raw';
import { CustomSky as Sky } from '../../CustomSky';
import { generateNormalMapFromTexture, loadTexture } from '../../../viz/textureLoading';

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
};

export const processLoadedScene = async (viz: VizState, loadedWorld: THREE.Group): Promise<SceneConfig> => {
  const base = initBaseScene(viz);
  base.light.castShadow = false;
  const baseDirectionalLightIntensity = 1;
  base.light.intensity = baseDirectionalLightIntensity;
  base.ambientlight.intensity = 0.1;
  base.light.position.set(40, 20, -80);
  const baseDirectionalLightColor = 0xfcbd63;
  base.light.color = new THREE.Color(baseDirectionalLightColor);

  const baseFogColor = 0x442222;
  const fog = new THREE.FogExp2(baseFogColor, 0.025);
  viz.scene.fog = fog;

  const bridgeTop = loadedWorld.getObjectByName('bridge_top')! as THREE.Mesh;
  const mat = bridgeTop.material as THREE.MeshStandardMaterial;
  const texture = mat.emissiveMap!;
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
    { color: new THREE.Color(0x121212), lightMap: texture, lightMapIntensity: 8 },
    { roughnessShader: BridgeTopRoughnessShader },
    {}
  );

  const loader = new THREE.ImageBitmapLoader();
  const bridgeTexture = await loadTexture(loader, 'https://ameo.link/u/abu.jpg', {
    // format: THREE.RedFormat,
  });
  const bridgeTextureNormal = await generateNormalMapFromTexture(bridgeTexture, {});

  const archesMaterial = buildCustomShader(
    {
      color: new THREE.Color(0x444444),
      roughness: 0.8,
      metalness: 0.9,
      roughnessMap: bridgeTexture,
      normalMap: bridgeTextureNormal,
      normalScale: 0.4,
      uvTransform: new THREE.Matrix3().scale(3.2, 3.6),
    },
    {},
    {}
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
    {}
  );

  const bridge = loadedWorld.getObjectByName('bridge')! as THREE.Mesh;
  bridge.material = buildCustomShader(
    {
      color: new THREE.Color(0xcccccc),
      roughness: 0.9,
      metalness: 0.9,
      map: bridgeTexture,
      normalMap: bridgeTextureNormal,
      normalScale: 3,
      uvTransform: new THREE.Matrix3().scale(10, 10),
    },
    { roughnessShader: BridgeTopRoughnessShader },
    { tileBreaking: { type: 'neyret' } }
  );

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

  const bridgeTopMist = loadedWorld.getObjectByName('bridge_top_mistnocollide')! as THREE.Mesh;
  const bridgeTopMistMat = buildCustomShader(
    { metalness: 0, alphaTest: 0.5, transparent: true },
    { colorShader: BridgeMistColorShader },
    {}
  );
  bridgeTopMist.material = bridgeTopMistMat;
  viz.registerBeforeRenderCb(curTimeSeconds => bridgeTopMistMat.setCurTimeSeconds(curTimeSeconds));

  const monolithTexture = await loadTexture(loader, 'https://ameo.link/u/ac1.jpg', {
    format: THREE.RedFormat,
  });
  const monolithTextureNormal = await generateNormalMapFromTexture(monolithTexture, {});
  const monolithMaterial = buildCustomShader(
    {
      color: new THREE.Color(0x424242),
      map: monolithTexture,
      normalMap: monolithTextureNormal,
      normalScale: 4,
      uvTransform: new THREE.Matrix3().scale(30, 30),
      roughness: 0.95,
      metalness: 0.2,
    },
    {},
    { tileBreaking: { type: 'neyret' } }
  );

  const monolithRingTexture = await loadTexture(loader, 'https://ameo.link/u/ac0.jpg', {
    // format: THREE.RedFormat,
  });
  const monolithRingTextureNormal = await generateNormalMapFromTexture(monolithRingTexture, {});
  const monolithRingMaterial = buildCustomShader(
    {
      color: new THREE.Color(0x353535),
      map: monolithRingTexture,
      normalMap: monolithRingTextureNormal,
      normalScale: 0.2,
      uvTransform: new THREE.Matrix3().scale(64, 64),
      roughness: 0.99,
      metalness: 0.5,
    },
    {},
    { tileBreaking: { type: 'neyret', patchScale: 1 } }
  );

  // const toRemove = [];
  for (const child of loadedWorld.children) {
    if (!child.name.startsWith('monolith')) {
      continue;
    }

    if (child.name.endsWith('_far')) {
      // toRemove.push(child);
      // continue;
      (child as THREE.Mesh).material = new THREE.MeshBasicMaterial({ color: new THREE.Color(0x0) });
    }

    if (child.name.includes('_ring')) {
      (child as THREE.Mesh).material = monolithRingMaterial;
      continue;
    }

    (child as THREE.Mesh).material = monolithMaterial;
  }

  // for (const mesh of toRemove) {
  //   loadedWorld.remove(mesh);
  // }

  const background = loadedWorld.getObjectByName('backgroundnocollide')! as THREE.Mesh;
  background.material = new THREE.MeshBasicMaterial({ color: 0x0 });

  const platformTexture = await loadTexture(loader, 'https://ameo.link/u/ac6.png', {
    // format: THREE.RedFormat,
  });
  const platformTextureNormal = await generateNormalMapFromTexture(platformTexture, {});
  const platformMaterial = buildCustomShader(
    {
      color: new THREE.Color(0xffffff),
      map: platformTexture,
      normalMap: platformTextureNormal,
      // roughnessMap: platformTexture,
      roughnessMap: await loadTexture(loader, 'https://ameo.link/u/ac6.png', {
        magFilter: THREE.NearestMipMapLinearFilter,
      }),
      normalScale: 5.5,
      uvTransform: new THREE.Matrix3().scale(400, 400),
      roughness: 1,
      metalness: 0.1,
    },
    { roughnessShader: PlatformRoughnessShader, colorShader: PlatformColorShader },
    { tileBreaking: { type: 'neyret', patchScale: 2 } }
  );
  const platform = loadedWorld.getObjectByName('platform')! as THREE.Mesh;
  platform.material = platformMaterial;

  const towerMaterial = buildCustomShader({ color: new THREE.Color(0x0) }, {}, { enableFog: false });
  const tower = loadedWorld.getObjectByName('tower')! as THREE.Mesh;
  tower.material = towerMaterial;

  const pillars = loadedWorld.getObjectByName('pillars')! as THREE.Mesh;
  pillars.material = buildCustomShader({ color: new THREE.Color(0x444444), fogMultiplier: 0.5 }, {}, {});

  const sky = buildSky();
  loadedWorld.add(sky);

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
    // spawnLocation: 'bridgeEnd',
    gravity: 6,
    player: {
      jumpVelocity: 2.8,
      colliderCapsuleSize: {
        height: 0.7,
        radius: 0.35,
      },
      movementAccelPerSecond: {
        onGround: 13,
        inAir: 3,
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
