import {
  BlendFunction,
  DepthOfFieldEffect,
  EffectComposer,
  EffectPass,
  KernelSize,
  RenderPass,
  SMAAEffect,
  SMAAPreset,
} from 'postprocessing';
import * as THREE from 'three';

import type { VizState } from 'src/viz';
import { DepthPass, MainRenderPass } from 'src/viz/passes/depthPrepass';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import {
  genCrossfadedTexture,
  generateNormalMapFromTexture,
  loadRawTexture,
  loadTexture,
} from 'src/viz/textureLoading';
import type { SceneConfig } from '..';
import { FogEffect } from './fogShader';

const locations = {
  spawn: {
    pos: new THREE.Vector3(0, 0.2, 0),
    rot: new THREE.Vector3(-0.06, -1.514, 0),
  },
};

const loadTextures = async () => {
  const loader = new THREE.ImageBitmapLoader();

  const cementTextureP = loadTexture(loader, 'https://ameo.link/u/amf.png');
  const cementTextureCombinedDiffuseNormalP = cementTextureP.then(cementTexture =>
    generateNormalMapFromTexture(cementTexture, {}, true)
  );

  const cloudsBgTextureP = loadTexture(loader, 'https://ameo.link/u/ame.jpg', {
    mapping: THREE.EquirectangularReflectionMapping,
    minFilter: THREE.NearestFilter,
    magFilter: THREE.NearestFilter,
  });

  const crossfadedCementTextureP = Promise.all([
    loadRawTexture('https://ameo-imgen.ameo.workers.dev/img-samples/000219.2405862953.png'),
    loadRawTexture('https://ameo-imgen.ameo.workers.dev/img-samples/000222.892303155.png'),
    loadRawTexture('https://ameo-imgen.ameo.workers.dev/img-samples/000206.3766963451.png'),
    loadRawTexture('https://ameo-imgen.ameo.workers.dev/img-samples/000212.2646278093.png'),
  ]).then(async textures => genCrossfadedTexture(textures, 0.2, {}));

  const [cementTextureCombinedDiffuseNormal, cloudsBgTexture, crossfadedCementTexture] = await Promise.all([
    cementTextureCombinedDiffuseNormalP,
    cloudsBgTextureP,
    crossfadedCementTextureP,
  ]);

  return {
    cementTextureCombinedDiffuseNormal,
    cloudsBgTexture,
    crossfadedCementTexture,
  };
};

const initScene = async (viz: VizState, loadedWorld: THREE.Group) => {
  const { cementTextureCombinedDiffuseNormal, cloudsBgTexture, crossfadedCementTexture } =
    await loadTextures();

  const backgroundScene = new THREE.Scene();
  backgroundScene.background = cloudsBgTexture;

  const bgAmbientLight = new THREE.AmbientLight(0xffffff, 0.5);
  const fgAmbientLight = new THREE.AmbientLight(0xffffff, 0.5);
  viz.scene.add(fgAmbientLight);
  backgroundScene.add(bgAmbientLight);

  // const fog = new THREE.Fog(0x282828, 0.1, 40);
  // viz.scene.fog = fog;

  // viz.scene.background = cloudsBgTexture;

  const cube = new THREE.Mesh(
    new THREE.BoxGeometry(1, 1, 1),
    new THREE.MeshStandardMaterial({
      color: 0x00ff00,
    })
  );
  cube.position.set(0, 5, 2);
  viz.scene.add(cube);

  // prettier-ignore
  const cementLUT = new Uint8Array([6,5,12,255,21,15,13,255,20,18,26,255,26,21,18,255,30,22,21,255,30,25,22,255,33,27,23,255,34,26,31,255,34,29,27,255,38,30,28,255,37,31,27,255,39,33,31,255,42,35,31,255,41,36,33,255,42,36,31,255,43,36,36,255,45,38,35,255,46,40,35,255,46,41,37,255,48,41,39,255,38,44,51,255,50,42,38,255,49,44,37,255,51,44,41,255,50,44,43,255,52,45,41,255,54,47,41,255,55,47,43,255,55,47,46,255,55,48,45,255,57,50,45,255,58,50,47,255,58,51,50,255,61,52,50,255,59,53,48,255,61,53,49,255,60,54,50,255,62,54,52,255,62,55,48,255,55,55,76,255,64,55,52,255,63,56,51,255,63,58,48,255,64,57,54,255,65,58,53,255,65,57,57,255,66,57,53,255,64,59,55,255,67,59,56,255,66,60,53,255,67,60,56,255,57,62,68,255,72,60,51,255,67,61,57,255,69,61,56,255,70,61,58,255,71,62,60,255,70,63,61,255,70,63,60,255,70,64,57,255,72,63,58,255,71,64,59,255,74,65,61,255,73,65,62,255,73,66,62,255,74,66,61,255,75,66,65,255,77,67,64,255,75,68,64,255,76,68,66,255,77,68,64,255,77,70,66,255,79,70,69,255,78,71,64,255,79,70,68,255,80,71,66,255,79,72,68,255,81,73,71,255,81,73,68,255,81,73,70,255,81,74,71,255,82,74,70,255,84,75,70,255,84,75,72,255,82,76,71,255,85,76,75,255,84,76,74,255,85,78,72,255,86,78,74,255,87,78,73,255,87,78,75,255,87,79,76,255,81,80,91,255,90,79,76,255,87,81,74,255,88,80,79,255,89,80,76,255,87,81,77,255,89,81,77,255,90,81,79,255,89,82,79,255,91,82,82,255,90,83,78,255,92,83,80,255,93,83,80,255,93,84,78,255,92,84,80,255,90,85,81,255,92,84,82,255,91,85,78,255,92,85,79,255,94,85,83,255,93,87,83,255,96,86,83,255,95,87,82,255,94,87,84,255,94,87,82,255,96,88,82,255,96,88,86,255,98,87,86,255,97,88,85,255,97,89,85,255,97,89,84,255,97,90,86,255,99,90,84,255,99,90,88,255,100,90,87,255,98,91,86,255,98,91,88,255,100,91,86,255,100,92,88,255,99,93,89,255,97,93,98,255,102,92,90,255,102,93,92,255,87,95,109,255,99,94,91,255,102,93,88,255,102,94,91,255,102,94,90,255,104,94,90,255,102,95,93,255,102,95,89,255,88,96,120,255,104,95,92,255,103,96,91,255,104,95,90,255,103,97,94,255,104,97,94,255,106,97,93,255,106,97,96,255,106,97,95,255,104,98,94,255,105,98,93,255,106,98,92,255,106,98,95,255,108,98,95,255,106,99,97,255,106,100,96,255,109,99,95,255,100,100,114,255,108,100,97,255,109,100,99,255,107,101,96,255,108,101,97,255,108,101,96,255,111,101,99,255,111,102,97,255,111,102,96,255,109,102,100,255,109,103,95,255,110,103,99,255,112,103,100,255,111,104,99,255,111,105,101,255,113,104,102,255,112,105,102,255,113,104,104,255,111,106,103,255,114,105,102,255,114,105,100,255,114,106,102,255,113,107,101,255,110,107,115,255,114,107,105,255,116,107,105,255,116,107,104,255,114,109,104,255,117,108,103,255,116,108,109,255,105,110,127,255,116,109,105,255,116,109,106,255,119,109,107,255,119,110,106,255,117,111,105,255,119,111,110,255,119,111,106,255,119,111,108,255,118,112,108,255,120,112,110,255,122,112,108,255,120,113,109,255,121,113,107,255,121,114,111,255,121,114,109,255,122,114,109,255,123,114,111,255,122,116,112,255,123,115,114,255,123,116,111,255,123,117,114,255,126,116,113,255,125,117,112,255,125,117,113,255,126,117,115,255,124,118,112,255,126,118,115,255,125,119,114,255,128,119,115,255,128,119,118,255,127,120,116,255,127,120,118,255,128,121,114,255,129,121,116,255,128,121,117,255,131,121,118,255,129,122,120,255,130,122,118,255,128,123,117,255,130,122,122,255,131,123,117,255,130,124,120,255,132,124,119,255,131,124,121,255,131,125,120,255,134,125,123,255,132,126,119,255,133,126,122,255,134,127,121,255,134,127,125,255,136,128,123,255,134,128,124,255,136,128,125,255,135,129,126,255,136,130,125,255,137,130,126,255,139,131,129,255,138,132,128,255,140,133,128,255,142,134,131,255,141,135,131,255,142,136,131,255,144,137,134,255,146,140,136,255,151,145,140,255]);
  const cementMat = buildCustomShader(
    {
      name: 'cement_custom',
      // TODO REVERT
      side: THREE.DoubleSide,
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
      // TODO REVERT
      side: THREE.DoubleSide,
      map: crossfadedCementTexture,
      uvTransform: new THREE.Matrix3().scale(0.03, 0.03),
    },
    {},
    {
      useGeneratedUVs: true,
      randomizeUVOffset: true,
    }
  );

  // const buildings: THREE.Object3D[] = [];
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

    if (lowerName.startsWith('walkway') || lowerName.startsWith('railing_barrier')) {
      obj.material = walkwayMat;
    }

    if (lowerName.startsWith('ivy')) {
      obj.userData.nocollide = true;
    }

    if (lowerName.startsWith('railing')) {
      obj.userData.convexhull = true;
    }
  });

  const buildings = loadedWorld.children.filter(obj => obj.name.startsWith('building'));

  buildings.forEach(building => {
    building.removeFromParent();
    backgroundScene.add(building);
  });

  return backgroundScene;
};

export const processLoadedScene = async (viz: VizState, loadedWorld: THREE.Group): Promise<SceneConfig> => {
  viz.camera.far = 500;
  viz.camera.updateProjectionMatrix();

  const backgroundScene = await initScene(viz, loadedWorld);

  const composer = new EffectComposer(viz.renderer);

  const depthPassMaterial = new THREE.MeshBasicMaterial({ color: new THREE.Color(0xff0000) });
  const backgroundDepthPass = new DepthPass(backgroundScene, viz.camera, depthPassMaterial);
  backgroundDepthPass.renderToScreen = false;
  composer.addPass(backgroundDepthPass);

  const backgroundRenderPass = new MainRenderPass(backgroundScene, viz.camera);
  backgroundRenderPass.renderToScreen = false;
  composer.addPass(backgroundRenderPass);

  const fogEffect = new FogEffect(BlendFunction.SRC);
  const fogEffectPass = new EffectPass(viz.camera, fogEffect);
  fogEffectPass.renderToScreen = false;
  composer.addPass(fogEffectPass);

  const depthOfFieldEffect = new DepthOfFieldEffect(viz.camera, {
    worldFocusDistance: 10,
    worldFocusRange: 50,
    bokehScale: 6,
  });
  depthOfFieldEffect.blurPass.kernelSize = KernelSize.VERY_SMALL;
  const effectsPass = new EffectPass(viz.camera, depthOfFieldEffect);
  effectsPass.renderToScreen = false;
  composer.addPass(effectsPass);

  const foregroundRenderPass = new RenderPass(viz.scene, viz.camera);
  foregroundRenderPass.renderToScreen = false;
  foregroundRenderPass.clear = false;
  foregroundRenderPass.clearPass.enabled = false;
  composer.addPass(foregroundRenderPass);

  const smaaEffect = new SMAAEffect({ preset: SMAAPreset.MEDIUM });
  const smaaPass = new EffectPass(viz.camera, smaaEffect);
  smaaPass.renderToScreen = true;
  composer.addPass(smaaPass);

  viz.renderer.autoClear = false;
  viz.renderer.autoClearColor = false;

  viz.registerResizeCb(() => {
    composer.setSize(viz.renderer.domElement.width, viz.renderer.domElement.height);
  });

  viz.setRenderOverride((timeDiffSeconds: number) => {
    composer.render(timeDiffSeconds);
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

  return {
    locations,
    spawnLocation: 'spawn',
    player: {
      jumpVelocity: 0,
      enableDash: false,
      colliderCapsuleSize: {
        height: 1.25,
        radius: 0.3,
      },
      movementAccelPerSecond: {
        onGround: 3,
        inAir: 1,
      },
    },
    debugPos: true,
  };
};
