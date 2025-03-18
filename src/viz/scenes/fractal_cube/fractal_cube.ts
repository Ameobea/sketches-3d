import * as THREE from 'three';

import type { SceneConfig } from '..';
import type { Viz } from '../..';
import bigCubeColorShader from '../../shaders/bigCube.frag?raw';
import { buildCustomShader } from '../../shaders/customShader';
import groundColorShader from '../../shaders/fractal_cube/ground/color.frag?raw';
import groundRoughnessShader from '../../shaders/fractal_cube/ground/roughness.frag?raw';
import { generateNormalMapFromTexture, loadTexture } from '../../textureLoading';
import { initBaseScene } from '../../util/util';
import { initWebSynth } from '../../webSynth';
import { buildCube } from './genCube';

const locations = {
  spawn: {
    pos: new THREE.Vector3(4.9, 24.35, 9.69),
    rot: new THREE.Vector3(-0.022, 1.488, 0),
  },
};

export const processLoadedScene = async (viz: Viz, loadedWorld: THREE.Group): Promise<SceneConfig> => {
  viz.camera.far = 10_000;
  viz.camera.updateMatrixWorld();

  const base = initBaseScene(viz);
  base.light.intensity = 1.5;
  base.ambientlight.intensity = 0.5;

  const loader = new THREE.ImageBitmapLoader();
  const groundTexture = await loadTexture(
    loader,
    // 'https://i.ameo.link/aau.jpg'
    // 'https://i.ameo.link/aap.jpg'
    // 'https://i.ameo.link/ab3.png'
    // 'https://i.ameo.link/ab4.png'
    // 'https://i.ameo.link/ab5.png'
    // 'https://i.ameo.link/ab6.png'
    // 'https://i.ameo.link/ab7.png'
    // 'https://i.ameo.link/ab8.png' // GOOD
    // 'https://i.ameo.link/ab9.png'
    'https://i.ameo.link/aba.png'
  );

  const srcData: ImageBitmap = groundTexture.source.data;
  const canvas = document.createElement('canvas');
  canvas.width = srcData.width;
  canvas.height = srcData.height;
  const ctx = canvas.getContext('2d')!;
  ctx.drawImage(srcData, 0, 0);
  document.body.appendChild(canvas);

  const groundNormalTexture = await generateNormalMapFromTexture(groundTexture);

  const uvTransform = new THREE.Matrix3().scale(64, 64);
  const groundMaterial = buildCustomShader(
    {
      map: groundTexture,
      normalMap: groundNormalTexture,
      uvTransform,
      normalScale: 1.5,
      metalness: 0.9,
      // emissiveIntensity: 0.2,
    },
    {
      colorShader: groundColorShader,
      roughnessShader: groundRoughnessShader,
      // emissiveShader: groundEmissiveShader,
    },
    { tileBreaking: { type: 'neyret', patchScale: 7 }, antialiasRoughnessShader: true }
  );

  viz.registerBeforeRenderCb(curTimeSeconds => groundMaterial.setCurTimeSeconds(curTimeSeconds));

  const ground = new THREE.Mesh(new THREE.BoxGeometry(200, 50, 200), groundMaterial);
  ground.position.set(0, -1, 0);
  loadedWorld.add(ground);

  const cubeMaterial = buildCustomShader(
    {
      metalness: 0.9,
      roughness: 0.9,
      color: new THREE.Color(0x0c0c0c),
    },
    {
      colorShader: bigCubeColorShader,
    },
    { antialiasRoughnessShader: true }
  );

  viz.registerBeforeRenderCb(curTimeSeconds => cubeMaterial.setCurTimeSeconds(curTimeSeconds));

  const cubes = buildCube(cubeMaterial);
  cubes.position.set(0, -22, 0);
  loadedWorld.add(cubes);

  initWebSynth({ compositionIDToLoad: 61 }).then(wsHandle => {
    wsHandle.startAll();
  });

  return { locations, spawnLocation: 'spawn', debugPos: true };
};
