import { N8AOPostPass } from 'n8ao';
import {
  BlendFunction,
  BloomEffect,
  EffectComposer,
  EffectPass,
  KernelSize,
  RenderPass,
  SMAAEffect,
  SMAAPreset,
} from 'postprocessing';
import * as THREE from 'three';
import { GodraysPass, type GodraysPassParams } from 'three-good-godrays';

import { buildCustomShader } from 'src/viz/shaders/customShader';
import { loadTexture } from 'src/viz/textureLoading';
import type { SceneConfig } from '..';
import type { VizState } from '../..';

export const processLoadedScene = async (viz: VizState, loadedWorld: THREE.Group): Promise<SceneConfig> => {
  viz.scene.add(loadedWorld);

  const LIGHT_COLOR = 0xa14e0b;
  const ambientLight = new THREE.AmbientLight(LIGHT_COLOR, 0.21);
  viz.scene.add(ambientLight);

  const loader = new THREE.ImageBitmapLoader();
  const buildingTexture = await loadTexture(loader, 'https://i.ameo.link/bds.jpg');
  const building = loadedWorld.getObjectByName('Cube') as THREE.Mesh;
  building.material = buildCustomShader(
    {
      map: buildingTexture,
      roughness: 0.4,
      metalness: 0.7,
      uvTransform: new THREE.Matrix3().scale(0.82, 0.82),
    },
    {},
    { useGeneratedUVs: true, randomizeUVOffset: true, tileBreaking: { type: 'neyret', patchScale: 8 } }
  );

  viz.renderer.shadowMap.enabled = true;
  viz.renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  viz.renderer.shadowMap.autoUpdate = true;

  const lightPos = new THREE.Vector3(-32, 27, -32);

  const dirLight = new THREE.DirectionalLight(LIGHT_COLOR, 1);
  dirLight.target.position.set(22, -2, 10);
  dirLight.castShadow = true;
  dirLight.shadow.bias = 0.005;
  dirLight.shadow.mapSize.width = 1024 * 2;
  dirLight.shadow.mapSize.height = 1024 * 2;
  dirLight.shadow.autoUpdate = true;
  dirLight.shadow.camera.near = 0.1;
  dirLight.shadow.camera.far = 200;
  dirLight.shadow.camera.left = -80;
  dirLight.shadow.camera.right = 150;
  dirLight.shadow.camera.top = 50;
  dirLight.shadow.camera.bottom = -50.0;
  dirLight.shadow.camera.updateProjectionMatrix();
  dirLight.matrixWorldNeedsUpdate = true;
  dirLight.updateMatrixWorld();
  dirLight.target.updateMatrixWorld();
  dirLight.position.copy(lightPos);
  viz.scene.add(dirLight);

  viz.scene.fog = new THREE.Fog(LIGHT_COLOR, 0.02, 200);

  // light helper to debug
  // const dirLightHelper = new THREE.DirectionalLightHelper(dirLight, 5);
  // viz.scene.add(dirLightHelper);

  // const pointLight = new THREE.PointLight(LIGHT_COLOR, 1, 100);
  // pointLight.castShadow = true;
  // // pointLight.shadow.bias = -0.005;
  // pointLight.shadow.mapSize.width = 1024 * 2;
  // pointLight.shadow.mapSize.height = 1024 * 2;
  // pointLight.shadow.autoUpdate = true;
  // pointLight.shadow.camera.near = 0.1;
  // pointLight.shadow.camera.far = 200;
  // pointLight.matrixWorldNeedsUpdate = true;
  // pointLight.updateMatrixWorld();
  // pointLight.position.copy(lightPos);
  // viz.scene.add(pointLight);
  // const dirLight = pointLight;

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

  const effectComposer = new EffectComposer(viz.renderer);
  const renderPass = new RenderPass(viz.scene, viz.camera);
  renderPass.renderToScreen = false;
  effectComposer.addPass(renderPass);

  const godraysParams: GodraysPassParams = {
    color: new THREE.Color().copy(dirLight.color),
    edgeRadius: 2,
    edgeStrength: 2,
    distanceAttenuation: 1,
    density: 1 / 10,
    maxDensity: 0.88,
    raymarchSteps: 60,
    blur: { kernelSize: KernelSize.LARGE, variance: 0.25 },
  };

  const n8aoPass = new N8AOPostPass(
    viz.scene,
    viz.camera,
    viz.renderer.domElement.width,
    viz.renderer.domElement.height
  );
  n8aoPass.gammaCorrection = false;
  n8aoPass.configuration.intensity = 9;
  n8aoPass.configuration.aoRadius = 9;
  n8aoPass.configuration.halfRes = false;
  effectComposer.addPass(n8aoPass);

  const godraysEffect = new GodraysPass(dirLight, viz.camera, godraysParams);
  godraysEffect.renderToScreen = true;
  effectComposer.addPass(godraysEffect);

  // const bloomEffect = new BloomEffect({
  //   intensity: 0.8,
  //   mipmapBlur: true,
  //   luminanceThreshold: 0.03,
  //   blendFunction: BlendFunction.ADD,
  //   luminanceSmoothing: 0.05,
  // });
  // const bloomPass = new EffectPass(viz.camera, bloomEffect);
  // bloomPass.dithering = false;

  // effectComposer.addPass(bloomPass);

  const smaaEffect2 = new SMAAEffect({ preset: SMAAPreset.MEDIUM });
  const smaaPass2 = new EffectPass(viz.camera, smaaEffect2);
  // smaaPass2.renderToScreen = true;
  // effectComposer.addPass(smaaPass2);

  viz.setRenderOverride(timeDiffSeconds => {
    effectComposer.render(timeDiffSeconds);
  });

  return {
    spawnLocation: 'spawn',
    player: {
      movementAccelPerSecond: { onGround: 9, inAir: 9 },
      colliderCapsuleSize: { height: 2.2, radius: 0.3 },
      jumpVelocity: 16,
    },
    locations: {
      spawn: {
        pos: new THREE.Vector3(0, 0, 0),
        rot: new THREE.Vector3(),
      },
    },
    // viewMode: {
    //   type: 'orbit',
    //   pos: new THREE.Vector3(50, 50, 50),
    //   target: new THREE.Vector3(0, 0, 0),
    // },
    debugPos: true,
  };
};
