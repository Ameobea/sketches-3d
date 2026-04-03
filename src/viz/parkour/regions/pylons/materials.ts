import * as THREE from 'three';

import type { Viz } from 'src/viz';
import { generateNormalMapFromTexture, loadNamedTextures, loadTexture } from 'src/viz/textureLoading';
import {
  buildCustomShader,
  type CustomShaderOptions,
  type CustomShaderProps,
  type CustomShaderShaders,
} from 'src/viz/shaders/customShader';
import { buildCheckpointMaterial } from 'src/viz/materials/Checkpoint/CheckpointMaterial';
import { AsyncOnce } from 'src/viz/util/AsyncOnce';
import { getNormalGenWorkers } from 'src/viz/workerPool';

const GreenMosaic2Textures = new AsyncOnce((loader: THREE.ImageBitmapLoader) =>
  loadNamedTextures(loader, {
    greenMosaic2Albedo: ['https://i.ameo.link/ccn.avif', { colorSpace: THREE.SRGBColorSpace }],
    greenMosaic2Normal: 'https://i.ameo.link/cwb.avif',
    greenMosaic2Roughness: 'https://i.ameo.link/cwc.avif',
  })
);

export const buildGreenMosaic2Material = async (
  loader: THREE.ImageBitmapLoader,
  shaderPropOverrides: Partial<CustomShaderProps> = {},
  shaderShaderOverrides: Partial<CustomShaderShaders> = {},
  shaderOptOverrides: Partial<CustomShaderOptions> = {}
) => {
  const { greenMosaic2Albedo, greenMosaic2Normal, greenMosaic2Roughness } =
    await GreenMosaic2Textures.get(loader);

  return buildCustomShader(
    {
      color: new THREE.Color(0xffffff),
      metalness: 0.5,
      roughness: 0.5,
      map: greenMosaic2Albedo,
      normalMap: greenMosaic2Normal,
      roughnessMap: greenMosaic2Roughness,
      uvTransform: new THREE.Matrix3().scale(7.8, 7.8),
      mapDisableDistance: null,
      normalScale: 2.2,
      ambientLightScale: 2,
      ...shaderPropOverrides,
    },
    shaderShaderOverrides,
    shaderOptOverrides
  );
};

export const buildGoldMaterial = async (
  loader: THREE.ImageBitmapLoader,
  shaderPropOverrides: Partial<CustomShaderProps> = {}
) => {
  const { goldTextureAlbedo, goldTextureNormal, goldTextureRoughness } = await loadNamedTextures(loader, {
    goldTextureAlbedo: ['https://i.ameo.link/be0.jpg', { colorSpace: THREE.SRGBColorSpace }],
    goldTextureNormal: 'https://i.ameo.link/be2.jpg',
    goldTextureRoughness: 'https://i.ameo.link/bdz.jpg',
  });

  return buildCustomShader(
    {
      map: goldTextureAlbedo,
      roughnessMap: goldTextureRoughness,
      normalMap: goldTextureNormal,
      color: new THREE.Color(0xaaaaaa),
      uvTransform: new THREE.Matrix3().scale(0.6, 0.6),
      normalScale: 4,
      roughness: 0.2,
      ...shaderPropOverrides,
    },
    {},
    { useTriplanarMapping: true }
  );
};

const buildTowerPlinthPedestalTextureCombinedDiffuseNormalTextureP = (
  providedLoader?: THREE.ImageBitmapLoader
): Promise<THREE.Texture> => {
  const loader = providedLoader ?? new THREE.ImageBitmapLoader();
  const towerPlinthPedestalTextureP = loadTexture(loader, 'https://i.ameo.link/cul.png', {
    colorSpace: THREE.SRGBColorSpace,
  });
  return towerPlinthPedestalTextureP.then(towerPlinthPedestalTexture =>
    generateNormalMapFromTexture(towerPlinthPedestalTexture, {}, true)
  );
};

export const buildPylonMaterial = async (loader?: THREE.ImageBitmapLoader) =>
  buildCustomShader(
    {
      color: new THREE.Color(0x7a7a7a),
      metalness: 0.18,
      roughness: 0.82,
      map: await buildTowerPlinthPedestalTextureCombinedDiffuseNormalTextureP(loader),
      uvTransform: new THREE.Matrix3().scale(0.8, 0.8),
      mapDisableDistance: null,
      normalScale: 5.2,
    },
    {},
    {
      usePackedDiffuseNormalGBA: true,
      useGeneratedUVs: true,
      randomizeUVOffset: false,
      tileBreaking: { type: 'neyret', patchScale: 0.9 },
    }
  );

export const ShinyPatchworkStoneTextures = new AsyncOnce((loader: THREE.ImageBitmapLoader) =>
  loadNamedTextures(loader, {
    shinyPatchworkStoneAlbedo: ['https://i.ameo.link/bqk.jpg', { colorSpace: THREE.SRGBColorSpace }],
    shinyPatchworkStoneNormal: 'https://i.ameo.link/bqm.jpg',
    shinyPatchworkStoneRoughness: 'https://i.ameo.link/bql.jpg',
  })
);

export const buildPylonsCheckpointMaterial = (viz: Viz) =>
  buildCheckpointMaterial(
    viz,
    [0.13, 0.015, 0.645],
    {},
    {
      noiseQuantize: 0.01,
      noisePosQuantize: 0.01,
      noiseBias: 0.12,
      noisePow: 1.2,
      noiseMultiplier: 3,
      noiseDir: [0, 1, -2],
      noiseRotation: [0, 0.28, 0],
      fadeTopDist: 0.1,
      fadeTopSteepness: 1,
      fadeBottomDist: 0,
      fadeBottomSteepness: 1,
      fadeEdgeFreq: 1.2,
      fadeEdgeSpeed: [3.1, 3.1],
      fadeEdgeAmp: 0.1,
      noiseVertBiasLo: 0.4,
      noiseVertBiasHi: 1,
      noiseVertBiasAmtLo: 0,
      noiseVertBiasAmtHi: -0.5,
      // Breeze
      breezeTimeFreq: 0.85,
      breezeThreshold: 0.3,
      breezeThresholdHi: 0.8,
      breezeModScale: 0.4,
      breezePmDepth: 1,
      breezeAmpMult: 2,
      breezeHotColor: [0.08, 0.018, 1.545],
      breezeColorMix: 1,
      // breezeBiasDelta: -0.0854,
      breezeNoiseAmpMult: 0,
    }
  );

export const buildPylonsMaterials = async (
  viz: Viz,
  loadedWorld: THREE.Group,
  loader = new THREE.ImageBitmapLoader()
) => {
  // kick off loading of the normal gen worker pool so that can be ready as soon as possible
  getNormalGenWorkers();

  const bgTextureP = (async () => {
    const bgImage = await loader.loadAsync('https://i.ameo.link/bqn.jpg');
    const bgTexture = new THREE.Texture(
      bgImage,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      THREE.SRGBColorSpace
    );
    bgTexture.mapping = THREE.EquirectangularRefractionMapping;
    bgTexture.needsUpdate = true;
    return bgTexture;
  })();

  const [
    pylonMaterial,
    bgTexture,
    { shinyPatchworkStoneAlbedo, shinyPatchworkStoneNormal, shinyPatchworkStoneRoughness },
    greenMosaic2Material,
    goldMaterial,
  ] = await Promise.all([
    buildPylonMaterial(loader),
    bgTextureP,
    ShinyPatchworkStoneTextures.get(loader),
    buildGreenMosaic2Material(loader),
    buildGoldMaterial(loader),
  ]);

  const shinyPatchworkStoneMaterial = buildCustomShader(
    {
      color: new THREE.Color(0xffffff),
      metalness: 0.5,
      roughness: 0.5,
      map: shinyPatchworkStoneAlbedo,
      normalMap: shinyPatchworkStoneNormal,
      roughnessMap: shinyPatchworkStoneRoughness,
      uvTransform: new THREE.Matrix3().scale(0.6, 0.6),
      mapDisableDistance: null,
      normalScale: 1.2,
      ambientLightScale: 2,
    },
    {},
    {
      useGeneratedUVs: true,
      randomizeUVOffset: false,
      tileBreaking: { type: 'neyret', patchScale: 0.9 },
    }
  );

  viz.scene.background = bgTexture;

  loadedWorld.traverse(obj => {
    if (!(obj instanceof THREE.Mesh)) {
      return;
    }

    if (obj.name.includes('sparkle')) {
      obj.material = shinyPatchworkStoneMaterial;
    } else {
      obj.material = pylonMaterial;
    }
  });

  return {
    pylonMaterial,
    checkpointMat: () => buildPylonsCheckpointMaterial(viz),
    bgTexture,
    shinyPatchworkStoneMaterial,
    greenMosaic2Material,
    goldMaterial,
    loader,
  };
};
