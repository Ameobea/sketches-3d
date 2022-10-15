import * as THREE from 'three';

import type { VizState } from 'src/viz';
import type { SceneConfig } from '..';
import { generateNormalMapFromTexture, loadTexture } from 'src/viz/textureLoading';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { EffectComposer, EffectPass, SMAAEffect } from 'postprocessing';
import { DepthPass, MainRenderPass } from 'src/viz/passes/depthPrepass';

const locations = {
  spawn: {
    pos: new THREE.Vector3(0, 2, 0),
    rot: new THREE.Vector3(-0.06, -1.514, 0),
  },
};

const loadTextures = async () => {
  const loader = new THREE.ImageBitmapLoader();

  const cementTextureP = loadTexture(loader, 'https://ameo.link/u/amd.png');
  const cementTextureCombinedDiffuseNormalP = cementTextureP.then(cementTexture =>
    generateNormalMapFromTexture(cementTexture, {}, true)
  );

  const cloudsBgTextureP = loadTexture(loader, 'https://ameo.link/u/ame.jpg', {
    mapping: THREE.EquirectangularReflectionMapping,
    minFilter: THREE.NearestFilter,
    magFilter: THREE.NearestFilter,
  });

  const [cementTextureCombinedDiffuseNormal, cloudsBgTexture] = await Promise.all([
    cementTextureCombinedDiffuseNormalP,
    cloudsBgTextureP,
  ]);

  return {
    cementTextureCombinedDiffuseNormal,
    cloudsBgTexture,
  };
};

const initScene = async (viz: VizState, loadedWorld: THREE.Group) => {
  const ambientLight = new THREE.AmbientLight(0xffffff, 0.5);
  viz.scene.add(ambientLight);

  const fog = new THREE.Fog(0x282828, 0.1, 40);
  viz.scene.fog = fog;

  const { cementTextureCombinedDiffuseNormal, cloudsBgTexture } = await loadTextures();

  viz.scene.background = cloudsBgTexture;

  const cube = new THREE.Mesh(
    new THREE.BoxGeometry(1, 1, 1),
    new THREE.MeshStandardMaterial({
      color: 0x00ff00,
    })
  );
  cube.position.set(0, 5, 2);
  viz.scene.add(cube);

  const cementMat = buildCustomShader(
    { map: cementTextureCombinedDiffuseNormal, uvTransform: new THREE.Matrix3().scale(0.05, 0.05) },
    {},
    { usePackedDiffuseNormalGBA: true, useGeneratedUVs: true, randomizeUVOffset: true }
  );
  loadedWorld.traverse(obj => {
    if (!(obj instanceof THREE.Mesh)) {
      return;
    }

    if (Array.isArray(obj.material) || !(obj.material instanceof THREE.MeshStandardMaterial)) {
      return;
    }

    if (
      obj.name.startsWith('building') ||
      obj.parent?.name.startsWith('building') ||
      obj.parent?.parent?.name.startsWith('building')
    ) {
      obj.userData.nocollide = true;
    } else {
      console.log(obj.name);
    }

    if (obj.material.name === 'cement') {
      obj.material = cementMat;
    }

    if (obj.name.startsWith('walkway_base')) {
      // TODO: new mat
      obj.material = cementMat;
      obj.userData.convexhull = true;
    }
  });
};

export const processLoadedScene = async (viz: VizState, loadedWorld: THREE.Group): Promise<SceneConfig> => {
  await initScene(viz, loadedWorld);

  const composer = new EffectComposer(viz.renderer);

  const depthPassMaterial = new THREE.MeshBasicMaterial({ color: new THREE.Color(0xff0000) });
  const depthPass = new DepthPass(viz.scene, viz.camera, depthPassMaterial);
  depthPass.renderToScreen = false;
  composer.addPass(depthPass);

  const mainRenderPass = new MainRenderPass(viz.scene, viz.camera);
  // mainRenderPass.renderToScreen = true;
  composer.addPass(mainRenderPass);

  const smaaEffect = new SMAAEffect({});
  const smaaPass = new EffectPass(viz.camera, smaaEffect);
  // smaaPass.renderToScreen = true;
  composer.addPass(smaaPass);

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
      jumpVelocity: 10.5,
      enableDash: false,
      colliderCapsuleSize: {
        height: 1,
        radius: 0.3,
      },
      movementAccelPerSecond: {
        onGround: 8,
        inAir: 5,
      },
    },
  };
};
