import * as THREE from 'three';

import type { VizState } from 'src/viz';
import { generateNormalMapFromTexture, loadNamedTextures, loadTexture } from 'src/viz/textureLoading';
import { buildCustomShader, type CustomShaderProps } from 'src/viz/shaders/customShader';
import BridgeMistColorShader from 'src/viz/shaders/bridge2/bridge_top_mist/color.frag?raw';

export const buildGreenMosaic2Material = async (
  loader: THREE.ImageBitmapLoader,
  shaderPropOverrides: Partial<CustomShaderProps> = {}
) => {
  const { greenMosaic2Albedo, greenMosaic2Normal, greenMosaic2Roughness } = await loadNamedTextures(loader, {
    greenMosaic2Albedo: 'https://i.ameo.link/ccn.avif',
    greenMosaic2Normal: 'https://i.ameo.link/bqi.jpg',
    greenMosaic2Roughness: 'https://i.ameo.link/bqj.jpg',
  });

  return buildCustomShader({
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
  });
};

export const buildGoldMaterial = async (
  loader: THREE.ImageBitmapLoader,
  shaderPropOverrides: Partial<CustomShaderProps> = {}
) => {
  const { goldTextureAlbedo, goldTextureNormal, goldTextureRoughness } = await loadNamedTextures(loader, {
    goldTextureAlbedo: 'https://i.ameo.link/be0.jpg',
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

export const buildMaterials = async (viz: VizState, loadedWorld: THREE.Group) => {
  const loader = new THREE.ImageBitmapLoader();
  const towerPlinthPedestalTextureP = loadTexture(
    loader,
    'https://pub-80300747d44d418ca912329092f69f65.r2.dev/img-samples/000005.1476533049.png'
  );
  const towerPlinthPedestalTextureCombinedDiffuseNormalTextureP = towerPlinthPedestalTextureP.then(
    towerPlinthPedestalTexture => generateNormalMapFromTexture(towerPlinthPedestalTexture, {}, true)
  );

  const bgTextureP = (async () => {
    const bgImage = await loader.loadAsync('https://i.ameo.link/bqn.jpg');
    const bgTexture = new THREE.Texture(bgImage);
    bgTexture.mapping = THREE.EquirectangularRefractionMapping;
    bgTexture.needsUpdate = true;
    return bgTexture;
  })();

  const [
    bgTexture,
    towerPlinthPedestalTextureCombinedDiffuseNormalTexture,
    { shinyPatchworkStoneAlbedo, shinyPatchworkStoneNormal, shinyPatchworkStoneRoughness },
    greenMosaic2Material,
    goldMaterial,
  ] = await Promise.all([
    bgTextureP,
    towerPlinthPedestalTextureCombinedDiffuseNormalTextureP,
    loadNamedTextures(loader, {
      shinyPatchworkStoneAlbedo: 'https://i.ameo.link/bqk.jpg',
      shinyPatchworkStoneNormal: 'https://i.ameo.link/bqm.jpg',
      shinyPatchworkStoneRoughness: 'https://i.ameo.link/bql.jpg',
    }),
    buildGreenMosaic2Material(loader),
    buildGoldMaterial(loader),
  ]);

  const pylonMaterial = buildCustomShader(
    {
      color: new THREE.Color(0x898989),
      metalness: 0.18,
      roughness: 0.82,
      map: towerPlinthPedestalTextureCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(0.8, 0.8),
      mapDisableDistance: null,
      normalScale: 5.2,
    },
    {},
    {
      usePackedDiffuseNormalGBA: true,
      useGeneratedUVs: true,
      randomizeUVOffset: true,
      tileBreaking: { type: 'neyret', patchScale: 0.9 },
    }
  );

  const checkpointMat = buildCustomShader(
    { metalness: 0, alphaTest: 0.05, transparent: true },
    { colorShader: BridgeMistColorShader },
    { disableToneMapping: true }
  );
  viz.registerBeforeRenderCb(curTimeSeconds => checkpointMat.setCurTimeSeconds(curTimeSeconds));

  const shinyPatchworkStoneMaterial = buildCustomShader(
    {
      color: new THREE.Color(0xffffff),
      metalness: 0.5,
      roughness: 0.5,
      map: shinyPatchworkStoneAlbedo,
      normalMap: shinyPatchworkStoneNormal,
      roughnessMap: shinyPatchworkStoneRoughness,
      uvTransform: new THREE.Matrix3().scale(0.8, 0.8),
      mapDisableDistance: null,
      normalScale: 1.2,
      ambientLightScale: 2,
    },
    {},
    {
      useGeneratedUVs: true,
      randomizeUVOffset: true,
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
    checkpointMat,
    bgTexture,
    shinyPatchworkStoneMaterial,
    greenMosaic2Material,
    goldMaterial,
  };
};
