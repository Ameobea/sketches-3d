import { EffectComposer, EffectPass, RenderPass } from 'postprocessing';
import { GodraysEffect } from 'src/viz/shaders/godrays/GodraysEffect';
import * as THREE from 'three';

import type { SceneConfig } from '..';
import type { VizState } from '../..';

export const processLoadedScene = async (viz: VizState, loadedWorld: THREE.Group): Promise<SceneConfig> => {
  viz.renderer.shadowMap.enabled = true;
  viz.renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  viz.renderer.shadowMap.autoUpdate = true;

  const lightPos = new THREE.Vector3(0, 0, 0);

  const pointLight = new THREE.PointLight(0xffffff, 1, 10000);
  pointLight.castShadow = true;
  // pointLight.shadow.bias = -0.005;
  pointLight.shadow.mapSize.width = 1024;
  pointLight.shadow.mapSize.height = 1024;
  pointLight.shadow.autoUpdate = true;
  pointLight.shadow.camera.near = 0.1;
  pointLight.shadow.camera.far = 1000;
  // pointLight.shadow.camera.fov = 90;
  pointLight.shadow.camera.updateProjectionMatrix();
  pointLight.position.copy(lightPos);
  viz.scene.add(pointLight);

  const cube = new THREE.Mesh(
    new THREE.BoxGeometry(1, 10, 1),
    new THREE.MeshStandardMaterial({ color: 0xff0000 })
  );
  cube.castShadow = true;
  cube.receiveShadow = true;
  cube.position.set(10, 0, 0);
  viz.scene.add(cube);

  // Render the scene once to populate the shadow map
  viz.renderer.render(viz.scene, viz.camera);

  const lightSphere = new THREE.Mesh(
    new THREE.SphereGeometry(0.5, 16, 16),
    new THREE.MeshBasicMaterial({ color: 0xffff00 })
  );
  lightSphere.position.copy(lightPos);
  lightSphere.castShadow = false;
  lightSphere.receiveShadow = false;
  viz.scene.add(lightSphere);

  const shadowReceiver = new THREE.Mesh(
    new THREE.BoxGeometry(6, 6, 6),
    new THREE.MeshStandardMaterial({ color: 0x00ff00 })
  );
  shadowReceiver.position.set(30, 0, 0);
  shadowReceiver.castShadow = true;
  shadowReceiver.receiveShadow = true;
  viz.scene.add(shadowReceiver);

  // Add 50 random cubes in the range of [30, -150, 30] to [150, 150, 150]
  for (let i = 0; i < 50; i++) {
    const cube = new THREE.Mesh(
      new THREE.BoxGeometry(20, 20, 20),
      new THREE.MeshStandardMaterial({ color: 0x00ff00 })
    );
    cube.castShadow = true;
    cube.receiveShadow = true;
    cube.position.set(Math.random() * 120 + 30, Math.random() * 300 - 150, Math.random() * 120 + 30);
    viz.scene.add(cube);
  }

  const shadowReceiver2 = new THREE.Mesh(
    new THREE.BoxGeometry(20, 20, 20),
    new THREE.MeshStandardMaterial({ color: 0x00ffff })
  );
  shadowReceiver2.position.set(70, 0, 0);
  shadowReceiver2.castShadow = true;
  shadowReceiver2.receiveShadow = true;
  viz.scene.add(shadowReceiver2);

  const backdrop = new THREE.Mesh(
    new THREE.BoxGeometry(10, 1000, 1000),
    new THREE.MeshStandardMaterial({ color: 0x0000ff })
  );
  backdrop.position.set(200, 0, 0);
  backdrop.castShadow = true;
  backdrop.receiveShadow = true;
  viz.scene.add(backdrop);

  const backdrop2 = new THREE.Mesh(
    new THREE.BoxGeometry(10, 1000, 1000),
    new THREE.MeshStandardMaterial({ color: 0x50506f })
  );
  backdrop2.position.set(-200, 0, 0);
  backdrop2.castShadow = true;
  backdrop2.receiveShadow = true;
  viz.scene.add(backdrop2);

  const effectComposer = new EffectComposer(viz.renderer);
  const renderPass = new RenderPass(viz.scene, viz.camera);
  const depthTexture = new THREE.DepthTexture(
    viz.renderer.domElement.width,
    viz.renderer.domElement.height,
    THREE.FloatType
  );
  renderPass.setDepthTexture(depthTexture);
  renderPass.renderToScreen = false;
  effectComposer.addPass(renderPass);

  const color = new THREE.Color(0xffffff);
  const edgeRadius = 2;
  const edgeStrength = 2;
  const distanceAttenuation = 0.005;
  const density = 1 / 128;
  // const density = 1 / 64;
  const maxDensity = 0.8;

  console.log(pointLight.shadow.map.texture);
  const godraysEffect = new GodraysEffect(
    {
      // scene: viz.scene,
      // camera: viz.camera,
      lightPos,
      cameraPos: viz.camera.position,
      // cameraNear: viz.camera.near,
      // cameraFar: viz.camera.far,
      cameraNear: pointLight.shadow.camera.near,
      cameraFar: pointLight.shadow.camera.far,
      depthCube: pointLight.shadow.map.texture as THREE.CubeTexture,
      mapSize: pointLight.shadow.mapSize.height,
      projectionMatrixInv: viz.camera.projectionMatrixInverse,
      // resolution: new THREE.Vector2(1, 1),
      viewMatrixInv: viz.camera.matrixWorld,
    },
    { color, edgeRadius, edgeStrength, distanceAttenuation, density, maxDensity }
  );
  // const godraysEffectPass = new EffectPass(viz.camera, godraysEffect);
  // godraysEffectPass.renderToScreen = true;
  godraysEffect.renderToScreen = true;
  godraysEffect.needsDepthTexture = true;
  // godraysEffect.setDepthTexture(depthTexture);
  effectComposer.addPass(godraysEffect);

  viz.setRenderOverride(timeDiffSeconds => {
    effectComposer.render(timeDiffSeconds);
  });

  viz.registerBeforeRenderCb(() => {
    // godraysEffect.setParams({});
  });

  return {
    viewMode: { type: 'orbit', pos: new THREE.Vector3(10, 10, 10), target: new THREE.Vector3(0, 0, 0) },
    locations: { spawn: { pos: new THREE.Vector3(0, 0, 0), rot: new THREE.Vector3() } },
    spawnLocation: 'spawn',
  };
};
