import * as THREE from 'three';

import type { VizState } from 'src/viz';
import type { VizConfig } from 'src/viz/conf';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { VolumetricPass } from 'src/viz/shaders/volumetric/volumetric';
import { LODTerrain } from 'src/viz/terrain/LODTerrain';
import { loadNamedTextures } from 'src/viz/textureLoading';
import type { SceneConfig } from '..';

export const processLoadedScene = async (
  viz: VizState,
  loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  viz.renderer.shadowMap.enabled = true;
  viz.renderer.shadowMap.type = THREE.PCFSoftShadowMap;

  viz.scene.add(new THREE.AmbientLight(0xffffff, 0.8));
  const sun = new THREE.DirectionalLight(0x4488bb, 0.6);
  sun.castShadow = true;
  sun.shadow.mapSize.width = 2048;
  sun.shadow.mapSize.height = 2048;
  sun.shadow.camera.near = 0.5;
  sun.shadow.camera.far = 800;
  sun.shadow.camera.left = -500;
  sun.shadow.camera.right = 500;
  sun.shadow.camera.top = 200;
  sun.shadow.camera.bottom = -100;
  sun.shadow.bias = -0.0005;
  sun.position.set(-330, 60, 330);
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

  console.log(sun.shadow.camera, sun, sun.target);

  // // helper for sun
  // const helper = new THREE.DirectionalLightHelper(sun, 5);
  // viz.scene.add(helper);

  // // helper for sun camera
  // const helper2 = new THREE.CameraHelper(sun.shadow.camera);
  // viz.scene.add(helper2);

  console.log(loadedWorld.children);

  const loader = new THREE.ImageBitmapLoader();
  const { stoneBricksAlbedo, stoneBricksNormal, stoneBricksRoughness, cloudsBackground } =
    await loadNamedTextures(loader, {
      stoneBricksAlbedo: '/textures/stone_wall/color_map.jpg',
      stoneBricksNormal: '/textures/stone_wall/normal_map_opengl.jpg',
      stoneBricksRoughness: '/textures/stone_wall/roughness_map.jpg',
      cloudsBackground: 'https://i.ameo.link/ame.jpg',
      // cloudsBackground: '/textures/sky_test.png',
      // cloudsBackground: '/textures/00005.jpg',
    });
  const { gemTexture, gemRoughness, gemNormal } = await loadNamedTextures(loader, {
    caveTexture: 'https://i.ameo.link/bfj.jpg',
    caveNormal: 'https://i.ameo.link/bfk.jpg',
    caveRoughness: 'https://i.ameo.link/bfl.jpg',
    gemTexture: 'https://i.ameo.link/bfy.jpg',
    gemRoughness: 'https://i.ameo.link/bfz.jpg',
    gemNormal: 'https://i.ameo.link/bg0.jpg',
  });

  cloudsBackground.mapping = THREE.EquirectangularReflectionMapping;
  cloudsBackground.magFilter = THREE.LinearFilter;
  cloudsBackground.minFilter = THREE.LinearFilter;
  cloudsBackground.generateMipmaps = false;
  viz.scene.background = cloudsBackground;

  const stoneBricks = loadedWorld.getObjectByName('minecraft_block-stone_bricks') as THREE.Mesh;
  const stoneBricksMaterial = buildCustomShader(
    {
      // map: stoneBricksAlbedo,
      // normalMap: stoneBricksNormal,
      // roughnessMap: stoneBricksRoughness,
      map: gemTexture,
      normalMap: gemNormal,
      roughnessMap: gemRoughness,
      metalness: 0.7,
      roughness: 0.5,
      uvTransform: new THREE.Matrix3().scale(0.2, 0.2),
      iridescence: 0.6,
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

  // sine wave test pattern
  const sampleHeight = (x: number, z: number) => {
    const octave0 = Math.sin(x / 10) * Math.cos(z / 10) * 10;
    return octave0 * 0.4;
    // const octave1 = Math.sin(x / 20) * Math.cos(z / 20) * 10;
    // const octave2 = Math.sin(x / 40) * Math.cos(z / 40) * 5;

    // return (octave0 + octave1 + octave2) * 0.7;
  };

  const viewportSize = viz.renderer.getSize(new THREE.Vector2());
  const terrain = new LODTerrain(
    viz.camera,
    {
      boundingBox: new THREE.Box2(new THREE.Vector2(-2000, -2000), new THREE.Vector2(2000, 2000)),
      maxPolygonWidth: 2000,
      minPolygonWidth: 1,
      sampleHeight: { type: 'simple', fn: sampleHeight },
      tileResolution: 64,
      maxPixelsPerPolygon: 10,
      material: stoneBricksMaterial,
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
    const volumetricPass = new VolumetricPass(viz.scene, viz.camera, {});
    composer.addPass(volumetricPass);
    viz.registerBeforeRenderCb(curTimeSeconds => volumetricPass.setCurTimeSeconds(curTimeSeconds));
  });

  return {
    viewMode: {
      type: 'firstPerson',
      // type: 'orbit',
      // pos: new THREE.Vector3(-15, 80, -5),
      // target: new THREE.Vector3(-15, 0, -5),
    },
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
        pos: new THREE.Vector3(-196.76904296875, 51.176124572753906, 244.1184539794922),
        rot: new THREE.Vector3(-0.10679632679489452, -12.479999999999633, 0),
      },
    },
  };
};
