import * as THREE from 'three';

import { buildCustomShader } from 'src/viz/shaders/customShader';
import { AsyncOnce } from 'src/viz/util/AsyncOnce';
import { loadNamedTextures } from 'src/viz/textureLoading';

export const GrayFossilRockTextures = new AsyncOnce((loader: THREE.ImageBitmapLoader) =>
  loadNamedTextures(loader, {
    platformDiffuse: 'https://i.ameo.link/cce.avif',
    platformNormal: 'https://i.ameo.link/ccf.avif',
  })
);

export const buildGrayFossilRockMaterial = async (loader: THREE.ImageBitmapLoader) => {
  const { platformDiffuse, platformNormal } = await GrayFossilRockTextures.get(loader);

  return buildCustomShader(
    {
      color: 0x474a4d,
      map: platformDiffuse,
      roughness: 0.9,
      metalness: 0.5,
      uvTransform: new THREE.Matrix3().scale(138.073, 138.073),
      normalMap: platformNormal,
      normalScale: 1,
      normalMapType: THREE.TangentSpaceNormalMap,
      mapDisableDistance: null,
      ambientLightScale: 1.8,
    },
    {},
    { useTriplanarMapping: false, tileBreaking: { type: 'neyret', patchScale: 2 } }
  );
};
