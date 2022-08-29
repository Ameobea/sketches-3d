import * as THREE from 'three';

import type { SceneConfig } from '.';
import type { VizState } from '..';
import { buildCustomShader } from '../shaders/customShader';
import { initBaseScene } from '../util';
import pillarColorShader from '../shaders/subdivided/pillar/color.frag?raw';
import pillarRoghnessShader from '../shaders/subdivided/pillar/roughness.frag?raw';
import PillarVertexShaderFragment from '../shaders/subdivided/pillar/displacement.vert?raw';
import groundRoughnessShader from '../shaders/subdivided/ground/roughness.frag?raw';
import { generateNormalMapFromTexture, loadTexture } from '../textureLoading';

const locations = {
  spawn: { pos: new THREE.Vector3(52.7, 1.35, -5.515), rot: new THREE.Vector3(0.51, 1.65, 0) },
};

export const processLoadedScene = async (viz: VizState, loadedWorld: THREE.Group): Promise<SceneConfig> => {
  const baseScene = initBaseScene(viz);
  baseScene.light.color = new THREE.Color(0xffffff);
  baseScene.light.intensity = 0.6;
  baseScene.ambientlight.intensity = 5.4;

  const pointLight = new THREE.PointLight(0x228888, 8, 100);
  pointLight.position.set(-40, 18, 15);
  viz.scene.add(pointLight);
  const pointLightCube = new THREE.Mesh(
    new THREE.BoxGeometry(1, 1, 1),
    new THREE.MeshBasicMaterial({ color: 0x228888 })
  );
  pointLightCube.position.copy(pointLight.position);
  viz.scene.add(pointLightCube);

  const loader = new THREE.ImageBitmapLoader();
  const groundTexture = await loadTexture(loader, 'https://ameo.link/u/aau.jpg');

  const groundNormalTexture = await generateNormalMapFromTexture(groundTexture);
  console.log({ groundNormalTexture });

  const ground = loadedWorld.getObjectByName('ground001')! as THREE.Mesh;
  const groundUVTransform = new THREE.Matrix3().scale(3 * 32, 4 * 32);
  const groundMaterial = buildCustomShader(
    {
      color: new THREE.Color(0x3232cc),
      metalness: 0.95,
      roughness: 0.2,
      map: groundTexture,
      normalMap: groundNormalTexture,
      uvTransform: groundUVTransform,
      normalScale: 4,
    },
    { roughnessShader: groundRoughnessShader },
    { antialiasRoughnessShader: true }
  );

  ground.material = groundMaterial;

  const texture = await loadTexture(
    loader,
    // 'https://ameo.link/u/aaj.jpg',
    // 'https://ameo.link/u/aai.jpg',
    // 'https://ameo.link/u/aal.jpg',
    // 'https://ameo.link/u/aam.jpg',
    'https://ameo.link/u/aap.jpg'
    // 'https://ameo.link/u/aaq.jpg', // GOOD
    // 'https://ameo.link/u/aar.jpg',
    // 'https://ameo.link/u/aas.jpg',
    // 'https://ameo.link/u/aat.jpg',
    // 'https://ameo.link/u/aau.jpg',
    // 'https://ameo.link/u/aaw.jpg',
    // 'https://ameo.link/u/aax.png',
  );
  // loader.manager = new THREE.LoadingManager();
  const normalTexture = await loadTexture(
    loader,
    // 'https://ameo.link/u/aaa.png',
    'https://ameo.link/u/aak.jpg'
    // 'https://ameo.link/u/aay.png',
  );

  const pillar = loadedWorld.getObjectByName('pillar')! as THREE.Mesh;
  const pillarMaterial = buildCustomShader(
    {
      metalness: 0.96,
      normalScale: 1,
      color: new THREE.Color(0xffffff),
      map: texture,
      normalMap: normalTexture,
      uvTransform: new THREE.Matrix3().scale(3 * 1, 4 * 1),
    },
    {
      customVertexFragment: PillarVertexShaderFragment,
      colorShader: pillarColorShader,
      roughnessShader: pillarRoghnessShader,
    },
    {}
  );
  pillar.material = pillarMaterial;
  pillar.scale.set(0.8, 0.6, 0.8);
  pillar.geometry = new THREE.CylinderGeometry(20, 20, 220, 64, 64);
  pillar.rotation.x = 0.2;

  viz.registerBeforeRenderCb(curTimeSeconds => {
    pillarMaterial.uniforms.curTimeSeconds.value = curTimeSeconds;
    groundMaterial.uniforms.curTimeSeconds.value = curTimeSeconds;
    pillar.rotation.y = curTimeSeconds * 0.1;

    // rotate point light about the pillar
    pointLight.position.x = Math.sin(curTimeSeconds * -0.3) * 20;
    pointLight.position.z = Math.cos(curTimeSeconds * -0.3) * 20;
    pointLightCube.position.copy(pointLight.position);
  });

  return { locations, spawnLocation: 'spawn' };
};
