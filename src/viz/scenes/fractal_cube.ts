import * as THREE from 'three';

import type { SceneConfig } from '.';
import type { VizState } from '..';
import { buildCustomShader } from '../shaders/customShader';
import pillarColorShader from '../shaders/subdivided/pillar/color.frag?raw';
import { generateNormalMapFromTexture, loadTexture } from '../textureLoading';
import { initBaseScene } from '../util';

const locations = {
  spawn: {
    pos: new THREE.Vector3(48.17740050559579, 23.920086905508146, 8.603910511800485),
    rot: new THREE.Vector3(-0.022, 1.488, 0),
  },
};

export const processLoadedScene = async (viz: VizState, loadedWorld: THREE.Group): Promise<SceneConfig> => {
  const base = initBaseScene(viz);

  const enginePromise = import('../wasmComp/engine');

  const loader = new THREE.ImageBitmapLoader();
  const groundTexture = await loadTexture(loader, 'https://ameo.link/u/aau.jpg');

  const srcData: ImageBitmap = groundTexture.source.data;
  const canvas = document.createElement('canvas');
  canvas.width = srcData.width;
  canvas.height = srcData.height;
  const ctx = canvas.getContext('2d')!;
  ctx.drawImage(srcData, 0, 0);
  document.body.appendChild(canvas);

  const engine = await enginePromise;
  await engine.default();
  const groundNormalTexture = await generateNormalMapFromTexture(groundTexture);

  const uvTransform = new THREE.Matrix3().scale(3, 4);
  const groundMaterial = buildCustomShader(
    {
      map: groundTexture,
      normalMap: groundNormalTexture,
      uvTransform,
      normalScale: 1,
      roughness: 0.2,
      metalness: 0.6,
    },
    { colorShader: pillarColorShader },
    { useTileBreaking: true }
  );

  const ground = new THREE.Mesh(new THREE.BoxGeometry(200, 50, 200), groundMaterial);
  ground.position.set(0, -1, 0);
  loadedWorld.add(ground);

  return { locations, spawnLocation: 'spawn', debugPos: true };
};
