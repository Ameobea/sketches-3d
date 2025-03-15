import * as THREE from 'three';
import { EffectComposer } from 'three/examples/jsm/postprocessing/EffectComposer.js';
import { RenderPass } from 'three/examples/jsm/postprocessing/RenderPass.js';
import { UnrealBloomPass } from 'three/examples/jsm/postprocessing/UnrealBloomPass.js';

import type { SceneConfig } from '.';
import type { VizState } from '..';
import bigCubeShader from '../shaders/bigCube.frag?raw';
import bridgeShader from '../shaders/bridge.frag?raw';
import { buildCustomShaderArgs } from '../shaders/customShader';
import redNoiseShader from '../shaders/redNoise.frag?raw';
import { getFlickerActivation, initBaseScene } from '../util';

const buildBridge = (viz: VizState, bridge: THREE.Mesh) => {
  bridge.material = new THREE.ShaderMaterial(
    buildCustomShaderArgs(
      {
        roughness: 0.996,
        metalness: 0.0,
        color: new THREE.Color(0x333333),
      },
      { colorShader: bridgeShader }
    )
  );
  bridge.material.needsUpdate = true;

  viz.registerBeforeRenderCb(curTimeSeconds => {
    (bridge.material as THREE.ShaderMaterial).uniforms.curTimeSeconds.value = curTimeSeconds;
  });
};

const locations = {
  spawn: {
    pos: new THREE.Vector3(48.17740050559579, 23.920086905508146, 8.603910511800485),
    rot: new THREE.Vector3(-0.022, 1.488, 0),
  },
  bigCube: {
    pos: new THREE.Vector3(-281.43973660347024, 22.754156253511294, -8.752855510181472),
    rot: new THREE.Vector3(-0.504, 1.772, 0),
  },
};

export const processLoadedScene = (viz: VizState, loadedWorld: THREE.Group): SceneConfig => {
  initBaseScene(viz);

  // Add close fog
  viz.scene.fog = new THREE.Fog(0x030303, 50, 215);

  const bridge = loadedWorld.getObjectByName('bridge') as THREE.Mesh;
  buildBridge(viz, bridge);

  const allObjectNames = loadedWorld.children.map(obj => obj.name);
  console.log('Loaded world objects:', allObjectNames);

  const bigCube = loadedWorld.getObjectByName('big_cube') as THREE.Mesh;
  bigCube.material = new THREE.MeshBasicMaterial({ color: 0x080808 });

  const bigCubeMat = new THREE.ShaderMaterial(
    buildCustomShaderArgs(
      {
        roughness: 0.96,
        metalness: 0.1,
        color: new THREE.Color(0x020202),
      },
      { colorShader: bigCubeShader }
    )
  );
  bigCubeMat.needsUpdate = true;
  bigCube.material = bigCubeMat;

  const treeMat = new THREE.ShaderMaterial(
    buildCustomShaderArgs(
      {
        roughness: 0.96,
        metalness: 0.1,
        color: new THREE.Color(0x020202),
      },
      { colorShader: redNoiseShader }
    )
  );
  treeMat.needsUpdate = true;

  viz.registerBeforeRenderCb(curTimeSeconds => {
    (bigCube.material as THREE.ShaderMaterial).uniforms.curTimeSeconds.value = curTimeSeconds;
    (treeMat as THREE.ShaderMaterial).uniforms.curTimeSeconds.value = curTimeSeconds;
  });

  loadedWorld.children.forEach(child => {
    const lowerName = child.name.toLowerCase();
    if (lowerName.startsWith('tree') || lowerName.startsWith('highlight_')) {
      const tree = child as THREE.Mesh;
      tree.material = treeMat;
    }
  });

  const pedestalTop = loadedWorld.getObjectByName('pedestal_top') as THREE.Mesh;
  pedestalTop.material = new THREE.MeshStandardMaterial({
    color: 0x080808,
    metalness: 0.8,
    roughness: 0.6,
  });

  const topGlowCube = loadedWorld.getObjectByName('top_glow_cube') as THREE.Mesh;
  const topGlowCubeMat = new THREE.MeshStandardMaterial({
    color: 0xff0000,
    emissive: 0xff0000,
    emissiveIntensity: 0.5,
  });
  topGlowCube.material = topGlowCubeMat;

  // Place point light inside the top glow cube
  const pointLight = new THREE.PointLight(0x880000, 10, 100);
  pointLight.position.copy(topGlowCube.position);
  pointLight.position.y -= 2;
  // viz.scene.add(pointLight);

  const bloomPass = new UnrealBloomPass(
    new THREE.Vector2(viz.renderer.domElement.width, viz.renderer.domElement.height),
    1.45,
    0.1,
    0.1689
  );
  const composer = new EffectComposer(viz.renderer);

  composer.addPass(new RenderPass(viz.scene, viz.camera));
  composer.addPass(bloomPass);

  viz.registerAfterRenderCb(() => {
    composer.render();
  });

  let rotSpeed = 0;
  const baseGlowCubePos = topGlowCube.position.clone();

  viz.registerBeforeRenderCb((curTimeSeconds, tDiffSeconds) => {
    const flickerActivation = Math.min(1, getFlickerActivation(curTimeSeconds) + 0.2);
    const activeColor = new THREE.Color(0x880000);
    const inactiveColor = new THREE.Color(0x000000);
    const color = activeColor
      .multiplyScalar(flickerActivation)
      .add(inactiveColor.clone().multiplyScalar(1 - flickerActivation));
    topGlowCubeMat.color.copy(color);
    topGlowCubeMat.emissive.copy(color);

    topGlowCube.position.set(
      baseGlowCubePos.x + Math.pow(Math.random() * flickerActivation, 1.5) * 0.87,
      baseGlowCubePos.y + Math.pow(Math.random() * flickerActivation, 1.5) * 0.87,
      baseGlowCubePos.z + Math.pow(Math.random() * flickerActivation, 1.5) * 0.87
    );

    pointLight.color.copy(color);

    const newRotSpeed = flickerActivation * 6.8 + 1;
    // Low-pass filter rotation speed
    rotSpeed = rotSpeed * 0.99 + newRotSpeed * 0.01;

    // Rotate the top glow cube
    topGlowCube.rotation.y += rotSpeed * tDiffSeconds;
  });

  return {
    locations,
    spawnLocation: 'spawn',
    player: {
      moveSpeed: { onGround: 9, inAir: 9 },
      colliderSize: { height: 2.2, radius: 0.3 },
      jumpVelocity: 16,
    },
  };
};
