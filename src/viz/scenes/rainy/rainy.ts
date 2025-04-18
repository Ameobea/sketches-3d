import { N8AOPostPass } from 'n8ao';
import {
  CopyPass,
  DepthOfFieldEffect,
  EffectComposer,
  EffectPass,
  KernelSize,
  RenderPass,
  SMAAEffect,
  SMAAPreset,
  ToneMappingEffect,
  ToneMappingMode,
} from 'postprocessing';
import * as THREE from 'three';

import type { Viz } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import { DepthPass, MainRenderPass } from 'src/viz/passes/depthPrepass';
import { buildCustomShader, type MaterialClass } from 'src/viz/shaders/customShader';
import {
  genCrossfadedTexture,
  generateNormalMapFromTexture,
  loadNamedTextures,
  loadRawTexture,
  loadTexture,
} from 'src/viz/textureLoading';
import { delay, DEVICE_PIXEL_RATIO, smoothstep } from 'src/viz/util/util';
import { initWebSynth } from 'src/viz/webSynth';
import type { SceneConfig } from '..';
import { FogPass } from './fogShader';

const locations = {
  spawn: {
    pos: new THREE.Vector3(-98, 2, 0),
    rot: new THREE.Vector3(-0.06, -1.514, 0),
  },
  corner: {
    pos: new THREE.Vector3(-2.173, 1.435, 1.95),
    rot: new THREE.Vector3(-0.686, 4.486, 0),
  },
  stairs: {
    pos: new THREE.Vector3(-1.0414, 1.435, -100),
    rot: new THREE.Vector3(-0.28, 7.764, 0),
  },
  greenhouse: {
    pos: new THREE.Vector3(-7.394, 12.457, -78.0357),
    rot: new THREE.Vector3(-0.028, 8.546, 0),
  },
  tree: {
    pos: new THREE.Vector3(-32.2194, 12.457, -53.979),
    rot: new THREE.Vector3(-0.134, -10.988, 0),
  },
};

const loadTextures = async () => {
  const loader = new THREE.ImageBitmapLoader();

  const cementTextureP = loadTexture(loader, 'https://i.ameo.link/amf.png');
  const cementTextureCombinedDiffuseNormalP = cementTextureP.then(cementTexture =>
    generateNormalMapFromTexture(cementTexture, {}, true)
  );

  const cloudsBgTextureP = loadTexture(loader, 'https://i.ameo.link/ame.jpg', {
    mapping: THREE.EquirectangularReflectionMapping,
    minFilter: THREE.NearestFilter,
    magFilter: THREE.NearestFilter,
  });

  const crossfadedCementTextureP = Promise.all([
    loadRawTexture('https://pub-80300747d44d418ca912329092f69f65.r2.dev/img-samples/000364.1012055443.png'),
    loadRawTexture('https://pub-80300747d44d418ca912329092f69f65.r2.dev/img-samples/000370.314757479.png'),
    loadRawTexture('https://pub-80300747d44d418ca912329092f69f65.r2.dev/img-samples/000365.1330968334.png'),
    loadRawTexture('https://pub-80300747d44d418ca912329092f69f65.r2.dev/img-samples/000364.1012055443.png'),
  ]).then(async textures => genCrossfadedTexture(textures, 0.2, { anisotropy: 4 }));

  const crossfadedCementTextureNormalP = crossfadedCementTextureP.then(cementTexture =>
    generateNormalMapFromTexture(cementTexture, {}, false)
  );

  const [
    cementTextureCombinedDiffuseNormal,
    cloudsBgTexture,
    crossfadedCementTexture,
    crossfadedCementTextureNormal,
    rest,
  ] = await Promise.all([
    cementTextureCombinedDiffuseNormalP,
    cloudsBgTextureP,
    crossfadedCementTextureP,
    crossfadedCementTextureNormalP,
    loadNamedTextures(loader, {
      goldTextureAlbedo: 'https://i.ameo.link/be0.jpg',
      goldTextureNormal: 'https://i.ameo.link/be2.jpg',
      windowSeamless: 'https://i.ameo.link/bn8.jpg',
      planterSoil1Albedo: 'https://i.ameo.link/bmz.jpg',
      planterSoil1Normal: 'https://i.ameo.link/bn0.jpg',
      planterSoil1Roughness: 'https://i.ameo.link/bn1.jpg',
      planterSoil2Albedo: 'https://i.ameo.link/bny.jpg',
      planterSoil2Normal: 'https://i.ameo.link/bnz.jpg',
      planterSoil2Roughness: 'https://i.ameo.link/bo0.jpg',
      particleBoardAlbedo: 'https://i.ameo.link/bnp.jpg',
      particleBoardNormal: 'https://i.ameo.link/bnq.jpg',
      particleBoardRoughness: 'https://i.ameo.link/bnr.jpg',
      trunkAlbedo: 'https://i.ameo.link/bo1.jpg',
      trunkNormal: 'https://i.ameo.link/bo2.jpg',
      trunkRoughness: 'https://i.ameo.link/bo3.jpg',
      leaves2: 'https://i.ameo.link/bnx.jpg',
    }),
  ]);

  return {
    cementTextureCombinedDiffuseNormal,
    cloudsBgTexture,
    crossfadedCementTexture,
    crossfadedCementTextureNormal,
    ...rest,
  };
};

const initScene = async (viz: Viz, loadedWorld: THREE.Group, _vizConfig: VizConfig) => {
  const {
    cementTextureCombinedDiffuseNormal,
    cloudsBgTexture,
    crossfadedCementTexture,
    crossfadedCementTextureNormal,
    goldTextureAlbedo,
    goldTextureNormal,
    windowSeamless,
    planterSoil1Albedo,
    planterSoil1Normal,
    planterSoil1Roughness,
    planterSoil2Albedo,
    planterSoil2Normal,
    planterSoil2Roughness,
    particleBoardAlbedo,
    particleBoardNormal,
    particleBoardRoughness,
    trunkAlbedo,
    trunkNormal,
    trunkRoughness,
    leaves2,
  } = await loadTextures();

  const backgroundScene = new THREE.Scene();
  backgroundScene.background = cloudsBgTexture;

  const bgAmbientLight = new THREE.AmbientLight(0xffffff, 0.35);
  const fgAmbientLight = new THREE.AmbientLight(0xffffff, 0.25);
  viz.scene.add(fgAmbientLight);
  backgroundScene.add(bgAmbientLight);

  // prettier-ignore
  const cementLUT = new Uint8Array([6,5,12,255,21,15,13,255,20,18,26,255,26,21,18,255,30,22,21,255,30,25,22,255,33,27,23,255,34,26,31,255,34,29,27,255,38,30,28,255,37,31,27,255,39,33,31,255,42,35,31,255,41,36,33,255,42,36,31,255,43,36,36,255,45,38,35,255,46,40,35,255,46,41,37,255,48,41,39,255,38,44,51,255,50,42,38,255,49,44,37,255,51,44,41,255,50,44,43,255,52,45,41,255,54,47,41,255,55,47,43,255,55,47,46,255,55,48,45,255,57,50,45,255,58,50,47,255,58,51,50,255,61,52,50,255,59,53,48,255,61,53,49,255,60,54,50,255,62,54,52,255,62,55,48,255,55,55,76,255,64,55,52,255,63,56,51,255,63,58,48,255,64,57,54,255,65,58,53,255,65,57,57,255,66,57,53,255,64,59,55,255,67,59,56,255,66,60,53,255,67,60,56,255,57,62,68,255,72,60,51,255,67,61,57,255,69,61,56,255,70,61,58,255,71,62,60,255,70,63,61,255,70,63,60,255,70,64,57,255,72,63,58,255,71,64,59,255,74,65,61,255,73,65,62,255,73,66,62,255,74,66,61,255,75,66,65,255,77,67,64,255,75,68,64,255,76,68,66,255,77,68,64,255,77,70,66,255,79,70,69,255,78,71,64,255,79,70,68,255,80,71,66,255,79,72,68,255,81,73,71,255,81,73,68,255,81,73,70,255,81,74,71,255,82,74,70,255,84,75,70,255,84,75,72,255,82,76,71,255,85,76,75,255,84,76,74,255,85,78,72,255,86,78,74,255,87,78,73,255,87,78,75,255,87,79,76,255,81,80,91,255,90,79,76,255,87,81,74,255,88,80,79,255,89,80,76,255,87,81,77,255,89,81,77,255,90,81,79,255,89,82,79,255,91,82,82,255,90,83,78,255,92,83,80,255,93,83,80,255,93,84,78,255,92,84,80,255,90,85,81,255,92,84,82,255,91,85,78,255,92,85,79,255,94,85,83,255,93,87,83,255,96,86,83,255,95,87,82,255,94,87,84,255,94,87,82,255,96,88,82,255,96,88,86,255,98,87,86,255,97,88,85,255,97,89,85,255,97,89,84,255,97,90,86,255,99,90,84,255,99,90,88,255,100,90,87,255,98,91,86,255,98,91,88,255,100,91,86,255,100,92,88,255,99,93,89,255,97,93,98,255,102,92,90,255,102,93,92,255,87,95,109,255,99,94,91,255,102,93,88,255,102,94,91,255,102,94,90,255,104,94,90,255,102,95,93,255,102,95,89,255,88,96,120,255,104,95,92,255,103,96,91,255,104,95,90,255,103,97,94,255,104,97,94,255,106,97,93,255,106,97,96,255,106,97,95,255,104,98,94,255,105,98,93,255,106,98,92,255,106,98,95,255,108,98,95,255,106,99,97,255,106,100,96,255,109,99,95,255,100,100,114,255,108,100,97,255,109,100,99,255,107,101,96,255,108,101,97,255,108,101,96,255,111,101,99,255,111,102,97,255,111,102,96,255,109,102,100,255,109,103,95,255,110,103,99,255,112,103,100,255,111,104,99,255,111,105,101,255,113,104,102,255,112,105,102,255,113,104,104,255,111,106,103,255,114,105,102,255,114,105,100,255,114,106,102,255,113,107,101,255,110,107,115,255,114,107,105,255,116,107,105,255,116,107,104,255,114,109,104,255,117,108,103,255,116,108,109,255,105,110,127,255,116,109,105,255,116,109,106,255,119,109,107,255,119,110,106,255,117,111,105,255,119,111,110,255,119,111,106,255,119,111,108,255,118,112,108,255,120,112,110,255,122,112,108,255,120,113,109,255,121,113,107,255,121,114,111,255,121,114,109,255,122,114,109,255,123,114,111,255,122,116,112,255,123,115,114,255,123,116,111,255,123,117,114,255,126,116,113,255,125,117,112,255,125,117,113,255,126,117,115,255,124,118,112,255,126,118,115,255,125,119,114,255,128,119,115,255,128,119,118,255,127,120,116,255,127,120,118,255,128,121,114,255,129,121,116,255,128,121,117,255,131,121,118,255,129,122,120,255,130,122,118,255,128,123,117,255,130,122,122,255,131,123,117,255,130,124,120,255,132,124,119,255,131,124,121,255,131,125,120,255,134,125,123,255,132,126,119,255,133,126,122,255,134,127,121,255,134,127,125,255,136,128,123,255,134,128,124,255,136,128,125,255,135,129,126,255,136,130,125,255,137,130,126,255,139,131,129,255,138,132,128,255,140,133,128,255,142,134,131,255,141,135,131,255,142,136,131,255,144,137,134,255,146,140,136,255,151,145,140,255]);
  const cementMat = buildCustomShader(
    {
      name: 'cement_custom',
      map: cementTextureCombinedDiffuseNormal,
      uvTransform: new THREE.Matrix3().scale(0.05, 0.05),
    },
    {},
    {
      usePackedDiffuseNormalGBA: { lut: cementLUT },
      useGeneratedUVs: true,
    }
  );
  const walkwayMat = buildCustomShader(
    {
      name: 'walkway_custom',
      map: crossfadedCementTexture,
      normalMap: crossfadedCementTextureNormal,
      normalScale: 3,
      uvTransform: new THREE.Matrix3().scale(0.03, 0.03),
    },
    {},
    {
      useGeneratedUVs: true,
      randomizeUVOffset: false,
    }
  );

  goldTextureNormal.repeat.set(34, 34);
  goldTextureAlbedo.repeat.set(34, 34);
  windowSeamless.repeat.set(40, 40);

  const greenhouseWindowsMaterial = new THREE.MeshPhysicalMaterial({
    map: windowSeamless,
    transmission: 1,
    roughness: 0.64,
    roughnessMap: goldTextureAlbedo,
    normalMap: goldTextureNormal,
    ior: 1.6,
    thickness: 0.8,
    thicknessMap: goldTextureAlbedo,
    color: new THREE.Color(0xd5cfd3),
  });

  const greenhouseWindowsMetalMaterial = buildCustomShader({
    color: new THREE.Color(0x181412),
    metalness: 1,
    roughness: 0.8,
  });

  const soil1Material = buildCustomShader(
    {
      map: planterSoil1Albedo,
      metalness: 0.1,
      roughness: 1,
      roughnessMap: planterSoil1Roughness,
      normalMap: planterSoil1Normal,
      normalScale: 3.5,
      uvTransform: new THREE.Matrix3().scale(2, 2),
    },
    {},
    { useGeneratedUVs: true, randomizeUVOffset: true, tileBreaking: { type: 'neyret', patchScale: 1.5 } }
  );

  const soil2Material = buildCustomShader(
    {
      map: planterSoil2Albedo,
      metalness: 0.1,
      roughness: 1,
      roughnessMap: planterSoil2Roughness,
      normalMap: planterSoil2Normal,
      normalScale: 3.5,
      uvTransform: new THREE.Matrix3().scale(2, 2),
    },
    {},
    { useGeneratedUVs: true, randomizeUVOffset: true, tileBreaking: { type: 'neyret', patchScale: 3.5 } }
  );

  const planterMaterial = buildCustomShader(
    {
      name: 'planter',
      map: crossfadedCementTexture,
      normalMap: crossfadedCementTextureNormal,
      normalScale: 3,
      uvTransform: new THREE.Matrix3().scale(0.18, 0.18),
    },
    {},
    {
      useGeneratedUVs: true,
      randomizeUVOffset: true,
    }
  );

  const plantPotMaterial = buildCustomShader(
    {
      name: 'planter',
      map: crossfadedCementTexture,
      normalMap: crossfadedCementTextureNormal,
      normalScale: 3,
      uvTransform: new THREE.Matrix3().scale(0.24, 0.24),
      ambientLightScale: 0.5,
    },
    {},
    {
      useGeneratedUVs: true,
      randomizeUVOffset: true,
    }
  );

  const particleBoardMaterial = buildCustomShader(
    {
      map: particleBoardAlbedo,
      metalness: 0.1,
      roughness: 1,
      roughnessMap: particleBoardRoughness,
      normalMap: particleBoardNormal,
      normalScale: 3.5,
      uvTransform: new THREE.Matrix3().scale(0.5, 0.5),
      ambientLightScale: 0.6,
      color: new THREE.Color(0xbfc3cf),
    },
    {},
    {
      useGeneratedUVs: true,
      randomizeUVOffset: true,
      tileBreaking: { type: 'neyret', patchScale: 1.5 },
    }
  );

  const bonsaiLeavesMat = buildCustomShader(
    {
      map: leaves2,
      metalness: 0.4,
      roughness: 1,
      // normalMap: leavesNormal,
      normalScale: 3.5,
      uvTransform: new THREE.Matrix3().scale(32, 32),
      ambientLightScale: 0.6,
    },
    {},
    { tileBreaking: { type: 'neyret', patchScale: 6 } }
  );

  const bonsaiTrunkMat = buildCustomShader(
    {
      map: trunkAlbedo,
      metalness: 0.4,
      roughness: 1,
      roughnessMap: trunkRoughness,
      normalMap: trunkNormal,
      normalScale: 3.5,
      uvTransform: new THREE.Matrix3().scale(5, 5),
    },
    {},
    { useTriplanarMapping: false }
  );

  const greenhouseShelves: THREE.Mesh[] = [];
  loadedWorld.traverse(obj => {
    const lowerName = obj.name.toLowerCase();

    if (!(obj instanceof THREE.Mesh)) {
      return;
    }

    if (Array.isArray(obj.material) || !(obj.material instanceof THREE.MeshStandardMaterial)) {
      return;
    }

    if (
      lowerName.startsWith('building') ||
      obj.parent?.name.startsWith('building') ||
      obj.parent?.parent?.name.startsWith('building')
    ) {
      obj.userData.nocollide = true;
    }

    if (obj.material.name === 'cement') {
      obj.material = cementMat;
    }

    if (
      lowerName.startsWith('walkway') ||
      lowerName.startsWith('railing_barrier') ||
      lowerName.startsWith('staircase')
    ) {
      obj.material = walkwayMat;
    }

    if (
      (lowerName.startsWith('railing') && !lowerName.includes('corner')) ||
      lowerName.startsWith('staircase_stairs')
    ) {
      obj.userData.convexhull = true;
    }

    if (lowerName === 'greenhouse_windows') {
      obj.material = greenhouseWindowsMaterial;
      viz.collisionWorldLoadedCbs.push(fpCtx => fpCtx.addTriMesh(obj));
    }

    if (lowerName === 'greenhouse_window_metal') {
      obj.material = greenhouseWindowsMetalMaterial;
    }

    if (lowerName.startsWith('planters') || lowerName === 'greenhouse_table') {
      obj.material = planterMaterial;
    }

    if (lowerName === 'soil_1') {
      obj.material = soil1Material;
    } else if (lowerName.startsWith('soil_2')) {
      obj.material = soil2Material;
    }

    if (lowerName.startsWith('greenhouse_platform')) {
      obj.material = planterMaterial;
    }

    if (lowerName.startsWith('greenhouse_shelves')) {
      obj.material = particleBoardMaterial;
      greenhouseShelves.push(obj);
    }

    if (lowerName.startsWith('greenhouse_shelf_poles')) {
      greenhouseShelves.push(obj);
    }

    if (lowerName === 'greenhouse_exterior_cement') {
      obj.material = planterMaterial;
    }

    if (lowerName.startsWith('greenhouse_plant_pot')) {
      obj.material = plantPotMaterial;
      greenhouseShelves.push(obj);
    }

    if (lowerName === 'greenhouse_plant_table') {
      obj.material = particleBoardMaterial;
      greenhouseShelves.push(obj);
    }

    if (lowerName.startsWith('greenhouse_plant_table_leg')) {
      greenhouseShelves.push(obj);
    }

    if (lowerName === 'bonsai_leaves') {
      obj.material = bonsaiLeavesMat;
      greenhouseShelves.push(obj);
    }

    if (lowerName === 'bonsai_trunk') {
      obj.material = bonsaiTrunkMat;
      greenhouseShelves.push(obj);
    }
  });

  const buildings = loadedWorld.children.filter(
    obj =>
      obj.name.startsWith('building') ||
      obj.name.startsWith('ground') ||
      obj.name === 'greenhouse_windows' ||
      obj.name === 'sines'
  );

  buildings.forEach(obj => {
    obj.removeFromParent();
    backgroundScene.add(obj);
  });

  return { backgroundScene, greenhouseShelves };
};

export const processLoadedScene = async (
  viz: Viz,
  loadedWorld: THREE.Group,
  vizConfig: VizConfig
): Promise<SceneConfig> => {
  viz.camera.far = 500;
  viz.camera.updateProjectionMatrix();

  const { backgroundScene, greenhouseShelves } = await initScene(viz, loadedWorld, vizConfig);

  const effectComposer = new EffectComposer(viz.renderer, { frameBufferType: THREE.HalfFloatType });
  effectComposer.autoRenderToScreen = false;

  const depthPassMaterial = new THREE.MeshDistanceMaterial({
    referencePosition: viz.camera.position,
    nearDistance: viz.camera.near,
    farDistance: viz.camera.far,
  });
  // hack to work around Three.JS bug.  Should probably be fixed in v159
  (depthPassMaterial as any).isMeshDistanceMaterial = false;
  const backgroundDepthPass = new DepthPass(backgroundScene, viz.camera, depthPassMaterial, true);
  backgroundDepthPass.clearPass.enabled = true;
  const getDistanceBuffer = () => backgroundDepthPass.renderTarget!;
  effectComposer.addPass(backgroundDepthPass);

  const backgroundRenderPass = new MainRenderPass(backgroundScene, viz.camera);
  effectComposer.addPass(backgroundRenderPass);

  class MyCopyPass extends CopyPass {
    constructor() {
      super();
      this.needsSwap = true;
    }

    override render(
      renderer: THREE.WebGLRenderer,
      inputBuffer: THREE.WebGLRenderTarget,
      outputBuffer: THREE.WebGLRenderTarget | null,
      _deltaTime?: number | undefined,
      _stencilTest?: boolean | undefined
    ) {
      (this.fullscreenMaterial as any).inputBuffer = inputBuffer.texture;
      renderer.setRenderTarget(this.renderToScreen ? null : outputBuffer);
      renderer.render(this.scene, this.camera);
    }
  }

  // fog pass reads from input buffer and writes to output buffer, then swaps buffers
  const fogPass = new FogPass(getDistanceBuffer, viz.camera);
  effectComposer.addPass(fogPass);

  // fog pass swaps buffers and DoF needs depth buffer on the input side, so we copy it back
  const copyPass = new MyCopyPass();
  effectComposer.addPass(copyPass);

  const depthOfFieldEffect = new DepthOfFieldEffect(viz.camera, {
    worldFocusDistance: 10,
    worldFocusRange: 50,
    bokehScale: 8,
  });
  depthOfFieldEffect.blurPass.kernelSize = KernelSize.VERY_SMALL;
  const bgEffectPass = new EffectPass(viz.camera, depthOfFieldEffect);
  effectComposer.addPass(bgEffectPass);

  // DoF also swaps buffers and render pass needs depth buffer on the side that it reads/writes to
  // (input) so we need to copy it back again
  const copyPass2 = new MyCopyPass();
  effectComposer.addPass(copyPass2);

  const foregroundRenderPass = new RenderPass(viz.scene, viz.camera);
  foregroundRenderPass.clear = false;
  foregroundRenderPass.clearPass.enabled = false;
  effectComposer.addPass(foregroundRenderPass);

  if (vizConfig.graphics.quality > GraphicsQuality.Low) {
    const n8aoPass = new N8AOPostPass(
      viz.scene,
      viz.camera,
      viz.renderer.domElement.width,
      viz.renderer.domElement.height
    );
    effectComposer.addPass(n8aoPass);
    n8aoPass.gammaCorrection = false;
    n8aoPass.configuration.intensity = 2;
    n8aoPass.configuration.aoRadius = 5;
    // \/ this breaks rendering and makes the background black if enabled
    // n8aoPass.configuration.halfRes = vizConfig.graphics.quality <= GraphicsQuality.Medium;
    n8aoPass.configuration.accumulate = true;
    n8aoPass.setQualityMode(
      {
        [GraphicsQuality.Low]: 'Performance',
        [GraphicsQuality.Medium]: 'Low',
        [GraphicsQuality.High]: 'High',
      }[vizConfig.graphics.quality]
    );
  }

  const glassScene = new THREE.Scene();

  class GlassRenderPass extends RenderPass {
    constructor(scene: THREE.Scene, camera: THREE.Camera) {
      super(scene, camera);
      this.needsDepthTexture = true;
      this.needsSwap = true;
    }

    override render(
      renderer: THREE.WebGLRenderer,
      inputBuffer: THREE.WebGLRenderTarget,
      outputBuffer: THREE.WebGLRenderTarget | null,
      _deltaTime?: number | undefined,
      _stencilTest?: boolean | undefined
    ) {
      // amazing hack to facilitate transmission for the glass.  It allows the output of all previous passes to
      // be used for transmission.  Then, we render to the output buffer.
      glassScene.background = inputBuffer.texture;
      const oldminFilter = inputBuffer.texture.minFilter;
      inputBuffer.texture.minFilter = THREE.LinearMipMapLinearFilter;
      inputBuffer.texture.needsUpdate = true;

      const scene = this.scene;
      const camera = this.camera;
      const shadowMapAutoUpdate = renderer.shadowMap.autoUpdate;
      const renderTarget = this.renderToScreen ? null : outputBuffer;
      if (this.skipShadowMapUpdate) {
        renderer.shadowMap.autoUpdate = false;
      }
      if (this.ignoreBackground || this.clearPass.overrideClearColor !== null) {
        scene.background = null;
      }
      renderer.setRenderTarget(renderTarget);
      renderer.render(scene, camera);
      renderer.shadowMap.autoUpdate = shadowMapAutoUpdate;

      inputBuffer.texture.minFilter = oldminFilter;
    }
  }

  const glassAmbientLight = new THREE.AmbientLight(0xffffff, 0.5);
  glassScene.add(glassAmbientLight);
  const glass = backgroundScene.getObjectByName('greenhouse_windows') as THREE.Mesh;
  const glassMetal = loadedWorld.getObjectByName('greenhouse_window_metal') as THREE.Mesh;
  glassScene.add(glass);
  backgroundScene.remove(glass);

  // n8ao swaps buffers, so the depth buffer is on the output side now.  Luckily, the glass pass actually
  // renders to the output buffer, so we don't need a copy here.
  const glassPass = new GlassRenderPass(glassScene, viz.camera);
  glassPass.clear = false;
  glassPass.clearPass.enabled = false;
  effectComposer.addPass(glassPass);

  // Glass pass swaps buffers, and the depth buffer is on the input side now, and normal Render pass
  // renders to the input buffer, so we're all good again.
  const greenhouseFgScene = new THREE.Scene();
  const greenhouseFgAmbientLight = new THREE.AmbientLight(0xffffff, 0.3);
  greenhouseFgScene.add(greenhouseFgAmbientLight);
  greenhouseFgScene.add(glassMetal);
  backgroundScene.remove(glassMetal);

  viz.collisionWorldLoadedCbs.push(fpCtx =>
    greenhouseShelves.forEach(mesh => {
      mesh.removeFromParent();
      greenhouseFgScene.add(mesh);
      fpCtx.addTriMesh(mesh);
    })
  );

  const glassMetalPass = new RenderPass(greenhouseFgScene, viz.camera);
  glassMetalPass.clear = false;
  glassMetalPass.clearPass.enabled = false;
  effectComposer.addPass(glassMetalPass);

  const smaaEffect2 = new SMAAEffect({
    preset: {
      [GraphicsQuality.Low]: SMAAPreset.LOW,
      [GraphicsQuality.Medium]: SMAAPreset.MEDIUM,
      [GraphicsQuality.High]: SMAAPreset.HIGH,
    }[vizConfig.graphics.quality],
  });
  const toneMappingEffect = new ToneMappingEffect({
    whitePoint: 1.1,
    middleGrey: 0.82,
    mode: ToneMappingMode.UNCHARTED2,
  });
  toneMappingEffect.blendMode.opacity.value = 0.5;
  viz.renderer.toneMappingExposure = 1.4;

  const smaaPass2 = new EffectPass(viz.camera, toneMappingEffect, smaaEffect2);
  smaaPass2.renderToScreen = true;
  effectComposer.addPass(smaaPass2);

  viz.renderer.autoClear = false;
  viz.renderer.autoClearColor = false;

  viz.registerResizeCb(() => {
    effectComposer.setSize(
      viz.renderer.domElement.width / DEVICE_PIXEL_RATIO,
      viz.renderer.domElement.height / DEVICE_PIXEL_RATIO
    );
  });

  viz.setRenderOverride((timeDiffSeconds: number) => {
    // depthPassMaterial.referencePosition?.copy(viz.camera.position);
    effectComposer.render(timeDiffSeconds);
  });

  const customDepthMaterial = new THREE.MeshDepthMaterial({
    depthPacking: THREE.RGBADepthPacking,
  });
  customDepthMaterial.depthWrite = false;
  customDepthMaterial.depthTest = false;
  viz.scene.traverse(obj => {
    if (obj instanceof THREE.Mesh) {
      obj.customDepthMaterial = customDepthMaterial;
    }
  });
  loadedWorld.traverse(obj => {
    if (obj instanceof THREE.Mesh) {
      obj.customDepthMaterial = customDepthMaterial;
    }
  });

  let playWalkSound: (materialClass: MaterialClass) => void = () => {};

  delay(0).then(() =>
    initWebSynth({ compositionIDToLoad: 115 }).then(ctx => {
      const getConnectables = () => ctx.getState().viewContextManager.patchNetwork.connectables;

      ctx.startAll();

      const synthDesignerID = '0c1f6c0c-91d6-8b13-c9c7-09bd35863453';
      const synthDesigner = getConnectables().get(synthDesignerID);
      const synthDesignerMailboxID = synthDesigner.inputs.get('midi').node.getInputCbs()
        .enableRxAudioThreadScheduling.mailboxIDs[0];
      playWalkSound = () => {
        ctx.postMIDIEventToAudioThread(synthDesignerMailboxID, 0, 60, 255);
        ctx.scheduleEventTimeRelativeToCurTime(
          0.2,
          () => void ctx.postMIDIEventToAudioThread(synthDesignerMailboxID, 1, 60, 255)
        );
      };

      const reverbID = '59b1de3a-df82-039c-3269-efcf6a42f7c8';

      let wetLevel: ConstantSourceNode | null = null;

      const outside = getConnectables().get('9');
      const inside = getConnectables().get('10');
      const rainGainHandles = {
        outside: outside.node.node.offset as AudioParam,
        inside: inside.node.node.offset as AudioParam,
      };
      viz.registerBeforeRenderCb(() => {
        const y = viz.camera.position.y;
        const z = viz.camera.position.z;

        const outsideFactor = 1 - smoothstep(3, 13.5, y);
        const insideFactorY = smoothstep(9, 15, y);
        const insideFactorZ = y > 5 ? smoothstep(-90, -75, z) : 0;
        const insideFactor = insideFactorY * 0.4 + insideFactorZ * 0.6;

        // -1 = muted, 0 = full
        rainGainHandles.inside.setValueAtTime(insideFactor - 1, 0);
        rainGainHandles.outside.setValueAtTime(outsideFactor - 1, 0);

        if (!wetLevel) {
          wetLevel =
            getConnectables().get(reverbID).inputs.get('wetLevel').node.manualControl ??
            (null as ConstantSourceNode | null);
        }

        if (wetLevel) {
          const MinReverbWetness = 0;
          const MaxReverbWetness = 70;

          const upFactor = smoothstep(2, 8, y);
          const downFactor = smoothstep(9, 15, y);
          const wetLevelVal =
            MinReverbWetness +
            upFactor * (MaxReverbWetness - MinReverbWetness) +
            downFactor * (MinReverbWetness - MaxReverbWetness);

          wetLevel.offset.setValueAtTime(wetLevelVal, 0);
        }
      });
    })
  );

  return {
    locations,
    spawnLocation: 'spawn',
    gravity: 22,
    player: {
      jumpVelocity: 0,
      dashConfig: { enable: false },
      colliderSize: {
        height: 1.35,
        radius: 0.3,
      },
      moveSpeed: {
        onGround: 3.9,
        inAir: 0,
      },
    },
    debugPos: true,
    sfx: {
      walk: {
        playWalkSound: (materialClass: MaterialClass) => playWalkSound(materialClass),
        timeBetweenStepsSeconds: 0.368,
        timeBetweenStepsJitterSeconds: 0.04,
      },
    },
  };
};
