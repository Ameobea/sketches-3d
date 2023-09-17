import * as THREE from 'three';

import type { VizState } from 'src/viz';
import type { VizConfig } from 'src/viz/conf';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { loadNamedTextures } from 'src/viz/textureLoading';
import type { SceneConfig } from '../..';
import { configurePostprocessing } from './postprocessing';

export const processLoadedScene = async (
  viz: VizState,
  loadedWorld: THREE.Group,
  vizConfig: VizConfig
): Promise<SceneConfig> => {
  viz.scene.background = new THREE.Color(0x000000);
  viz.scene.add(new THREE.AmbientLight(0xffffff, 0.5));
  viz.camera.near = 0.1;
  viz.camera.far = 1000;
  viz.camera.updateProjectionMatrix();

  const loader = new THREE.ImageBitmapLoader();
  const { groundDiffuse, groundTextureNormal, groundTextureRoughness } = await loadNamedTextures(loader, {
    // groundDiffuse: 'https://i.ameo.link/bet.jpg',
    // groundTextureNormal: 'https://i.ameo.link/beu.jpg',
    // groundTextureRoughness: 'https://i.ameo.link/bev.jpg',
    groundDiffuse: '/bet.jpg',
    groundTextureNormal: '/beu.jpg',
    groundTextureRoughness: '/bev.jpg',
  });
  const ground = loadedWorld.getObjectByName('Cube') as THREE.Mesh;
  ground.material = buildCustomShader(
    {
      map: groundDiffuse,
      normalMap: groundTextureNormal,
      roughnessMap: groundTextureRoughness,
      // metalness: 0.9,
      uvTransform: new THREE.Matrix3().scale(9.8982, 9.8982),
      color: new THREE.Color(0x222222),
    },
    {},
    {}
  );

  configurePostprocessing(viz, vizConfig.graphics.quality);

  return {
    viewMode: {
      type: 'firstPerson',
    },
    locations: {
      spawn: {
        pos: new THREE.Vector3(0, 2, 0),
        rot: new THREE.Vector3(),
      },
    },
    spawnLocation: 'spawn',
    debugPos: true,
    // player: { colliderCapsuleSize: { height: 10, radius: 1 } },
  };
};
