import { N8AOPostPass } from 'n8ao';
import { EffectComposer, EffectPass, KernelSize, RenderPass, SMAAEffect, SMAAPreset } from 'postprocessing';
import * as THREE from 'three';
import { GodraysPass, type GodraysPassParams } from 'three-good-godrays';

import { buildCustomShader } from 'src/viz/shaders/customShader';
import { loadTexture } from 'src/viz/textureLoading';
import type { SceneConfig } from '..';
import type { VizState } from '../..';
import BgMonolithColorShader from './shaders/bgMonolith/color.frag?raw';

export const processLoadedScene = async (viz: VizState, loadedWorld: THREE.Group): Promise<SceneConfig> => {
  viz.scene.add(loadedWorld);

  const LIGHT_COLOR = 0xa14e0b;
  const ambientLight = new THREE.AmbientLight(LIGHT_COLOR, 0.41);
  viz.scene.add(ambientLight);

  const loader = new THREE.ImageBitmapLoader();
  const buildingTexture = await loadTexture(loader, 'https://i.ameo.link/bdu.jpg');
  const building = loadedWorld.getObjectByName('Cube') as THREE.Mesh;
  building.material = buildCustomShader(
    {
      color: new THREE.Color(0x999999),
      map: buildingTexture,
      roughness: 0.4,
      metalness: 0.4,
      uvTransform: new THREE.Matrix3().scale(0.0482, 0.0482),
    },
    {},
    {
      useGeneratedUVs: true,
      randomizeUVOffset: true,
      // tileBreaking: { type: 'neyret', patchScale: 1 },
    }
  );

  viz.renderer.shadowMap.enabled = true;
  viz.renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  viz.renderer.shadowMap.needsUpdate = true;

  const lightPos = new THREE.Vector3(-32, 27, -32);

  const dirLight = new THREE.DirectionalLight(LIGHT_COLOR, 1);
  dirLight.target.position.set(22, -2, 10);
  dirLight.castShadow = true;
  dirLight.shadow.bias = 0.005;
  // dirLight.shadow.blurSamples = 24;
  // dirLight.shadow.radius = 200;
  dirLight.shadow.mapSize.width = 1024 * 2;
  dirLight.shadow.mapSize.height = 1024 * 2;
  dirLight.shadow.autoUpdate = true;
  dirLight.shadow.camera.near = 0.1;
  dirLight.shadow.camera.far = 200;
  dirLight.shadow.camera.left = -80;
  dirLight.shadow.camera.right = 150;
  dirLight.shadow.camera.top = 100;
  dirLight.shadow.camera.bottom = -50.0;
  dirLight.shadow.camera.updateProjectionMatrix();
  dirLight.matrixWorldNeedsUpdate = true;
  dirLight.updateMatrixWorld();
  dirLight.target.updateMatrixWorld();
  dirLight.position.copy(lightPos);
  viz.scene.add(dirLight);

  viz.scene.fog = new THREE.Fog(LIGHT_COLOR, 0.02, 200);
  viz.scene.background = new THREE.Color(0x8f4509);

  // Render the scene once to populate the shadow map
  viz.renderer.render(viz.scene, viz.camera);

  // const lightSphere = new THREE.Mesh(
  //   new THREE.SphereGeometry(0.5, 16, 16),
  //   new THREE.MeshBasicMaterial({ color: 0xffff00 })
  // );
  // lightSphere.position.copy(lightPos);
  // lightSphere.castShadow = false;
  // lightSphere.receiveShadow = false;
  // viz.scene.add(lightSphere);

  const bgMonoliths = loadedWorld.children.filter(c => c.name.startsWith('bg_monolith')) as THREE.Mesh[];
  const bgMonolithMaterial = buildCustomShader(
    { color: new THREE.Color(0xffffff), transparent: true, fogMultiplier: 0.2 },
    { colorShader: BgMonolithColorShader },
    { enableFog: true }
  );
  bgMonoliths.forEach(m => {
    m.material = bgMonolithMaterial;
  });

  const border = loadedWorld.getObjectByName('border') as THREE.Mesh;
  const goldTextureAlbedo = await loadTexture(loader, 'https://i.ameo.link/be0.jpg');
  const goldTextureNormal = await loadTexture(loader, 'https://i.ameo.link/be2.jpg');
  const goldTextureRoughness = await loadTexture(loader, 'https://i.ameo.link/bdz.jpg');
  const goldMaterial = buildCustomShader(
    {
      color: new THREE.Color(0xffffff),
      map: goldTextureAlbedo,
      normalMap: goldTextureNormal,
      roughnessMap: goldTextureRoughness,
      uvTransform: new THREE.Matrix3().scale(0.8982, 0.8982),
      roughness: 0.7,
      metalness: 0.7,
    },
    {},
    {
      useGeneratedUVs: true,
      randomizeUVOffset: true,
      tileBreaking: { type: 'neyret', patchScale: 3.5 },
    }
  );
  border.material = goldMaterial;

  const brokenPillar = loadedWorld.getObjectByName('broken_monolith') as THREE.Mesh;
  brokenPillar.material = buildCustomShader(
    {
      color: new THREE.Color(0xffffff),
      map: goldTextureAlbedo,
      normalMap: goldTextureNormal,
      roughnessMap: goldTextureRoughness,
      uvTransform: new THREE.Matrix3().scale(40.8982, 40.8982),
      roughness: 0.7,
      metalness: 0.7,
    },
    {},
    {
      tileBreaking: { type: 'neyret', patchScale: 3.5 },
    }
  );

  const rectArea = new THREE.RectAreaLight(LIGHT_COLOR, 28.8, 40, 1);
  rectArea.position.set(-25, 8, 28);
  rectArea.rotation.set(0, 0, 0);
  rectArea.lookAt(rectArea.position.x, -rectArea.position.y, rectArea.position.z);
  viz.scene.add(rectArea);

  const ceilingRailings = loadedWorld.getObjectByName('ceiling_railings') as THREE.Mesh;
  ceilingRailings.material = buildCustomShader(
    {
      color: new THREE.Color(LIGHT_COLOR),
      metalness: 1,
    },
    {},
    {}
  );

  configurePostprocessing(viz, dirLight);

  return {
    spawnLocation: 'spawn',
    gravity: 60,
    player: {
      movementAccelPerSecond: { onGround: 9, inAir: 9 },
      colliderCapsuleSize: { height: 2.2, radius: 0.8 },
      jumpVelocity: 12,
    },
    locations: {
      spawn: {
        pos: new THREE.Vector3(0, 0, 0),
        rot: new THREE.Vector3(-0.01, 1.412, 0),
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

function configurePostprocessing(viz: VizState, dirLight: THREE.DirectionalLight) {
  const effectComposer = new EffectComposer(viz.renderer, { multisampling: 2 });
  const renderPass = new RenderPass(viz.scene, viz.camera);
  effectComposer.addPass(renderPass);

  const godraysParams: GodraysPassParams = {
    color: new THREE.Color().copy(dirLight.color),
    edgeRadius: 4,
    edgeStrength: 1,
    distanceAttenuation: 1,
    density: 1 / 8,
    maxDensity: 1,
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
  n8aoPass.configuration.intensity = 7;
  n8aoPass.configuration.aoRadius = 9;
  n8aoPass.configuration.halfRes = false;
  effectComposer.addPass(n8aoPass);

  const godraysEffect = new GodraysPass(dirLight, viz.camera, godraysParams);
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
  effectComposer.addPass(smaaPass2);

  viz.setRenderOverride(timeDiffSeconds => {
    effectComposer.render(timeDiffSeconds);
    viz.renderer.shadowMap.autoUpdate = false;
  });

  viz.renderer.toneMapping = THREE.CineonToneMapping;
  viz.renderer.toneMappingExposure = 1.8;
}
