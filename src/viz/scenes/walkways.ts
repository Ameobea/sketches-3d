import * as THREE from 'three';

import type { SceneConfig } from '.';
import type { Viz } from '..';
import { buildCustomShaderArgs } from '../shaders/customShader';
import walkwayColorShader from '../shaders/walkway/color.frag?raw';
import walkwayRoughnessShader from '../shaders/walkway/roughness.frag?raw';
import { initBaseScene } from '../util/three';

const conf = {
  platformCount: 30,
  platformSpacing: 45,
  platformWidth: 14.5,
  platformLength: 10_000,
  platformHeight: 90,

  conduitHeight: 8,
  conduitWidth: 8,
  conduitLength: 10_000,
  conduitHeightOffset: 30,
};

const buildPlatforms = () => {
  const platforms = new THREE.Group();
  platforms.name = 'platforms';

  const conduits = new THREE.Group();
  conduits.name = 'conduits';

  const platformRange = conf.platformCount * (conf.platformSpacing + conf.platformWidth);
  const platformStart = -platformRange / 2;

  const platformMaterial = new THREE.ShaderMaterial(
    buildCustomShaderArgs(
      {
        roughness: 0,
        metalness: 0.95,
        color: new THREE.Color(0xffffff),
        normalScale: 0.5,
      },
      { roughnessShader: walkwayRoughnessShader, colorShader: walkwayColorShader },
      { antialiasColorShader: true }
    )
  );

  const conduitMaterial = new THREE.MeshStandardMaterial({
    color: 0x050505,
  });

  for (let xIx = 0; xIx < conf.platformCount; xIx++) {
    // walkway
    const x = platformStart + xIx * (conf.platformSpacing + conf.platformWidth);
    const walkwayGeometry = new THREE.BoxGeometry(
      conf.platformWidth,
      conf.platformHeight,
      conf.platformLength,
      100,
      1,
      100
    );
    const walkwayMesh = new THREE.Mesh(walkwayGeometry, platformMaterial);
    walkwayMesh.position.set(x, conf.platformHeight / 2 + Math.abs(x) * -0.0, 0);
    platforms.add(walkwayMesh);

    // conduit
    if (xIx % 2 === 0) {
      // continue;
    }

    const conduitGeometry = new THREE.BoxGeometry(conf.conduitWidth, conf.conduitHeight, conf.conduitLength);
    const conduitMesh = new THREE.Mesh(conduitGeometry, platformMaterial);
    conduitMesh.position.set(x, conf.platformHeight + conf.conduitHeightOffset, 0);
    conduits.add(conduitMesh);
  }

  for (let zIx = 0; zIx < conf.platformCount; zIx++) {
    const z = platformStart + zIx * (conf.platformSpacing + conf.platformWidth);
    const geometry = new THREE.BoxGeometry(
      conf.platformLength,
      conf.platformHeight,
      conf.platformWidth,
      100,
      1,
      100
    );
    const mesh = new THREE.Mesh(
      geometry,
      // platformMaterial
      conduitMaterial
    );
    // Rotate 90 degrees about the y axis
    // mesh.rotation.y = Math.PI / 2;
    mesh.position.set(0, -2 + conf.platformHeight / 2 + Math.abs(z) * -0.0 - 0.05, z);
    platforms.add(mesh);
  }

  return { platforms, conduits };
};

export const processLoadedScene = (viz: Viz, loadedWorld: THREE.Group): SceneConfig => {
  const baseScene = initBaseScene(viz);
  baseScene.light.intensity = 0.0;
  baseScene.ambientlight.intensity = 0.2;
  // viz.scene.fog = new THREE.FogExp2(0x0, 0.001);

  // const light = new THREE.DirectionalLight(0xffffff, 0.5);
  // light.position.set(-80, 160, -80);
  // viz.scene.add(light);

  const pointLight = new THREE.PointLight(0x8888ff, 0.8);
  pointLight.position.set(0, 0, 0);
  viz.scene.add(pointLight);

  setTimeout(() => {
    const newPos = viz.camera.position.clone();
    newPos.y += 10;
    pointLight.position.copy(newPos);
    const cube = new THREE.Mesh(
      new THREE.BoxGeometry(1, 1, 1),
      new THREE.MeshBasicMaterial({ color: 0xffffff })
    );
    cube.position.copy(newPos);
    viz.scene.add(cube);
  }, 1000);

  viz.registerBeforeRenderCb((curTimeSeconds: number) => {
    // if (Math.sin(curTimeSeconds) > 0) {
    //   pointLight.color = new THREE.Color(0xff0000);
    // } else {
    //   pointLight.color = new THREE.Color(0x8888ff);
    // }
    // pointLight.position.y += 10;
    // light.position.x = Math.sin(curTimeSeconds * 0.3) * 80;
    // light.position.z = Math.cos(curTimeSeconds * 0.3) * 280;
    // baseScene.light.position.x = Math.sin(curTimeSeconds * 0.3) * -280;
    // baseScene.light.position.z = Math.cos(curTimeSeconds * 0.3) * -80;
  });

  const { platforms, conduits } = buildPlatforms();
  loadedWorld.add(platforms);
  loadedWorld.add(conduits);

  const mats = [...platforms.children, ...conduits.children].map(
    m => (m as THREE.Mesh).material as THREE.ShaderMaterial
  );
  viz.registerBeforeRenderCb(curTimeSeconds => {
    for (const material of mats) {
      if (material instanceof THREE.ShaderMaterial) {
        material.uniforms.curTimeSeconds.value = curTimeSeconds;
      }
    }
  });

  return {
    locations: { spawn: { pos: new THREE.Vector3(0, 92, 0), rot: new THREE.Vector3(-0.022, 1.488, 0) } },
    spawnLocation: 'spawn',
    // viewMode: { type: 'orbit', pos: new THREE.Vector3(150, 100, 0), target: new THREE.Vector3(0, 100, 0) },
    debugPos: true,
  };
};
