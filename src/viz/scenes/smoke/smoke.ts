import * as THREE from 'three';

import { buildCustomShader } from 'src/viz/shaders/customShader';
import { loadTexture } from 'src/viz/textureLoading';
import type { SceneConfig } from '..';
import type { VizState } from '../..';
import { buildAndAdd3DVicsekFractal } from './3DvicsekFractal';
import { configurePostprocessing } from './postprocessing';
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
  dirLight.shadow.bias = 0.01;
  // dirLight.shadow.blurSamples = 24;
  // dirLight.shadow.radius = 200;
  dirLight.shadow.mapSize.width = 1024 * 4;
  dirLight.shadow.mapSize.height = 1024 * 4;
  dirLight.shadow.autoUpdate = true;
  dirLight.shadow.camera.near = 0.1;
  dirLight.shadow.camera.far = 300;
  dirLight.shadow.camera.left = -120;
  dirLight.shadow.camera.right = 150;
  dirLight.shadow.camera.top = 100;
  dirLight.shadow.camera.bottom = -150.0;
  dirLight.shadow.camera.updateProjectionMatrix();
  dirLight.matrixWorldNeedsUpdate = true;
  dirLight.updateMatrixWorld();
  dirLight.target.updateMatrixWorld();
  dirLight.position.copy(lightPos);
  viz.scene.add(dirLight);

  // helper for dirlight camera
  // const helper = new THREE.CameraHelper(dirLight.shadow.camera);
  // viz.scene.add(helper);

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
    { color: new THREE.Color(0x444444), transparent: true, fogMultiplier: 0.2 },
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
    { colorShader: BgMonolithColorShader },
    {
      useGeneratedUVs: true,
      randomizeUVOffset: false,
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

  buildAndAdd3DVicsekFractal(viz, new THREE.Vector3(177, -17, 180), 160, 4, goldMaterial, positions =>
    positions.filter(pos => {
      if (pos[1] < -50) {
        return false;
      }
      if (pos[0] > 184) {
        return false;
      }

      if (pos[0] < 0 && pos[2] > 140) {
        return false;
      }
      return true;
    })
  );

  configurePostprocessing(viz, dirLight);

  return {
    spawnLocation: 'spawn',
    gravity: 30,
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
      end: {
        pos: new THREE.Vector3(-1.1315797567367554, -6.251111030578613, 125.19293212890625),
        rot: new THREE.Vector3(-0.37520367320509945, 6.1739999999999196, 0),
      },
    },
    debugPos: true,
  };
};
