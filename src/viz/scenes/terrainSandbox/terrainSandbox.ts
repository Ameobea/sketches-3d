import * as THREE from 'three';

import type { Viz } from 'src/viz';
import type { VizConfig } from 'src/viz/conf';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { LODTerrain } from 'src/viz/terrain/LODTerrain';
import type { TerrainGenParams } from 'src/viz/terrain/TerrainGenWorker/TerrainGenWorker.worker';
import { loadNamedTextures } from 'src/viz/textureLoading';
import { getTerrainGenWorker } from 'src/viz/workerPool';
import type { SceneConfig } from '..';

export const processLoadedScene = async (
  viz: Viz,
  loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  viz.camera.near = 0.5;
  viz.camera.far = 10000;
  viz.camera.updateProjectionMatrix();

  viz.scene.add(new THREE.AmbientLight(0xffffff, 0.8));
  const sun = new THREE.DirectionalLight(0xffffff, 1);
  sun.castShadow = false;
  viz.scene.add(sun);

  const terrainGenWorker = await getTerrainGenWorker();
  const ctxPtr = await terrainGenWorker.createTerrainGenCtx();

  let params: TerrainGenParams = {
    variant: {
      // Hill: { octaves: 11, wavelengths: [220, 160, 120, 100, 75, 40, 20, 10, 5, 2, 1], seed: 33 },
      OpenSimplex: {
        coordinate_scales: [0.001, 0.005, 0.01, 0.02, 0.04, 0.08, 0.16, 0.32],
        weights: [22, 7, 2, 1, 0.5, 0.25, 0.125, 0.0625],
        seed: 33,
        magnitude: 1,
        offset_x: 0,
        offset_z: 0,
      },
    },
    magnitude: 20,
  };
  await terrainGenWorker.setTerrainGenParams(ctxPtr, params);

  const loader = new THREE.ImageBitmapLoader();
  const { gemTexture, gemRoughness, gemNormal } = await loadNamedTextures(loader, {
    caveTexture: 'https://i.ameo.link/bfj.jpg',
    caveNormal: 'https://i.ameo.link/bfk.jpg',
    caveRoughness: 'https://i.ameo.link/bfl.jpg',
    gemTexture: 'https://i.ameo.link/bfy.jpg',
    gemRoughness: 'https://i.ameo.link/bfz.jpg',
    gemNormal: 'https://i.ameo.link/bg0.jpg',
  });

  const viewportSize = viz.renderer.getSize(new THREE.Vector2());
  const buildTerrain = () =>
    new LODTerrain(
      viz.camera,
      {
        boundingBox: new THREE.Box2(new THREE.Vector2(-2000, -2000), new THREE.Vector2(2000, 2000)),
        material: buildCustomShader(
          {
            map: gemTexture,
            normalMap: gemNormal,
            roughnessMap: gemRoughness,
            metalness: 0.7,
            roughness: 1.5,
            uvTransform: new THREE.Matrix3().scale(0.02, 0.02),
            iridescence: 0.6,
            mapDisableDistance: null,
          },
          {},
          {
            useTriplanarMapping: true,
            randomizeUVOffset: true,
          }
        ),
        // debugLOD: true,
        // TODO: Configurable
        maxPolygonWidth: 2000,
        minPolygonWidth: 10,
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
        tileResolution: 128,
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

  configureDefaultPostprocessingPipeline(viz, vizConf.graphics.quality);

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
