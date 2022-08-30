import * as THREE from 'three';
import { Sky } from 'three/examples/jsm/objects/Sky';

import type { VizState } from '../../../viz';
import { initBaseScene } from '../../../viz/util';
import type { SceneConfig } from '..';
import { buildCustomShader } from '../../../viz/shaders/customShader';
import groundRoughnessShader from '../../shaders/subdivided/ground/roughness.frag?raw';

const locations = {
  spawn: {
    pos: new THREE.Vector3(-1.7557428208542067, 2, -0.57513478883080035),
    rot: new THREE.Vector3(-0.05999999999999997, -1.514, 0),
  },
};

export const processLoadedScene = async (viz: VizState, loadedWorld: THREE.Group): Promise<SceneConfig> => {
  const base = initBaseScene(viz);
  base.light.castShadow = false;
  base.light.intensity += 0.5;

  const bridgeTop = loadedWorld.getObjectByName('bridge_top')! as THREE.Mesh;
  const mat = bridgeTop.material as THREE.MeshStandardMaterial;
  const texture = mat.emissiveMap!;
  mat.emissiveMap = null;
  mat.emissive = new THREE.Color(0x0);

  // This is necessary to deal with issue with GLTF exports and Three.js.
  //
  // Three.JS expects the UV map for light map to be in `uv2` but the GLTF
  // exporter puts it in `uv1`.
  const geometry = bridgeTop.geometry;
  geometry.attributes.uv2 = geometry.attributes.uv;

  bridgeTop.material = buildCustomShader(
    { color: new THREE.Color(0x121212), lightMap: texture, lightMapIntensity: 2 },
    { roughnessShader: groundRoughnessShader },
    {}
  );

  const ex = new THREE.CapsuleGeometry(0.35, 1.8, 24, 24);
  const exMesh = new THREE.Mesh(ex, new THREE.MeshStandardMaterial({ color: 0xff0000 }));
  exMesh.position.set(-1.7, 2, -0.6);
  loadedWorld.add(exMesh);

  const arches = loadedWorld.getObjectByName('arch')! as THREE.Mesh;
  arches.material = buildCustomShader(
    { color: new THREE.Color(0x444444), roughness: 0.4, metalness: 0.9 },
    {},
    {}
  );

  const fins = loadedWorld.getObjectByName('fins')! as THREE.Mesh;
  fins.material = buildCustomShader(
    { color: new THREE.Color(0x444444), roughness: 0.4, metalness: 0.9 },
    {},
    {}
  );

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
    spawnLocation: 'spawn',
    gravity: 6,
    player: {
      jumpVelocity: 2.8,
      colliderCapsuleSize: {
        height: 1.8,
        radius: 0.35,
      },
      movementAccelPerSecond: {
        onGround: 2,
        inAir: 1,
      },
    },
  };
};
