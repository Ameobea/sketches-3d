import * as THREE from 'three';

import type { Viz } from 'src/viz';
import type { VizConfig } from 'src/viz/conf';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { LODTerrain } from 'src/viz/terrain/LODTerrain';
import type { SceneConfig } from '../..';

export const processLoadedScene = async (
  viz: Viz,
  loadedWorld: THREE.Group,
  vizConfig: VizConfig
): Promise<SceneConfig> => {
  viz.scene.background = new THREE.Color(0x030303);
  viz.scene.add(new THREE.AmbientLight(0xffffff, 1.5));
  viz.camera.far = 10000;
  viz.camera.updateProjectionMatrix();

  loadedWorld.children[0].removeFromParent();

  // sine wave test pattern
  const sampleHeight = (x: number, z: number) => Math.sin(x / 20) * Math.cos(z / 20) * 10;

  const terrain = new LODTerrain(
    viz.camera,
    {
      boundingBox: new THREE.Box2(new THREE.Vector2(-5000, -5000), new THREE.Vector2(5000, 5000)),
      maxPolygonWidth: 200,
      minPolygonWidth: 5,
      sampleHeight: { type: 'simple', fn: sampleHeight },
      tileResolution: 256,
      material: new THREE.MeshStandardMaterial({ color: 0xaaaaaa, wireframe: true }),
      maxPixelsPerPolygon: 100,
    },
    viz.renderer.getSize(new THREE.Vector2())
  );
  viz.scene.add(terrain);
  viz.registerBeforeRenderCb(() => terrain.update());

  configureDefaultPostprocessingPipeline({ viz, quality: vizConfig.graphics.quality });

  return {
    viewMode: {
      type: 'orbit',
      pos: new THREE.Vector3(40, 40, 40),
      target: new THREE.Vector3(),
    },
    locations: {
      spawn: {
        pos: new THREE.Vector3(0, 2, 0),
        rot: new THREE.Vector3(),
      },
    },
    spawnLocation: 'spawn',
    gravity: 30,
    player: {
      moveSpeed: { onGround: 19, inAir: 19 },
      colliderSize: { height: 6.2, radius: 0.8 },
      jumpVelocity: 16,
      oobYThreshold: -50,
    },
    debugPos: true,
  };
};
