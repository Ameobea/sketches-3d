import * as THREE from 'three';

import type { Viz } from 'src/viz';
import type { VizConfig } from 'src/viz/conf';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { buildCustomShader, setDefaultDistanceAmpParams } from 'src/viz/shaders/customShader';
import { loadNamedTextures } from 'src/viz/textureLoading';
import { delay } from 'src/viz/util/util';
import { initWebSynth } from 'src/viz/webSynth';
import type { SceneConfig } from '..';
import { addDecorations } from './decorations';

export const processLoadedScene = async (
  viz: Viz,
  loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  initWebSynth({ compositionIDToLoad: 110 }).then(async wsCtx => {
    console.log(wsCtx);
    await delay(1300);
    wsCtx.setGlobalBpm(20);
    wsCtx.startAll();
  });

  viz.scene.add(new THREE.AmbientLight(0xffffff, 3, 3.5));

  viz.renderer.shadowMap.enabled = false;

  const funnelSpotlight = new THREE.SpotLight(0x612e06, 1.5, 120, 0.07, 0.9, 0);
  funnelSpotlight.position.set(-16, 100, -5);
  funnelSpotlight.target.position.set(-16, 0, -5);
  funnelSpotlight.updateMatrixWorld();
  funnelSpotlight.target.updateMatrixWorld();
  viz.scene.add(funnelSpotlight);

  const fakeSky = new THREE.Mesh(
    new THREE.PlaneGeometry(300, 300),
    new THREE.MeshBasicMaterial({ color: 0x612e06, side: THREE.DoubleSide })
  );
  fakeSky.position.set(-16, 90, -5);
  // Rotate to be parallel with the ground
  fakeSky.rotation.x = Math.PI / 2;
  fakeSky.matrixWorldNeedsUpdate = true;
  viz.scene.add(fakeSky);

  setDefaultDistanceAmpParams({
    ampFactor: 2,
    falloffEndDistance: 30,
    falloffStartDistance: 0.1,
    exponent: 1.5,
  });

  // const spotlightHelper = new THREE.SpotLightHelper(funnelSpotlight);
  // viz.scene.add(spotlightHelper);

  const loader = new THREE.ImageBitmapLoader();
  const {
    caveTexture,
    caveRoughness,
    // caveNormal
  } = await loadNamedTextures(loader, {
    caveTexture: 'https://i.ameo.link/bfj.jpg',
    caveRoughness: 'https://i.ameo.link/bfl.jpg',
    // caveNormal: 'https://i.ameo.link/bfk.jpg',
  });

  const playerPointLight = new THREE.PointLight(0xd1c9ab, 0.75, 50, 0.7);
  viz.scene.add(playerPointLight);

  const cave = loadedWorld.getObjectByName('cave') as THREE.Mesh;

  const caveMat = buildCustomShader(
    {
      map: caveTexture,
      roughnessMap: caveRoughness,
      metalness: 0.94,
      uvTransform: new THREE.Matrix3().scale(0.1, 0.1),
      clearcoat: 0.07,
      clearcoatRoughness: 0.97,
      iridescence: 0.14,
    },
    {},
    { useTriplanarMapping: { sharpenFactor: 8 } }
  );
  cave.material = caveMat;

  // TODO: Should be the same mat as cave probably
  const stalagMat = buildCustomShader(
    {
      color: new THREE.Color(0x888888),
      map: caveTexture,
      // normalScale: 1.8,
      roughnessMap: caveRoughness,
      metalness: 0.94,
      uvTransform: new THREE.Matrix3().scale(0.1, 0.1),
      // clearcoat: 0.07,
      // clearcoatRoughness: 0.97,
      // iridescence: 0.14,
      mapDisableDistance: null,
    },
    {},
    { useTriplanarMapping: true }
  );

  addDecorations(viz, loadedWorld, stalagMat);

  const beforeRenderCb = () => {
    playerPointLight.position.copy(viz.camera.position);
  };
  viz.registerBeforeRenderCb(beforeRenderCb);

  configureDefaultPostprocessingPipeline({ viz, quality: vizConf.graphics.quality });

  return {
    spawnLocation: 'spawn',
    gravity: 30,
    player: {
      moveSpeed: { onGround: 9, inAir: 9 },
      colliderSize: { height: 2.2, radius: 0.8 },
      jumpVelocity: 12,
      oobYThreshold: -210,
      dashConfig: { enable: false },
    },
    debugPos: true,
    locations: {
      spawn: {
        pos: new THREE.Vector3(-15, 80, -5),
        rot: new THREE.Vector3(-1.5707963267948966, -0.013999999999999973, 0),
      },
    },
  };
};
