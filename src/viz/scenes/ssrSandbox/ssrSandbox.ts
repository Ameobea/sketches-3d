import * as THREE from 'three';

import type { VizState } from 'src/viz';
import type { VizConfig } from 'src/viz/conf';
import type { SceneConfig } from '..';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { buildCustomShader } from 'src/viz/shaders/customShader';

export const processLoadedScene = async (
  viz: VizState,
  _loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  viz.renderer.shadowMap.enabled = false;
  // viz.camera.far = 100;
  // viz.camera.updateProjectionMatrix();

  const ambientLight = new THREE.AmbientLight(0xffffff, 0.8);
  viz.scene.add(ambientLight);

  const dirLight = new THREE.DirectionalLight(0xffffff, 2);
  viz.scene.add(dirLight);

  // add a platform to stand on
  const platformGeo = new THREE.BoxGeometry(50, 30, 50);
  const platformMat = buildCustomShader({
    reflection: { alpha: 0.95 },
    color: 0x003300,
  });
  const platform = new THREE.Mesh(platformGeo, platformMat);
  platform.position.set(0, -20, 0);
  viz.scene.add(platform);

  const ssrMaterial = buildCustomShader({
    // reflection: { alpha: 0.9 },
    color: 0x990099,
  });
  const cube = new THREE.Mesh(new THREE.BoxGeometry(3, 4, 3), ssrMaterial);
  cube.position.set(20, 3, 20);
  viz.scene.add(cube);

  const vanillaCube = new THREE.Mesh(
    new THREE.BoxGeometry(3, 30, 3),
    new THREE.MeshBasicMaterial({ color: 0x0000ff })
  );
  vanillaCube.position.set(10, -10, 10);
  viz.scene.add(vanillaCube);

  viz.collisionWorldLoadedCbs.push(fpCtx => {
    fpCtx.addTriMesh(platform);
  });

  configureDefaultPostprocessingPipeline(viz, vizConf.graphics.quality);

  return {
    spawnLocation: 'spawn',
    gravity: 30,
    player: {
      moveSpeed: { onGround: 10, inAir: 13 },
      colliderCapsuleSize: { height: 2.2, radius: 0.8 },
      jumpVelocity: 12,
      oobYThreshold: -10,
      dashConfig: { enable: true },
    },
    debugPos: true,
    locations: {
      spawn: {
        pos: [1.15471613407135, 8.7756818532943726, -0.19975419342517853],
        rot: [-0.8227963267948929, -48.78199999999914, 0],
      },
    },
    legacyLights: false,
  };
};
