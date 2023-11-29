import { writable } from 'svelte/store';
import * as THREE from 'three';

import type { VizState } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { VolumetricPass } from 'src/viz/shaders/volumetric/volumetric';
import { generateNormalMapFromTexture, loadNamedTextures, loadTexture } from 'src/viz/textureLoading';
import type { SceneConfig } from '..';
import BridgeMistColorShader from '../../shaders/bridge2/bridge_top_mist/color.frag?raw';

const locations = {
  spawn: {
    pos: new THREE.Vector3(2, 2, 6),
    rot: new THREE.Vector3(-0.1, 1.378, 0),
  },
  '3': {
    pos: new THREE.Vector3(-73.322, 27.647, -33.4451),
    rot: new THREE.Vector3(-0.212, -8.5, 0),
  },
};

const buildMaterials = async (viz: VizState) => {
  const loader = new THREE.ImageBitmapLoader();
  const towerPlinthPedestalTextureP = loadTexture(
    loader,
    'https://pub-80300747d44d418ca912329092f69f65.r2.dev/img-samples/000005.1476533049.png'
  );
  const towerPlinthPedestalTextureCombinedDiffuseNormalTextureP = towerPlinthPedestalTextureP.then(
    towerPlinthPedestalTexture => generateNormalMapFromTexture(towerPlinthPedestalTexture, {}, true)
  );

  const bgTextureP = (async () => {
    const bgImage = await loader.loadAsync('https://i.ameo.link/bqn.jpg');
    const bgTexture = new THREE.Texture(bgImage);
    bgTexture.mapping = THREE.EquirectangularRefractionMapping;
    bgTexture.needsUpdate = true;
    return bgTexture;
  })();

  const [
    bgTexture,
    towerPlinthPedestalTextureCombinedDiffuseNormalTexture,
    {
      shinyPatchworkStoneAlbedo,
      shinyPatchworkStoneNormal,
      shinyPatchworkStoneRoughness,
      greenMosaic2Albedo,
      greenMosaic2Normal,
      greenMosaic2Roughness,
    },
  ] = await Promise.all([
    bgTextureP,
    towerPlinthPedestalTextureCombinedDiffuseNormalTextureP,
    loadNamedTextures(loader, {
      shinyPatchworkStoneAlbedo: 'https://i.ameo.link/bqk.jpg',
      shinyPatchworkStoneNormal: 'https://i.ameo.link/bqm.jpg',
      shinyPatchworkStoneRoughness: 'https://i.ameo.link/bql.jpg',
      greenMosaic2Albedo: 'https://i.ameo.link/bqh.jpg',
      greenMosaic2Normal: 'https://i.ameo.link/bqi.jpg',
      greenMosaic2Roughness: 'https://i.ameo.link/bqj.jpg',
    }),
  ]);

  const pylonMaterial = buildCustomShader(
    {
      color: new THREE.Color(0x898989),
      metalness: 0.18,
      roughness: 0.82,
      map: towerPlinthPedestalTextureCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(0.8, 0.8),
      mapDisableDistance: null,
      normalScale: 5.2,
    },
    {},
    {
      usePackedDiffuseNormalGBA: true,
      useGeneratedUVs: true,
      randomizeUVOffset: true,
      tileBreaking: { type: 'neyret', patchScale: 0.9 },
    }
  );

  const checkpointMat = buildCustomShader(
    { metalness: 0, alphaTest: 0.05, transparent: true },
    { colorShader: BridgeMistColorShader },
    { disableToneMapping: true }
  );
  viz.registerBeforeRenderCb(curTimeSeconds => checkpointMat.setCurTimeSeconds(curTimeSeconds));

  const shinyPatchworkStoneMaterial = buildCustomShader(
    {
      color: new THREE.Color(0xffffff),
      metalness: 0.5,
      roughness: 0.5,
      map: shinyPatchworkStoneAlbedo,
      normalMap: shinyPatchworkStoneNormal,
      roughnessMap: shinyPatchworkStoneRoughness,
      uvTransform: new THREE.Matrix3().scale(0.8, 0.8),
      mapDisableDistance: null,
      normalScale: 1.2,
      ambientLightScale: 2,
    },
    {},
    {
      useGeneratedUVs: true,
      randomizeUVOffset: true,
      tileBreaking: { type: 'neyret', patchScale: 0.9 },
    }
  );

  const greenMosaic2Material = buildCustomShader(
    {
      color: new THREE.Color(0xffffff),
      metalness: 0.5,
      roughness: 0.5,
      map: greenMosaic2Albedo,
      normalMap: greenMosaic2Normal,
      roughnessMap: greenMosaic2Roughness,
      uvTransform: new THREE.Matrix3().scale(0.8, 0.8),
      mapDisableDistance: null,
      normalScale: 2.2,
      ambientLightScale: 2,
    },
    {},
    {
      useTriplanarMapping: true,
    }
  );

  return {
    pylonMaterial,
    checkpointMat,
    bgTexture,
    shinyPatchworkStoneMaterial,
    greenMosaic2Material,
  };
};

interface InitCollectablesArgs {
  viz: VizState;
  loadedWorld: THREE.Group;
  collectableName: string;
  onCollect: (obj: THREE.Mesh) => void;
  material: THREE.Material;
  collisionRegionScale?: THREE.Vector3;
  type?: 'mesh' | 'convexHull';
}

const initCollectables = ({
  viz,
  loadedWorld,
  collectableName,
  onCollect,
  material,
  collisionRegionScale,
  type = 'mesh',
}: InitCollectablesArgs) => {
  const collectables: THREE.Mesh[] = [];
  loadedWorld.traverse(obj => {
    if (obj instanceof THREE.Mesh && obj.name.includes(collectableName)) {
      obj.userData.nocollide = true;
      collectables.push(obj);
      obj.material = material;
    }
  });

  const reachedCollectables: Set<THREE.Mesh> = new Set();
  viz.collisionWorldLoadedCbs.push(fpCtx => {
    for (const collectable of collectables) {
      fpCtx.addPlayerRegionContactCb(
        {
          type,
          mesh: collectable,
          scale: collisionRegionScale,
        },
        () => {
          if (!reachedCollectables.has(collectable)) {
            reachedCollectables.add(collectable);
            collectable.visible = false;
            onCollect(collectable);
          }
        }
      );
    }
  });

  return reachedCollectables;
};

const initCheckpoints = (
  viz: VizState,
  loadedWorld: THREE.Group<THREE.Object3DEventMap>,
  setSpawnPoint: (pos: THREE.Vector3, rot: THREE.Vector3) => void,
  checkpointMat: THREE.Material
) => {
  return initCollectables({
    viz,
    loadedWorld,
    collectableName: 'checkpoint',
    onCollect: checkpoint => {
      setSpawnPoint(
        checkpoint.position,
        new THREE.Vector3(viz.camera.rotation.x, viz.camera.rotation.y, viz.camera.rotation.z)
      );
      // TODO: sfx
    },
    material: checkpointMat,
    collisionRegionScale: new THREE.Vector3(1, 30, 1),
  });
};

const initDashTokens = (viz: VizState, loadedWorld: THREE.Group, dashTokenMaterial: THREE.Material) => {
  const dashCharges = writable(0);
  initCollectables({
    viz,
    loadedWorld,
    collectableName: 'dash',
    onCollect: () => {
      dashCharges.update(n => n + 1);
      // TODO: sfx
    },
    material: dashTokenMaterial,
    type: 'convexHull',
  });
  return dashCharges;
};

export const processLoadedScene = async (
  viz: VizState,
  loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  const ambientLight = new THREE.AmbientLight(0xffffff, 0.8);
  viz.scene.add(ambientLight);

  const { pylonMaterial, checkpointMat, bgTexture, shinyPatchworkStoneMaterial, greenMosaic2Material } =
    await buildMaterials(viz);

  viz.scene.background = bgTexture;

  const sunPos = new THREE.Vector3(200, 290, -135);
  const sunLight = new THREE.DirectionalLight(0xffffff, 3.6);
  sunLight.position.copy(sunPos);
  viz.scene.add(sunLight);

  loadedWorld.traverse(obj => {
    if (!(obj instanceof THREE.Mesh)) {
      return;
    }

    if (obj.name.includes('sparkle')) {
      obj.material = shinyPatchworkStoneMaterial;
    } else {
      obj.material = pylonMaterial;
    }
  });

  const setSpawnPoint = (pos: THREE.Vector3, rot: THREE.Vector3) => viz.fpCtx!.setSpawnPos(pos, rot);
  initCheckpoints(viz, loadedWorld, setSpawnPoint, checkpointMat);

  const curDashCharges = initDashTokens(viz, loadedWorld, greenMosaic2Material);

  configureDefaultPostprocessingPipeline(viz, vizConf.graphics.quality, (composer, viz, quality) => {
    const volumetricPass = new VolumetricPass(viz.scene, viz.camera, {
      fogMinY: -20,
      fogMaxY: -4,
      fogColorLowDensity: new THREE.Vector3(0.2, 0.2, 0.2),
      fogColorHighDensity: new THREE.Vector3(0.8, 0.8, 0.8),
      ambientLightColor: new THREE.Color(0xffffff),
      ambientLightIntensity: 1.2,
      heightFogStartY: -20,
      heightFogEndY: -8,
      maxRayLength: 200,
      minStepLength: 0.1,
      noiseBias: 1.2,
      heightFogFactor: 0.24,
      ...{
        [GraphicsQuality.Low]: { baseRaymarchStepCount: 38 },
        [GraphicsQuality.Medium]: { baseRaymarchStepCount: 60 },
        [GraphicsQuality.High]: { baseRaymarchStepCount: 75 },
      }[quality],
    });
    composer.addPass(volumetricPass);
    viz.registerBeforeRenderCb(curTimeSeconds => volumetricPass.setCurTimeSeconds(curTimeSeconds));
  });

  return {
    spawnLocation: 'spawn',
    gravity: 30,
    player: {
      moveSpeed: { onGround: 10, inAir: 13 },
      colliderCapsuleSize: { height: 2.2, radius: 0.8 },
      jumpVelocity: 12,
      oobYThreshold: -10,
      dashConfig: {
        enable: true,
        chargeConfig: { curCharges: curDashCharges },
      },
    },
    debugPos: true,
    debugPlayerKinematics: true,
    locations,
    legacyLights: false,
  };
};
