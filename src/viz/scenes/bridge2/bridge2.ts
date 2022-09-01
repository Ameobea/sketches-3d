import * as THREE from 'three';

import type { VizState } from '../../../viz';
import { initBaseScene } from '../../../viz/util';
import type { SceneConfig } from '..';
import { buildCustomShader } from '../../../viz/shaders/customShader';
import groundRoughnessShader from '../../shaders/subdivided/ground/roughness.frag?raw';
import BridgeMistColorShader from '../../shaders/bridge2/bridge_top_mist/color.frag?raw';
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
};

export const processLoadedScene = async (viz: VizState, loadedWorld: THREE.Group): Promise<SceneConfig> => {
  const base = initBaseScene(viz);
  base.light.castShadow = false;
  base.light.intensity += 0.5;
  base.light.position.set(40, 10, -80);
  base.light.color = new THREE.Color(0xffcd73);

  viz.scene.fog = new THREE.FogExp2(0x442222, 0.1);

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
    { roughnessShader: groundRoughnessShader },
    {}
  );

  // const ex = new THREE.CapsuleGeometry(0.35, 1.3, 24, 24);
  // const exMesh = new THREE.Mesh(ex, new THREE.MeshStandardMaterial({ color: 0xff0000 }));
  // exMesh.position.set(-1.7, 5, -0.6);
  // loadedWorld.add(exMesh);

  const archesMaterial = buildCustomShader(
    { color: new THREE.Color(0x444444), roughness: 0.4, metalness: 0.9 },
    {},
    {}
  );
  const arches = loadedWorld.getObjectByName('arch')! as THREE.Mesh;
  arches.material = archesMaterial;
  const brokenArches = loadedWorld.getObjectByName('broken_arch')! as THREE.Mesh;
  brokenArches.material = archesMaterial;

  const fins = loadedWorld.getObjectByName('fins')! as THREE.Mesh;
  fins.material = buildCustomShader(
    { color: new THREE.Color(0x444444), roughness: 0.4, metalness: 0.9 },
    {},
    {}
  );

  const loader = new THREE.ImageBitmapLoader();
  const bridgeTexture = await loadTexture(loader, 'https://ameo.link/u/abu.jpg');
  const bridgeTextureNormal = await generateNormalMapFromTexture(bridgeTexture);
  const bridge = loadedWorld.getObjectByName('bridge')! as THREE.Mesh;
  bridge.material = buildCustomShader(
    {
      color: new THREE.Color(0x999999),
      roughness: 0.9,
      metalness: 0.9,
      map: bridgeTexture,
      normalMap: bridgeTextureNormal,
      normalScale: 3,
      uvTransform: new THREE.Matrix3().scale(6, 6),
    },
    { roughnessShader: groundRoughnessShader },
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

  const bridgeTopMist = loadedWorld.getObjectByName('bridge_top_mistnocollide')! as THREE.Mesh;
  const bridgeTopMistMat = buildCustomShader(
    { roughness: 0.8, metalness: 0, alphaTest: 0.5, transparent: true },
    { colorShader: BridgeMistColorShader },
    {}
  );
  bridgeTopMist.material = bridgeTopMistMat;
  viz.registerBeforeRenderCb(curTimeSeconds => bridgeTopMistMat.setCurTimeSeconds(curTimeSeconds));

  const sky = new Sky();
  sky.scale.setScalar(450000);
  loadedWorld.add(sky);

  const sun = new THREE.Vector3();
  const effectController = {
    turbidity: 0.8,
    rayleigh: 2.378,
    mieCoefficient: 0.005,
    mieDirectionalG: 0.7,
    elevation: 2,
    azimuth: 180,
  };

  const uniforms = sky.material.uniforms;
  uniforms['turbidity'].value = effectController.turbidity;
  uniforms['rayleigh'].value = effectController.rayleigh;
  uniforms['mieCoefficient'].value = effectController.mieCoefficient;
  uniforms['mieDirectionalG'].value = effectController.mieDirectionalG;

  const phi = THREE.MathUtils.degToRad(90 - effectController.elevation);
  const theta = THREE.MathUtils.degToRad(effectController.azimuth);

  sun.setFromSphericalCoords(1, phi, theta);

  uniforms['sunPosition'].value.copy(sun);
  sky.material.uniformsNeedUpdate = true;
  sky.material.needsUpdate = true;

  return {
    locations,
    debugPos: true,
    // spawnLocation: 'spawn',
    spawnLocation: 'gouge',
    gravity: 6,
    player: {
      jumpVelocity: 2.8,
      colliderCapsuleSize: {
        height: 0.7,
        radius: 0.35,
      },
      movementAccelPerSecond: {
        onGround: 6,
        inAir: 3,
      },
    },
  };
};
