import { EffectComposer, EffectPass, RenderPass } from 'postprocessing';
import * as THREE from 'three';
import { GodraysPass, type GodraysPassParams } from 'three-good-godrays';

import { buildCustomShader } from 'src/viz/shaders/customShader';
import { generateNormalMapFromTexture, loadTexture } from 'src/viz/textureLoading';
import type { SceneConfig } from '../..';
import type { Viz } from '../../..';

export const processLoadedScene = async (viz: Viz, loadedWorld: THREE.Group): Promise<SceneConfig> => {
  const loader = new THREE.ImageBitmapLoader();
  const dungeonWallTextureP = loadTexture(loader, 'https://i.ameo.link/akz.jpg');
  const dungeonWallTextureCombinedDiffuseNormalTextureP = dungeonWallTextureP.then(dungeonWallTexture =>
    generateNormalMapFromTexture(dungeonWallTexture, {}, true)
  );
  const dungeonWallTextureCombinedDiffuseNormalTexture =
    await dungeonWallTextureCombinedDiffuseNormalTextureP;

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
    new THREE.MeshBasicMaterial({ color: 0x00ff00 })
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
  backdrop.position.set(400, 0, 0);
  backdrop.castShadow = true;
  backdrop.receiveShadow = true;
  viz.scene.add(backdrop);

  const backdrop2 = new THREE.Mesh(
    new THREE.BoxGeometry(10, 1000, 1000),
    buildCustomShader(
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
    )
  );
  backdrop2.position.set(-200, 0, 0);
  backdrop2.castShadow = true;
  backdrop2.receiveShadow = true;
  viz.scene.add(backdrop2);

  const effectComposer = new EffectComposer(viz.renderer, { frameBufferType: THREE.HalfFloatType });
  const renderPass = new RenderPass(viz.scene, viz.camera);
  renderPass.renderToScreen = false;
  effectComposer.addPass(renderPass);

  const godraysParams: GodraysPassParams = {
    color: new THREE.Color(0xffffff),
    edgeRadius: 2,
    edgeStrength: 2,
    distanceAttenuation: 0.006,
    density: 1 / 126,
    maxDensity: 1,
    blur: false,
    raymarchSteps: 100,
  };

  const godraysEffect = new GodraysPass(pointLight, viz.camera, godraysParams);
  godraysEffect.renderToScreen = true;
  effectComposer.addPass(godraysEffect);

  setInterval(() => {
    godraysParams.color.setHex(Math.random() * 0xffffff);
    pointLight.color.copy(godraysParams.color);
    godraysEffect.setParams(godraysParams);
  }, 3000);

  viz.registerBeforeRenderCb(curTimeSeconds => {
    pointLight.position.y = Math.sin(curTimeSeconds * 0.5) * 20;
    lightSphere.position.copy(pointLight.position);
  });

  viz.setRenderOverride(timeDiffSeconds => {
    effectComposer.render(timeDiffSeconds);
  });

  return {
    viewMode: {
      type: 'orbit',
      pos: new THREE.Vector3(295.7257487200072, -29.58650677773067, 230.2576364577206),
      target: new THREE.Vector3(0, 0, 0),
    },
    locations: {
      spawn: {
        pos: new THREE.Vector3(295.7257487200072, -29.58650677773067, 230.2576364577206),
        rot: new THREE.Vector3(),
      },
    },
    spawnLocation: 'spawn',
  };
};
