import * as THREE from 'three';

import { AsyncOnce } from 'src/viz/util/AsyncOnce';
import { loadNamedTextures } from 'src/viz/textureLoading';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import colorShader from './shaders/color.frag?raw';

const Mat = new AsyncOnce(async (loader: THREE.ImageBitmapLoader) => {
  const { tilesDiffuse } = await loadNamedTextures(loader, { tilesDiffuse: 'https://i.ameo.link/cvl.avif' });
  return buildCustomShader(
    {
      color: 0xaaaaaa,
      map: tilesDiffuse,
      roughness: 0.9,
      metalness: 0.3,
      uvTransform: new THREE.Matrix3().scale(0.148, 0.148),
      mapDisableDistance: null,
    },
    { colorShader },
    {
      useGeneratedUVs: true,
      randomizeUVOffset: true,
      tileBreaking: { type: 'neyret', patchScale: 9.5 },
    }
  );
});

export const buildGraySToneBricksFloorMaterial = (loader: THREE.ImageBitmapLoader) => Mat.get(loader);
