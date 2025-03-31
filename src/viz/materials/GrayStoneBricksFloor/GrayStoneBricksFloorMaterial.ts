import * as THREE from 'three';

import { AsyncOnce } from 'src/viz/util/AsyncOnce';
import { loadNamedTextures } from 'src/viz/textureLoading';
import {
  buildCustomShader,
  type CustomShaderOptions,
  type CustomShaderProps,
  type CustomShaderShaders,
} from 'src/viz/shaders/customShader';
import colorShader from './shaders/color.frag?raw';

const Texs = new AsyncOnce((loader: THREE.ImageBitmapLoader) =>
  loadNamedTextures(loader, { tilesDiffuse: 'https://i.ameo.link/cvl.avif' })
);

export const buildGrayStoneBricksFloorMaterial = async (
  loader: THREE.ImageBitmapLoader,
  propsOverrides?: CustomShaderProps,
  shadersOverrides?: CustomShaderShaders,
  optsOverrides?: CustomShaderOptions
) => {
  const { tilesDiffuse } = await Texs.get(loader);
  return buildCustomShader(
    {
      color: 0xaaaaaa,
      map: tilesDiffuse,
      roughness: 0.9,
      metalness: 0.3,
      uvTransform: new THREE.Matrix3().scale(0.148, 0.148),
      mapDisableDistance: null,
      ...(propsOverrides ?? {}),
    },
    { colorShader, ...(shadersOverrides ?? {}) },
    {
      useGeneratedUVs: true,
      randomizeUVOffset: true,
      tileBreaking: { type: 'neyret', patchScale: 9.5 },
      ...(optsOverrides ?? {}),
    }
  );
};
