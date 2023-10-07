import * as THREE from 'three';

import type { VizState } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { VolumetricPass } from 'src/viz/shaders/volumetric/volumetric';
import { LODTerrain } from 'src/viz/terrain/LODTerrain';
import type { TerrainGenParams } from 'src/viz/terrain/TerrainGenWorker/TerrainGenWorker.worker';
import { loadNamedTextures } from 'src/viz/textureLoading';
import { getTerrainGenWorker } from 'src/viz/workerPool';
import type { SceneConfig } from '..';

export const processLoadedScene = async (
  viz: VizState,
  loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  viz.renderer.shadowMap.enabled = true;
  // viz.renderer.shadowMap.type = THREE.VSMShadowMap;

  viz.scene.add(new THREE.AmbientLight(0xffffff, 0.6));
  const sun = new THREE.DirectionalLight(0x4488bb, 1.6);
  sun.castShadow = true;
  sun.shadow.mapSize.width = 2048 * 4;
  sun.shadow.mapSize.height = 2048 * 4;
  sun.shadow.camera.near = 0.5;
  sun.shadow.camera.far = 1000;
  sun.shadow.camera.left = -500;
  sun.shadow.camera.right = 500;
  sun.shadow.camera.top = 200;
  sun.shadow.camera.bottom = -100;
  sun.shadow.bias = 0.0002;
  // sun.shadow.normalBias = 0.2;
  sun.shadow.radius = 4;
  sun.shadow.blurSamples = 64;
  sun.position.set(-330, 90, 330);
  sun.shadow.camera.position.copy(sun.position);
  sun.target.position.set(100, 0, 0);
  sun.shadow.camera.lookAt(sun.target.position);
  sun.target.updateMatrixWorld();
  sun.matrixAutoUpdate = true;
  sun.updateMatrixWorld();

  sun.shadow.camera.updateProjectionMatrix();
  sun.shadow.camera.updateMatrixWorld();
  viz.scene.add(sun);
  viz.scene.add(sun.target);

  // // helper for sun
  // const helper = new THREE.DirectionalLightHelper(sun, 5);
  // viz.scene.add(helper);

  // // helper for sun camera
  // const helper2 = new THREE.CameraHelper(sun.shadow.camera);
  // viz.scene.add(helper2);

  const loader = new THREE.ImageBitmapLoader();
  const {
    // stoneBricksAlbedo,
    // stoneBricksNormal,
    // stoneBricksRoughness,
    cloudsBackground,
    gemTexture,
    gemRoughness,
    gemNormal,
    glossyBlackBricksColor,
    glossyBlackBricksNormal,
    glossyBlackBricksRoughness,
    goldFleckedObsidianColor,
    goldFleckedObsidianNormal,
    goldFleckedObsidianRoughness,
  } = await loadNamedTextures(loader, {
    // stoneBricksAlbedo: '/textures/stone_wall/color_map.jpg',
    // stoneBricksNormal: '/textures/stone_wall/normal_map_opengl.jpg',
    // stoneBricksRoughness: '/textures/stone_wall/roughness_map.jpg',
    cloudsBackground: 'https://i.ameo.link/ame.jpg',
    // cloudsBackground: '/textures/00005.jpg',
    gemTexture: 'https://i.ameo.link/bfy.jpg',
    gemRoughness: 'https://i.ameo.link/bfz.jpg',
    gemNormal: 'https://i.ameo.link/bg0.jpg',
    glossyBlackBricksColor: 'https://i.ameo.link/bip.jpg',
    glossyBlackBricksNormal: 'https://i.ameo.link/biq.jpg',
    glossyBlackBricksRoughness: 'https://i.ameo.link/bir.jpg',
    goldFleckedObsidianColor: 'https://i.ameo.link/biv.jpg',
    goldFleckedObsidianNormal: 'https://i.ameo.link/biw.jpg',
    goldFleckedObsidianRoughness: 'https://i.ameo.link/bix.jpg',
  });

  cloudsBackground.mapping = THREE.EquirectangularReflectionMapping;
  cloudsBackground.magFilter = THREE.LinearFilter;
  cloudsBackground.minFilter = THREE.LinearFilter;
  cloudsBackground.generateMipmaps = false;
  viz.scene.background = cloudsBackground;

  const stoneBricks = loadedWorld.getObjectByName('minecraft_block-stone_bricks') as THREE.Mesh;
  const stoneBricksMaterial = buildCustomShader(
    {
      map: glossyBlackBricksColor,
      normalMap: glossyBlackBricksNormal,
      roughnessMap: glossyBlackBricksRoughness,
      metalness: 0.7,
      roughness: 0.7,
      uvTransform: new THREE.Matrix3().scale(0.1, 0.1),
      iridescence: 0.4,
    },
    {},
    {
      useGeneratedUVs: true,
      randomizeUVOffset: true,
      tileBreaking: { type: 'neyret', patchScale: 0.3 },
    }
  );
  stoneBricks.material = stoneBricksMaterial;

  const cobble = loadedWorld.getObjectByName('minecraft_block-cobblestone') as THREE.Mesh;
  cobble.material = stoneBricksMaterial;

  const smoothStoneSlabs = loadedWorld.getObjectByName(
    'minecraft_block-smooth_stone_slab_side'
  ) as THREE.Mesh;
  smoothStoneSlabs.material = stoneBricksMaterial;

  const smoothStone = loadedWorld.getObjectByName('minecraft_block-smooth_stone') as THREE.Mesh;
  smoothStone.material = stoneBricksMaterial;

  const stairsPointLight = new THREE.PointLight(0x6ef5f3, 1.5, 50, 0);
  stairsPointLight.castShadow = true;
  stairsPointLight.shadow.mapSize.width = 512;
  stairsPointLight.shadow.mapSize.height = 512;
  stairsPointLight.shadow.camera.near = 0.5;
  stairsPointLight.shadow.camera.far = 50;
  stairsPointLight.position.set(-296.092529296875, 44.4, 271.1970947265625);
  viz.scene.add(stairsPointLight);

  const stairsLightFixture = loadedWorld.getObjectByName('stairs_light_fixture') as THREE.Mesh;
  stairsLightFixture.castShadow = false;
  stairsLightFixture.receiveShadow = false;
  stairsLightFixture.userData.noLight = true;
  stairsLightFixture.material = buildCustomShader(
    {
      map: gemTexture,
      normalMap: gemNormal,
      roughnessMap: gemRoughness,
      metalness: 0.9,
      roughness: 1.5,
      uvTransform: new THREE.Matrix3().scale(0.4, 0.4),
      iridescence: 0.6,
      color: new THREE.Color(stairsPointLight.color),
      ambientLightScale: 80,
    },
    {},
    { useGeneratedUVs: true }
  );

  const monolithMaterial = buildCustomShader(
    {
      map: goldFleckedObsidianColor,
      normalMap: goldFleckedObsidianNormal,
      roughnessMap: goldFleckedObsidianRoughness,
      metalness: 0.99,
      roughness: 0.87,
      uvTransform: new THREE.Matrix3().scale(10.35, 10.35),
      iridescence: 0.2,
      mapDisableDistance: null,
      color: new THREE.Color(0xaaaaaa),
      ambientLightScale: 3,
    },
    {},
    {
      // useGeneratedUVs: true,
      randomizeUVOffset: false,
      // tileBreaking: { type: 'neyret', patchScale: 1.3 },
      // useTriplanarMapping: true,
    }
  );

  const monolith = loadedWorld.getObjectByName('monolith') as THREE.Mesh;
  monolith.material = monolithMaterial;

  const monolithProngs = loadedWorld.getObjectByName('monolith_prongs') as THREE.Mesh;
  monolithProngs.material = monolithMaterial;

  loadedWorld.traverse(c => {
    if (
      c instanceof THREE.Mesh &&
      (c.name.startsWith('monolith_strut') ||
        c.name.startsWith('monolith_bridge') ||
        c.name.startsWith('monolith_base') ||
        c.name.startsWith('monolith_shard'))
    ) {
      c.material = monolithMaterial;
    }
  });

  viz.scene.fog = new THREE.FogExp2(0x000000, 0.0005);

  const terrainGenWorker = await getTerrainGenWorker();
  const ctxPtr = await terrainGenWorker.createTerrainGenCtx();

  const params: TerrainGenParams = {
    variant: {
      OpenSimplex: {
        coordinate_scales: [0.002, 0.005, 0.01, 0.02, 0.04, 0.08, 0.16, 0.32],
        weights: [15, 7, 2, 2, 0.5, 0.25, 0.125, 0.0625],
        seed: 3534,
      },
    },
    magnitude: 4,
  };
  await terrainGenWorker.setTerrainGenParams(ctxPtr, params);

  const terrainMaterial = buildCustomShader(
    {
      map: goldFleckedObsidianColor,
      normalMap: goldFleckedObsidianNormal,
      roughnessMap: goldFleckedObsidianRoughness,
      metalness: 0.3,
      roughness: 0.97,
      uvTransform: new THREE.Matrix3().scale(0.35, 0.35),
      iridescence: 0.2,
      mapDisableDistance: null,
      color: new THREE.Color(0xaaaaaa),
    },
    {},
    {
      useGeneratedUVs: true,
      randomizeUVOffset: false,
      tileBreaking: { type: 'neyret', patchScale: 1.3 },
    }
  );

  const viewportSize = viz.renderer.getSize(new THREE.Vector2());
  const terrain = new LODTerrain(
    viz.camera,
    {
      boundingBox: new THREE.Box2(new THREE.Vector2(-2000, -2000), new THREE.Vector2(2000, 2000)),
      maxPolygonWidth: 2000,
      minPolygonWidth: 1,
      sampleHeight: {
        type: 'batch',
        fn: (resolution, worldSpaceBounds) =>
          terrainGenWorker.genHeightmap(ctxPtr, resolution, worldSpaceBounds),
      },
      tileResolution: 64,
      maxPixelsPerPolygon: 10,
      material: terrainMaterial,
    },
    viewportSize
  );
  viz.scene.add(terrain);
  viz.registerBeforeRenderCb(() => terrain.update());
  viz.collisionWorldLoadedCbs.push(fpCtx => terrain.initializeCollision(fpCtx));

  // render one frame to populate shadow map
  viz.renderer.shadowMap.needsUpdate = true;
  sun.shadow.needsUpdate = true;
  viz.renderer.render(viz.scene, viz.camera);

  // disable shadow map updates for the rest of the scene
  viz.renderer.shadowMap.autoUpdate = false;
  viz.renderer.shadowMap.needsUpdate = false;
  sun.shadow.needsUpdate = false;

  configureDefaultPostprocessingPipeline(viz, vizConf.graphics.quality, (composer, viz, quality) => {
    const volumetricPass = new VolumetricPass(
      viz.scene,
      viz.camera,
      {
        [GraphicsQuality.Low]: {
          maxRayLength: 200,
          minStepLength: 0.4,
          baseRaymarchStepCount: 35,
          noiseBias: 0.7,
          heightFogFactor: 0.24,
        },
        [GraphicsQuality.Medium]: {
          maxRayLength: 200,
          minStepLength: 0.3,
          baseRaymarchStepCount: 60,
          heightFogFactor: 0.2,
        },
        [GraphicsQuality.High]: {
          maxRayLength: 300,
          minStepLength: 0.2,
          baseRaymarchStepCount: 80,
          heightFogFactor: 0.13,
        },
      }[quality]
    );
    composer.addPass(volumetricPass);
    viz.registerBeforeRenderCb(curTimeSeconds => volumetricPass.setCurTimeSeconds(curTimeSeconds));
  });

  return {
    viewMode: { type: 'firstPerson' },
    spawnLocation: 'spawn',
    gravity: 30,
    player: {
      movementAccelPerSecond: { onGround: 9, inAir: 9 },
      colliderCapsuleSize: { height: 2.2, radius: 0.8 },
      jumpVelocity: 12,
      oobYThreshold: -210,
    },
    debugPos: true,
    locations: {
      spawn: {
        pos: [-196.76904296875, 51.176124572753906, 244.1184539794922],
        rot: [-0.10679632679489452, -12.479999999999633, 0],
      },
      stairs: {
        pos: [-302.592529296875, 46, 272.8970947265625],
        rot: [-0.660796326794895, -14.597999999999562, 0],
      },
      base: {
        pos: [-63.28660583496094, 37.54802322387695, 47.51081085205078],
        rot: [-0.04679632679489448, -13.469999999999843, 0],
      },
    },
  };
};
