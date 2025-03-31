import { goto } from '$app/navigation';
import { EffectPass, KernelSize, SelectiveBloomEffect } from 'postprocessing';
import * as THREE from 'three';

import { getSentry } from 'src/sentry';
import type { Viz } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { buildCustomBasicShader } from 'src/viz/shaders/customBasicShader';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { VolumetricPass } from 'src/viz/shaders/volumetric/volumetric';
import { LODTerrain } from 'src/viz/terrain/LODTerrain';
import type { TerrainGenParams } from 'src/viz/terrain/TerrainGenWorker/TerrainGenWorker.worker';
import { loadNamedTextures } from 'src/viz/textureLoading';
import { smoothstepScale } from 'src/viz/util/util';
import { getTerrainGenWorker } from 'src/viz/workerPool';
import type { SceneConfig, SceneLocations } from '..';
import { getRuneGenerator } from './runeGen/runeGen';
import MonolithLightBeamColorShader from './shaders/monolithLightBeam/color.frag?raw';
import TotemBeamColorShader from './shaders/totemBeam/color.frag?raw';

const locations: SceneLocations = {
  spawn: {
    pos: [-196.769, 51.176, 244.118],
    rot: [-0.1068, -12.48, 0],
  },
  stairs: {
    pos: [-302.59253, 46, 272.8971],
    rot: [-0.6608, -14.598, 0],
  },
  base: {
    pos: [-63.2866, 37.548, 47.511],
    rot: [-0.0468, -13.47, 0],
  },
  outside: {
    pos: [177.221, 31.886, 821.6586],
    rot: [-0.1028, 0.252, 0],
  },
  top: {
    pos: [-119.5325, 72.6, 110.954],
    rot: [-0.659, -26.412, 0],
  },
  two: {
    pos: [-0.2293, 35.73, -232.21358],
    rot: [-0.2388, -12.58, 0],
  },
};

const initTerrain = async (
  viz: Viz,
  texturesPromise: Promise<{
    goldFleckedObsidianColor: THREE.Texture;
    goldFleckedObsidianNormal: THREE.Texture;
    goldFleckedObsidianRoughness: THREE.Texture;
  }>
) => {
  const terrainGenWorker = await getTerrainGenWorker();
  const ctxPtr = await terrainGenWorker.createTerrainGenCtx();

  const params: TerrainGenParams = {
    variant: {
      OpenSimplex: {
        coordinate_scales: [0.002, 0.005, 0.01, 0.02, 0.04, 0.08, 0.16, 0.32],
        weights: [15, 7, 2, 2, 0.5, 0.25, 0.125, 0.0625],
        seed: 122152121282581211,
        magnitude: 0.9,
        offset_x: -49,
        offset_z: -15,
      },
    },
    magnitude: 4,
  };
  await terrainGenWorker.setTerrainGenParams(ctxPtr, params);

  const terrainMaterialPromise = texturesPromise.then(t =>
    buildCustomShader(
      {
        map: t.goldFleckedObsidianColor,
        normalMap: t.goldFleckedObsidianNormal,
        roughnessMap: t.goldFleckedObsidianRoughness,
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
    )
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
      material: terrainMaterialPromise,
    },
    viewportSize
  );
  viz.scene.add(terrain);
  viz.registerBeforeRenderCb(() => terrain.update());
  viz.collisionWorldLoadedCbs.push(fpCtx => terrain.initializeCollision(fpCtx));
};

export const processLoadedScene = async (
  viz: Viz,
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
  sun.position.set(-330, 110, 330);
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
  const texturesPromise = loadNamedTextures(loader, {
    cloudsBackground: 'https://i.ameo.link/ame.jpg',
    gemTexture: 'https://i.ameo.link/bfy.jpg',
    gemRoughness: 'https://i.ameo.link/bfz.jpg',
    gemNormal: 'https://i.ameo.link/bg0.jpg',
    glossyBlackBricksColor: 'https://i.ameo.link/bip.jpg',
    glossyBlackBricksNormal: 'https://i.ameo.link/biq.jpg',
    glossyBlackBricksRoughness: 'https://i.ameo.link/bir.jpg',
    goldFleckedObsidianColor: 'https://i.ameo.link/biv.jpg',
    goldFleckedObsidianNormal: 'https://i.ameo.link/biw.jpg',
    goldFleckedObsidianRoughness: 'https://i.ameo.link/bix.jpg',
    goldTextureAlbedo: 'https://i.ameo.link/be0.jpg',
    goldTextureNormal: 'https://i.ameo.link/be2.jpg',
    goldTextureRoughness: 'https://i.ameo.link/bdz.jpg',
    totemAlbedo: 'https://i.ameo.link/bl9.jpg',
    totemNormal: 'https://i.ameo.link/bla.jpg',
    totemRoughness: 'https://i.ameo.link/blb.jpg',
  });

  initTerrain(
    viz,
    texturesPromise.then(
      ({ goldFleckedObsidianColor, goldFleckedObsidianNormal, goldFleckedObsidianRoughness }) => ({
        goldFleckedObsidianColor,
        goldFleckedObsidianNormal,
        goldFleckedObsidianRoughness,
      })
    )
  );

  const monolithBase = loadedWorld.getObjectByName('monolith_base') as THREE.Mesh;
  const runeMatPromise = texturesPromise.then(t =>
    buildCustomShader(
      {
        map: t.goldTextureAlbedo,
        normalMap: t.goldTextureNormal,
        roughnessMap: t.goldTextureRoughness,
        metalness: 0.99,
        roughness: 0.87,
        uvTransform: new THREE.Matrix3().scale(0.35, 0.35),
      },
      {},
      { useTriplanarMapping: true }
    )
  );
  getRuneGenerator().then(async runeGen => {
    const mesh = await runeGen.generateMesh(monolithBase, runeMatPromise);
    mesh.castShadow = true;
    mesh.receiveShadow = true;
    viz.scene.add(mesh);
  });

  const {
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
    totemAlbedo,
    totemNormal,
    totemRoughness,
  } = await texturesPromise;

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
      randomizeUVOffset: false,
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
      ambientLightScale: 30,
    },
    {},
    { randomizeUVOffset: false }
  );

  const stairsPointLight = new THREE.PointLight(0x6ef5f3, 1.1, 50, 0);
  stairsPointLight.castShadow = true;
  stairsPointLight.shadow.mapSize.width = 512;
  stairsPointLight.shadow.mapSize.height = 512;
  stairsPointLight.shadow.camera.near = 0.5;
  stairsPointLight.shadow.camera.far = 50;
  stairsPointLight.position.set(-296.092529296875, 44.4, 271.1970947265625);
  viz.scene.add(stairsPointLight);

  const stairsBottomPointLight = new THREE.PointLight(0x30dba5, 1.0, 120, 0.5);
  stairsBottomPointLight.castShadow = true;
  stairsBottomPointLight.shadow.mapSize.width = 512;
  stairsBottomPointLight.shadow.mapSize.height = 512;
  stairsBottomPointLight.shadow.camera.near = 0.5;
  stairsBottomPointLight.shadow.camera.far = 180;
  stairsBottomPointLight.position.set(-239, 1.5, 259);
  viz.scene.add(stairsBottomPointLight);

  const stairsBottomOutsidePointLight = new THREE.PointLight(0x30dba5, 0.7, 40, 0.5);
  stairsBottomOutsidePointLight.position.set(-238, 5, 235);
  viz.scene.add(stairsBottomOutsidePointLight);

  const stairsBottomPlatform = loadedWorld.getObjectByName('stairs_bottom_platform') as THREE.Mesh;
  stairsBottomPlatform.material = monolithMaterial;

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

  const monolithDoor = loadedWorld.getObjectByName('monolith_door') as THREE.Mesh;
  monolithDoor.material = monolithMaterial;

  // monolith door lights
  const doorLightOffMat = buildCustomShader({ color: new THREE.Color(0xee1111) });
  const doorLightOnMat = buildCustomShader({ color: new THREE.Color(0x11ee11) });
  const doorLights = ([1, 2, 3, 4] as const).map(i => {
    const light = loadedWorld.getObjectByName(`monolith_door_light_${i}`) as THREE.Mesh;
    light.material = doorLightOffMat;
    return light;
  });

  // monolith light beams
  const monolithLightBeamMat = buildCustomBasicShader(
    { color: new THREE.Color(0x11ee11) },
    { colorShader: MonolithLightBeamColorShader }
  );
  monolithLightBeamMat.transparent = true;
  const monolithLightBeams = [1, 2, 3, 4, 5].map(i => {
    const beam = loadedWorld.getObjectByName(`monolith_light_beam_${i}`) as THREE.Mesh;
    beam.material = monolithLightBeamMat;
    beam.visible = false;
    return beam;
  });

  const monolithShardMaterial = buildCustomShader(
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
      ambientLightScale: 30,
      ambientDistanceAmp: { ampFactor: 1.8, exponent: 2, falloffStartDistance: 10, falloffEndDistance: 150 },
    },
    {},
    { randomizeUVOffset: false }
  );

  // totems
  const totemLocations: [number, number, number][] = [
    [-227, 2, 259.7],
    [375.3, 66, 276.3],
    [0.6, 36.9, -240],
    [-114.4, 76, 110],
  ];
  const totemMaterial = buildCustomShader(
    {
      map: totemAlbedo,
      normalMap: totemNormal,
      roughnessMap: totemRoughness,
      metalness: 0.9,
      roughness: 1,
      uvTransform: new THREE.Matrix3().scale(0.5, 0.5),
      ambientLightScale: 8,
      ambientDistanceAmp: { ampFactor: -0.8, exponent: 1, falloffStartDistance: 50, falloffEndDistance: 150 },
    },
    {},
    { useTriplanarMapping: true }
  );

  const addTotemLight = (pos: [number, number, number]) => {
    const light = new THREE.PointLight(0x6ef5f3, 1.5, 80, 2);
    light.castShadow = false;
    light.position.set(pos[0], pos[1], pos[2]);
    viz.scene.add(light);
  };
  [1, 2, 3].forEach(i => addTotemLight(totemLocations[i]));
  const totems: THREE.Mesh[] = [];

  loadedWorld.traverse(c => {
    if (
      c instanceof THREE.Mesh &&
      (c.name.startsWith('monolith_strut') ||
        c.name.startsWith('monolith_bridge') ||
        c.name.startsWith('monolith_base') ||
        c.name.startsWith('monolith_prongs') ||
        c.name === 'monolith' ||
        c.name.startsWith('wall_filler'))
    ) {
      c.material = monolithMaterial;
    } else if (c instanceof THREE.Mesh && c.name.startsWith('monolith_shard')) {
      c.material = monolithShardMaterial;
    } else if (c instanceof THREE.Mesh && c.name.startsWith('totem')) {
      c.material = totemMaterial;
      c.castShadow = false;
      c.userData.nocollide = true;
      c.userData.noLight = true;
      totems.push(c);
    } else if (c.name.startsWith('stairs_invisible')) {
      c.visible = false;
    }
  });

  let lastTotemPickupTimeSeconds = -Infinity;
  const TotemBeamVisibilityWindowSeconds = 30;
  const TotemBeamHeight = 500;
  const totemBeamMat = buildCustomBasicShader(
    { color: new THREE.Color(0x61041b) },
    { colorShader: TotemBeamColorShader }
  );
  totemBeamMat.transparent = true;
  const totemBeams = totems.map(totem => {
    const totemBeam = (monolithLightBeams[0] as THREE.Mesh).clone();
    totemBeam.material = totemBeamMat;
    totemBeam.scale.set(0.2, TotemBeamHeight, 0.2);
    viz.scene.add(totemBeam);
    totemBeam.visible = false;
    totemBeam.position.copy(totem.position.clone().add(new THREE.Vector3(0, TotemBeamHeight, 0)));
    return totemBeam;
  });
  viz.registerBeforeRenderCb(curTimeSeconds => {
    totemBeamMat.setCurTimeSeconds(curTimeSeconds);
    totemBeams.forEach((totemBeam, i) => {
      const shouldShowBeam =
        !totemCollected[i] && curTimeSeconds - lastTotemPickupTimeSeconds < TotemBeamVisibilityWindowSeconds;
      totemBeam.visible = shouldShowBeam;
      if (!shouldShowBeam) {
        return;
      }

      const distanceToPlayer = totems[i].position.distanceTo(viz.camera.position);
      const widthFactor = smoothstepScale(20, TotemBeamHeight, distanceToPlayer, 0.2, 0.8);
      totemBeam.scale.set(widthFactor, TotemBeamHeight, widthFactor);
    });
  });

  const doorLight = new THREE.PointLight(0xee1111, 0.0001, 22, 2.2);
  doorLight.position.set(-9, 40, 16);
  viz.scene.add(doorLight);

  const exitPortalGeom = new THREE.BoxGeometry(6, 6, 6);
  const exitPortalMat = buildCustomShader(
    {
      map: gemTexture,
      normalMap: gemNormal,
      roughnessMap: gemRoughness,
      metalness: 0.9,
      roughness: 1.5,
      uvTransform: new THREE.Matrix3().scale(0.9, 0.9),
      iridescence: 0.6,
      color: new THREE.Color(0xee1111),
      ambientLightScale: 30,
    },
    {},
    {}
  );
  const exitPortal = new THREE.Mesh(exitPortalGeom, exitPortalMat);
  exitPortal.visible = false;
  exitPortal.position.set(-10, 37, 16);
  exitPortal.userData.noLight = true;
  exitPortal.userData.noCollide = true;
  viz.scene.add(exitPortal);
  const nextLevelURL = `/construction${window.location.origin.includes('localhost') ? '' : '.html'}`;
  viz.registerBeforeRenderCb(curTimeSeconds => {
    const addedRotation = 0.015 * (Math.sin(curTimeSeconds) * 0.5 + 0.5) + 0.02;
    exitPortal.rotation.y += addedRotation;
    while (exitPortal.rotation.y > Math.PI * 2) {
      exitPortal.rotation.y -= Math.PI * 2;
    }
  });
  viz.collisionWorldLoadedCbs.push(fpCtx => {
    fpCtx.addPlayerRegionContactCb(
      {
        type: 'box',
        pos: exitPortal.position,
        halfExtents: new THREE.Vector3(
          exitPortalGeom.parameters.width / 2,
          exitPortalGeom.parameters.height / 2,
          exitPortalGeom.parameters.depth / 2
        ),
      },
      () => {
        const curTimeSeconds = viz.clock.getElapsedTime();
        getSentry()?.captureMessage('Stone level completed', { extra: { levelPlayTime: curTimeSeconds } });
        goto(nextLevelURL);
      }
    );
  });

  const handleAllTotemsCollected = () => {
    const door = loadedWorld.getObjectByName('monolith_door') as THREE.Mesh;
    door.visible = false;
    viz.fpCtx!.removeCollisionObject(door.userData.rigidBody);
    delete door.userData.rigidBody;

    doorLight.intensity = 3;

    exitPortal.visible = true;

    monolithLightBeams[4].visible = true;

    getSentry()?.captureMessage('Stone level all totems collected');
  };

  const baseMoveSpeed = 12.8;
  const totemCollected = new Array(totems.length).fill(false);
  const handleTotemCollision = (i: number) => {
    if (totemCollected[i]) {
      return;
    }
    totemCollected[i] = true;
    lastTotemPickupTimeSeconds = viz.clock.getElapsedTime();

    const totem = totems[i];
    totem.visible = false;
    totemBeams[i].visible = false;

    monolithLightBeams[i].visible = true;
    doorLights[i].material = doorLightOnMat;

    if (i === 2) {
      getSentry()?.captureMessage('Stone level jump puzzle totem collected');
      viz.sceneConf.player!.moveSpeed = { inAir: baseMoveSpeed * 1.3, onGround: baseMoveSpeed * 1.3 };
      // animate FOV increase
      const fovChangeDurationSeconds = 0.3;
      const initialFOV = vizConf.graphics.fov;
      const targetFOV = vizConf.graphics.fov + 10;

      let now: number | undefined;
      const cb = (curTimeSeconds: number) => {
        if (now === undefined) {
          now = curTimeSeconds;
          return;
        }

        const t = (curTimeSeconds - now) / fovChangeDurationSeconds;
        if (t >= 1) {
          viz.camera.fov = targetFOV;
          viz.unregisterBeforeRenderCb(cb);
        } else {
          viz.camera.fov = initialFOV + t * (targetFOV - initialFOV);
        }

        viz.camera.updateProjectionMatrix();
      };
      viz.registerBeforeRenderCb(cb);
    }

    if (totemCollected.every(x => x)) {
      handleAllTotemsCollected();
    }
  };

  viz.collisionWorldLoadedCbs.push(fpCtx => {
    totems.forEach(
      (totem, i) =>
        void fpCtx.addPlayerRegionContactCb({ type: 'mesh', mesh: totem }, () => void handleTotemCollision(i))
    );
  });

  // render one frame to populate shadow map
  viz.renderer.shadowMap.needsUpdate = true;
  sun.shadow.needsUpdate = true;
  viz.renderer.render(viz.scene, viz.camera);

  // disable shadow map updates for the rest of the scene
  viz.renderer.shadowMap.autoUpdate = false;
  viz.renderer.shadowMap.needsUpdate = false;
  sun.shadow.needsUpdate = false;

  configureDefaultPostprocessingPipeline(
    viz,
    vizConf.graphics.quality,
    (composer, viz, quality) => {
      const volumetricPass = new VolumetricPass(viz.scene, viz.camera, {
        fogMinY: -50,
        fogMaxY: -4,
        fogDensityMultiplier: 0.046,
        postDensityMultiplier: 4,
        noisePow: 3,
        heightFogStartY: -10,
        heightFogEndY: -4,
        fogColorHighDensity: new THREE.Vector3(0.06, 0.87, 0.53),
        fogColorLowDensity: new THREE.Vector3(0.11, 0.31, 0.7),
        fogFadeOutRangeY: 0.1,
        ...{
          [GraphicsQuality.Low]: {
            maxRayLength: 200,
            minStepLength: 0.3,
            baseRaymarchStepCount: 45,
            noiseBias: 0.7,
            heightFogFactor: 0.24 * 4,
          },
          [GraphicsQuality.Medium]: {
            maxRayLength: 200,
            minStepLength: 0.23,
            baseRaymarchStepCount: 70,
            heightFogFactor: 0.15 * 4,
          },
          [GraphicsQuality.High]: {
            maxRayLength: 350,
            minStepLength: 0.07,
            baseRaymarchStepCount: 110,
            heightFogFactor: 0.14 * 4,
          },
        }[quality],
        halfRes: true,
      });
      composer.addPass(volumetricPass);
      viz.registerBeforeRenderCb(curTimeSeconds => volumetricPass.setCurTimeSeconds(curTimeSeconds));

      const selectiveBloomEffect = new SelectiveBloomEffect(viz.scene, viz.camera, {
        intensity: 2,
        // blendFunction: BlendFunction.LINEAR_DODGE,
        luminanceThreshold: 0,
        kernelSize: KernelSize.LARGE,
        radius: 0.4,
        luminanceSmoothing: 0,
        mipmapBlur: true,
      } as any);
      selectiveBloomEffect.inverted = false;
      selectiveBloomEffect.ignoreBackground = true;
      selectiveBloomEffect.selection.set([...monolithLightBeams, ...totems, ...doorLights, exitPortal]);
      composer.addPass(new EffectPass(viz.camera, selectiveBloomEffect));
    },
    undefined,
    { toneMappingExposure: 1.3 }
  );

  return {
    viewMode: { type: 'firstPerson' },
    spawnLocation: 'spawn',
    gravity: 30,
    player: {
      moveSpeed: { onGround: baseMoveSpeed, inAir: baseMoveSpeed },
      colliderSize: { height: 2.2, radius: 0.8 },
      jumpVelocity: 12,
      oobYThreshold: -210,
    },
    debugPos: true,
    locations,
    goBackOnLoad: false,
  };
};
