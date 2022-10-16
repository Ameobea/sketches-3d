import * as THREE from 'three';

import { buildCustomShader } from '../../shaders/customShader';
import { generateNormalMapFromTexture, loadTexture } from '../../textureLoading';
import TowerEntryPlinthColorShader from './shaders/color.frag?raw';
import TowerEntryPlinthMetalnessShader from './shaders/metalness.frag?raw';
import TowerEntryPlinthRoughnessShader from './shaders/roughness.frag?raw';

export const buildMuddyGoldenLoopsMat = async (loader: THREE.ImageBitmapLoader) => {
  const texture = await loadTexture(
    loader,
    'https://ameo-imgen.ameo.workers.dev/img-samples/000008.1932710312.png'
  );
  const combinedDiffuseNormalTexture = await generateNormalMapFromTexture(texture, {}, true);

  return buildCustomShader(
    {
      color: new THREE.Color(0x989898),
      metalness: 0.8,
      roughness: 0.97,
      map: combinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(0.4, 0.4),
      mapDisableDistance: null,
      normalScale: 4,
    },
    {
      colorShader: TowerEntryPlinthColorShader,
      roughnessShader: TowerEntryPlinthRoughnessShader,
      metalnessShader: TowerEntryPlinthMetalnessShader,
    },
    {
      usePackedDiffuseNormalGBA: true,
      disabledDirectionalLightIndices: [0],
      useGeneratedUVs: true,
      tileBreaking: { type: 'neyret', patchScale: 2 },
    }
  );
};
