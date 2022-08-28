import * as THREE from 'three';

import type { SceneConfig } from '.';
import type { VizState } from '..';
import { buildCustomShader } from '../shaders/customShader';
import { initBaseScene } from '../util';
import pillarColorShader from '../shaders/subdivided/pillar/color.frag?raw';
import pillarRoghnessShader from '../shaders/subdivided/pillar/roughness.frag?raw';
import PillarVertexShaderFragment from '../shaders/subdivided/pillar/displacement.vert?raw';
import groundRoughnessShader from '../shaders/subdivided/ground/roughness.frag?raw';
import { generateNormalMapFromTexture } from '../shaders/normalMapGeneration';

const locations = {
  spawn: { pos: new THREE.Vector3(52.7, 1.35, -5.515), rot: new THREE.Vector3(0.51, 1.65, 0) },
};

export const processLoadedScene = async (viz: VizState, loadedWorld: THREE.Group): Promise<SceneConfig> => {
  const enginePromise = import('../wasmComp/engine');

  const content = document.createElement('div');
  content.id = 'content';
  content.style.display = 'none';
  document.body.appendChild(content);
  import('https://ameo.dev/web-synth-headless/headless.js').then(async mod => {
    const webSynthHandle = await mod.initHeadlessWebSynth();
    console.log(webSynthHandle);
  });

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
  const groundTexture = await new Promise<THREE.Texture>((resolve, reject) =>
    loader.load(
      'https://ameo.link/u/aau.jpg',
      imageBitmap => {
        console.log('loaded');
        const texture = new THREE.CanvasTexture(
          imageBitmap,
          THREE.UVMapping,
          THREE.RepeatWrapping,
          THREE.RepeatWrapping,
          THREE.NearestFilter,
          THREE.NearestFilter
        );
        texture.needsUpdate = true;
        resolve(texture);
      },
      undefined,
      reject
    )
  );

  // const groundNormalTexture = await new Promise<THREE.Texture>((resolve, reject) =>
  //   loader.load(
  //     'https://ameo.link/u/aav.jpg',
  //     imageBitmap => {
  //       console.log('loaded');
  //       const texture = new THREE.CanvasTexture(
  //         imageBitmap,
  //         THREE.UVMapping,
  //         THREE.RepeatWrapping,
  //         THREE.RepeatWrapping,
  //         THREE.LinearFilter,
  //         THREE.LinearFilter
  //       );
  //       texture.needsUpdate = true;
  //       resolve(texture);
  //     },
  //     undefined,
  //     reject
  //   )
  // );
  const engine = await enginePromise;
  await engine.default();
  const groundNormalTexture = generateNormalMapFromTexture(engine, groundTexture);

  const ground = loadedWorld.getObjectByName('ground001')! as THREE.Mesh;
  const groundMaterial = new THREE.ShaderMaterial(
    buildCustomShader(
      { color: new THREE.Color(0x3232cc), metalness: 0.95, roughness: 0.2 },
      { roughnessShader: groundRoughnessShader },
      { antialiasRoughnessShader: true }
    )
  );
  const groundUVTransform = new THREE.Matrix3().scale(3 * 32, 4 * 32);
  groundMaterial.uniforms.uvTransform.value = groundUVTransform;
  groundMaterial.map = groundTexture;
  groundMaterial.normalMap = groundNormalTexture;
  groundMaterial.normalMapType = THREE.TangentSpaceNormalMap;
  groundMaterial.uniforms.map.value = groundTexture;
  groundMaterial.uniforms.normalMap.value = groundNormalTexture;
  groundMaterial.needsUpdate = true;
  ground.material = groundMaterial;

  const texture = await new Promise<THREE.Texture>((resolve, reject) =>
    loader.load(
      // 'https://ameo.link/u/aaj.jpg',
      // 'https://ameo.link/u/aai.jpg',
      // 'https://ameo.link/u/aal.jpg',
      // 'https://ameo.link/u/aam.jpg',
      'https://ameo.link/u/aap.jpg',
      // 'https://ameo.link/u/aaq.jpg', // GOOD
      // 'https://ameo.link/u/aar.jpg',
      // 'https://ameo.link/u/aas.jpg',
      // 'https://ameo.link/u/aat.jpg',
      // 'https://ameo.link/u/aau.jpg',
      // 'https://ameo.link/u/aaw.jpg',
      // 'https://ameo.link/u/aax.png',
      imageBitmap => {
        console.log('loaded');
        const texture = new THREE.CanvasTexture(
          imageBitmap,
          THREE.UVMapping,
          THREE.RepeatWrapping,
          THREE.RepeatWrapping,
          THREE.LinearFilter,
          THREE.LinearFilter
        );
        texture.needsUpdate = true;
        resolve(texture);
      },
      undefined,
      reject
    )
  );
  loader.manager = new THREE.LoadingManager();
  const normalTexture = await new Promise<THREE.Texture>((resolve, reject) =>
    loader.load(
      // 'https://ameo.link/u/aaa.png',
      'https://ameo.link/u/aak.jpg',
      // 'https://ameo.link/u/aay.png',
      imageBitmap => {
        console.log('loaded normal map');
        const texture = new THREE.CanvasTexture(
          imageBitmap,
          THREE.UVMapping,
          THREE.RepeatWrapping,
          THREE.RepeatWrapping,
          // THREE.LinearFilter,
          THREE.NearestFilter,
          // THREE.LinearFilter,
          THREE.NearestFilter,
          THREE.RGBAFormat
        );
        texture.needsUpdate = true;
        resolve(texture);
      },
      undefined,
      reject
    )
  );

  const pillar = loadedWorld.getObjectByName('pillar')! as THREE.Mesh;
  const pillarMaterial = new THREE.ShaderMaterial(
    buildCustomShader(
      { metalness: 0.96, normalScale: 1, color: new THREE.Color(0xffffff) },
      {
        customVertexFragment: PillarVertexShaderFragment,
        colorShader: pillarColorShader,
        roughnessShader: pillarRoghnessShader,
      },
      {}
    )
  );
  pillarMaterial.map = texture;
  pillarMaterial.normalMap = normalTexture;
  pillarMaterial.normalMapType = THREE.TangentSpaceNormalMap;
  pillarMaterial.uniforms.map.value = texture;
  pillarMaterial.uniforms.normalMap.value = normalTexture;
  const pillarUVTransform = new THREE.Matrix3().scale(3 * 1, 4 * 1);
  pillarMaterial.uniforms.uvTransform.value = pillarUVTransform;
  pillarMaterial.needsUpdate = true;
  pillar.material = pillarMaterial;
  pillar.scale.set(0.8, 0.6, 0.8);
  pillar.geometry = new THREE.CylinderGeometry(20, 20, 220, 64, 64);
  pillar.rotation.x = 0.2;

  viz.registerBeforeRenderCb(curTimeSeconds => {
    pillarMaterial.uniforms.curTimeSeconds.value = curTimeSeconds;
    groundMaterial.uniforms.curTimeSeconds.value = curTimeSeconds;
    pillar.rotation.y = curTimeSeconds * 0.1;

    // rOTATE point light about the pillar
    pointLight.position.x = Math.sin(curTimeSeconds * -0.3) * 20;
    pointLight.position.z = Math.cos(curTimeSeconds * -0.3) * 20;
    pointLightCube.position.copy(pointLight.position);
  });

  return { locations, spawnLocation: 'spawn' };
};
