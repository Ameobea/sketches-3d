import * as THREE from 'three';

import type { VizState } from 'src/viz';
import type { VizConfig } from 'src/viz/conf';
import { LODTerrain } from 'src/viz/terrain/LODTerrain';
import type { TerrainGenParams } from 'src/viz/terrain/TerrainGenWorker/TerrainGenWorker.worker';
import { getTerrainGenWorker } from 'src/viz/workerPool';
import type { SceneConfig } from '..';

export const processLoadedScene = async (
  viz: VizState,
  loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  const terrainGenWorker = await getTerrainGenWorker();
  const ctxPtr = await terrainGenWorker.createTerrainGenCtx();

  let params: TerrainGenParams = { Hill: { octaves: 5, wavelengths: [40, 20, 10, 5, 2], seed: 33 } };
  await terrainGenWorker.setTerrainGenParams(ctxPtr, params);

  const viewportSize = viz.renderer.getSize(new THREE.Vector2());
  const buildTerrain = () =>
    new LODTerrain(
      viz.camera,
      {
        boundingBox: new THREE.Box2(new THREE.Vector2(-2000, -2000), new THREE.Vector2(2000, 2000)),
        material: new THREE.MeshBasicMaterial({ color: 0x00ff00 }),
        debugLOD: true,
        // TODO: Configurable
        maxPolygonWidth: 2000,
        minPolygonWidth: 20,
        sampleHeight: {
          type: 'batch',
          fn: (
            resolution: [number, number],
            worldSpaceBounds: {
              mins: [number, number];
              maxs: [number, number];
            }
          ) => terrainGenWorker.genHeightmap(ctxPtr, resolution, worldSpaceBounds),
        },
        tileResolution: 64,
        maxPixelsPerPolygon: 10,
      },
      viewportSize
    );

  let terrain = buildTerrain();
  viz.scene.add(terrain);
  viz.registerResizeCb(() => (terrain.viewportSize = viz.renderer.getSize(new THREE.Vector2())));
  viz.registerBeforeRenderCb(() => terrain.update());

  const updateTerrain = () => {
    viz.scene.remove(terrain);
    terrain = buildTerrain();
    viz.scene.add(terrain);
  };

  const handleParamsChange = async (newParams: TerrainGenParams) => {
    params = newParams;
    await terrainGenWorker.setTerrainGenParams(ctxPtr, params);
    updateTerrain();
  };

  return {
    locations: {
      spawn: {
        pos: new THREE.Vector3(200, 100, 0),
        rot: new THREE.Vector3(0, 0, 0),
      },
    },
    spawnLocation: 'spawn',
    viewMode: {
      type: 'orbit',
      pos: new THREE.Vector3(190.16798130391035, 70.33263180077928, 59.63493635180146),
      target: new THREE.Vector3(167.41573227055312, 37.46032347797772, -81.3361176559136),
    },
  };
};
