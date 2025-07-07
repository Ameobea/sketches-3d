import type { Viz } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import * as THREE from 'three';
import { N8AOPostPass } from 'n8ao';
import type { SceneConfig } from '..';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { loadRawTexture, loadTexture } from 'src/viz/textureLoading';
import { MaterialClass, buildCustomShader } from 'src/viz/shaders/customShader';
import { VolumetricPass } from 'src/viz/shaders/volumetric/volumetric';
import { ToneMappingEffect, ToneMappingMode } from 'postprocessing';
import crystalEmissiveShader from './shaders/crystal/emissive.frag?raw';
import { initWebSynth } from 'src/viz/webSynth';

const locations = {
  spawn: {
    pos: [80.12519073486328, 23.87233543395996, 178.36131286621094] as [number, number, number],
    rot: [-0.31479632679489683, 28.273999999999052, 0] as [number, number, number],
  },
  // TARGET: 406, 41, 409
};

const loadTextures = async () => {
  const loader = new THREE.ImageBitmapLoader();
  const glassDiffuseTexPromise = loadTexture(loader, 'https://i.ameo.link/cbg.avif', {});
  const glassNormalTexPromise = loadRawTexture('https://i.ameo.link/cbf.avif');

  const [glassDiffuseTex, glassNormalTex, bgImage, crystalNormalTex] = await Promise.all([
    glassDiffuseTexPromise,
    glassNormalTexPromise,
    loader.loadAsync('https://i.ameo.link/cbr.avif'),
    loader.loadAsync('https://i.ameo.link/cbw.avif'),
  ]);

  const glassNormalMap = new THREE.Texture(
    glassNormalTex,
    THREE.UVMapping,
    THREE.RepeatWrapping,
    THREE.RepeatWrapping,
    THREE.NearestFilter,
    THREE.NearestMipMapLinearFilter,
    THREE.RGBAFormat,
    THREE.UnsignedByteType,
    1
  );
  glassNormalMap.generateMipmaps = true;
  glassNormalMap.needsUpdate = true;
  const crystalNormalMap = new THREE.Texture(
    crystalNormalTex,
    THREE.UVMapping,
    THREE.RepeatWrapping,
    THREE.RepeatWrapping,
    THREE.NearestFilter,
    THREE.NearestMipMapLinearFilter,
    THREE.RGBAFormat,
    THREE.UnsignedByteType,
    1
  );
  crystalNormalMap.generateMipmaps = true;
  crystalNormalMap.needsUpdate = true;

  const bgTexture = new THREE.Texture(
    bgImage,
    THREE.EquirectangularReflectionMapping,
    THREE.RepeatWrapping,
    THREE.RepeatWrapping,
    THREE.NearestFilter,
    THREE.NearestFilter
  );
  bgTexture.needsUpdate = true;

  return { glassDiffuseTex, glassNormalMap, bgTexture, crystalNormalMap };
};

const addCrystals = (
  viz: Viz,
  basaltEngine: typeof import('src/viz/wasmComp/basalt'),
  ctx: number,
  mat: THREE.Material
) => {
  const crystalCount = basaltEngine.basalt_get_crystal_mesh_count(ctx);

  for (let crystalIx = 0; crystalIx < crystalCount; crystalIx++) {
    const indices = basaltEngine.basalt_take_crystal_mesh_indices(ctx, crystalIx);
    const vertices = basaltEngine.basalt_take_crystal_mesh_vertices(ctx, crystalIx);
    const normals = basaltEngine.basalt_take_crystal_mesh_normals(ctx, crystalIx);

    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.BufferAttribute(vertices, 3));
    geometry.setAttribute('normal', new THREE.BufferAttribute(normals, 3));
    geometry.setIndex(new THREE.BufferAttribute(indices, 1));

    const transforms = basaltEngine.basalt_take_crystal_mesh_transforms(ctx, crystalIx);

    const instancedMesh = new THREE.InstancedMesh(geometry, mat, transforms.length / 16);
    instancedMesh.castShadow = true;
    instancedMesh.receiveShadow = true;
    instancedMesh.instanceMatrix.set(transforms);
    viz.scene.add(instancedMesh);

    const collisionMesh = new THREE.Mesh(geometry, mat);
    viz.collisionWorldLoadedCbs.push(fpCtx => {
      for (let meshIx = 0; meshIx < transforms.length / 16; meshIx++) {
        collisionMesh.position.set(
          transforms[meshIx * 16 + 12],
          transforms[meshIx * 16 + 13],
          transforms[meshIx * 16 + 14]
        );
        collisionMesh.quaternion.setFromRotationMatrix(
          new THREE.Matrix4().fromArray(transforms.slice(meshIx * 16, meshIx * 16 + 16))
        );
        fpCtx.addTriMesh(collisionMesh);
      }
    });
  }
};

export const processLoadedScene = async (
  viz: Viz,
  _loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  viz.camera.near = 0.07;
  viz.camera.far = 7000;
  viz.camera.updateProjectionMatrix();

  const basaltEngineP = import('../../wasmComp/basalt').then(async engine => {
    await engine.default();
    return engine;
  });

  const ambientLight = new THREE.AmbientLight(0xffffff, 2.4);
  viz.scene.add(ambientLight);

  // dim pointlight that follows the camera
  const playerPointLight = new THREE.PointLight(0xd1c9ab, 2.75, 50, 0.7);
  playerPointLight.castShadow = false;
  viz.scene.add(playerPointLight);
  const pointLightOffset = new THREE.Vector3(0, 2.2, 0);
  viz.registerBeforeRenderCb(() => playerPointLight.position.copy(viz.camera.position).add(pointLightOffset));

  let playCrystalLandSoundInner: (() => void) | undefined;
  initWebSynth({ compositionIDToLoad: 126 }).then(ctx => {
    const connectables: {
      [key: string]: {
        inputs: { [key: string]: { node: any; type: string } };
        outputs: { [key: string]: { node: any; type: string } };
      };
    } = ctx.getState().viewContextManager.patchNetwork.connectables.toJS();

    for (const { inputs } of Object.values(connectables)) {
      for (const name of Object.keys(inputs)) {
        if (name === 'midi') {
          const synthDesignerMailboxID =
            inputs.midi.node.getInputCbs().enableRxAudioThreadScheduling.mailboxIDs[0];

          playCrystalLandSoundInner = () => {
            ctx.postMIDIEventToAudioThread(synthDesignerMailboxID, 0, 26, 255);
            ctx.scheduleEventTimeRelativeToCurTime(
              0.2,
              () => void ctx.postMIDIEventToAudioThread(synthDesignerMailboxID, 1, 26, 255)
            );
          };
        }
      }
    }

    ctx.setGlobalVolume(11);
  });

  const playCrystalLandSound = () => playCrystalLandSoundInner?.();

  // TODO: use web worker
  const basaltEngine = await basaltEngineP;
  const basaltCtx = basaltEngine.basalt_gen();
  const chunkCount = basaltEngine.basalt_get_chunk_count(basaltCtx);
  const geometries = [];
  for (let chunkIx = 0; chunkIx < chunkCount; chunkIx++) {
    const vertices = basaltEngine.basalt_take_vertices(basaltCtx, chunkIx);
    const indices = basaltEngine.basalt_take_indices(basaltCtx, chunkIx);
    const normals = basaltEngine.basalt_take_normals(basaltCtx, chunkIx);

    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.BufferAttribute(vertices, 3));
    geometry.setAttribute('normal', new THREE.BufferAttribute(normals, 3));
    const needsU32Indices = indices.some(i => i > 65535);
    geometry.setIndex(new THREE.BufferAttribute(needsU32Indices ? indices : new Uint16Array(indices), 1));
    geometries.push(geometry);
  }

  const collisionIndices = basaltEngine.basalt_take_collision_indices(basaltCtx);
  const collisionVertices = basaltEngine.basalt_take_collision_vertices(basaltCtx);

  const debugMat = new THREE.MeshBasicMaterial({ color: 0xff0000, wireframe: true });

  const collisionGeom = new THREE.BufferGeometry();
  collisionGeom.setAttribute('position', new THREE.BufferAttribute(collisionVertices, 3));
  collisionGeom.setIndex(new THREE.BufferAttribute(collisionIndices, 1));
  const collisionMesh = new THREE.Mesh(collisionGeom, debugMat);
  viz.collisionWorldLoadedCbs.push(fpCtx => fpCtx.addTriMesh(collisionMesh));

  const { glassDiffuseTex, glassNormalMap, bgTexture, crystalNormalMap } = await loadTextures();

  viz.scene.background = bgTexture;

  const pillarParams = {
    color: 0x372a20,
    map: glassDiffuseTex,
    roughnessMap: glassDiffuseTex,
    roughness: 1,
    uvTransform: new THREE.Matrix3().scale(0.073, 0.073),
    normalMap: glassNormalMap,
    normalScale: 0.85,
    normalMapType: THREE.TangentSpaceNormalMap,
    mapDisableDistance: null,
    metalness: 0.11,
    iridescence: 0.1,
  };
  const pillarShaders = {
    roughnessShader: `
    float getCustomRoughness(vec3 pos, vec3 normal, float baseRoughness, float curTimeSeconds, SceneCtx ctx) {
      return clamp(1. - baseRoughness + 0.242, 0., 1.);
    }`,
    iridescenceShader: `
    float getCustomIridescence(vec3 pos, vec3 normal, float baseIridescence, float curTimeSeconds, SceneCtx ctx) {
      vec3 noisePos = pos * 0.28 + vec3(curTimeSeconds * 0.004);
      float noiseVal = pow(fbm(noisePos), 2.);
      return baseIridescence + smoothstep(0.3, 0.6, noiseVal) * 0.2;
    }
    `,
    colorShader: `
vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
float downActivation = 1. - smoothstep(-30., 0., pos.y);
vec3 lowColor = vec3(0.204,0.153,0.114)*0.03;
vec3 outColor = mix(baseColor, lowColor, downActivation);
float noiseVal = clamp(pow(fbm(pos * 0.1) * 2.5 - 1., 1.4), 0., 1.);
vec3 noiseColor = vec3(0.204,0.153,0.114)*0.03;
outColor = mix(outColor, noiseColor, noiseVal);
return vec4(outColor, 1.);
}
    `,
  };
  const pillarMat = buildCustomShader(pillarParams, pillarShaders, { useTriplanarMapping: true });
  const distantPillerMat = buildCustomShader(
    {
      ...pillarParams,
      color: 0x271a10,
      uvTransform: new THREE.Matrix3().scale(0.003, 0.003),
      iridescence: 0,
    },
    {
      ...pillarShaders,
      iridescenceShader: undefined,
    },
    { useTriplanarMapping: true }
  );
  viz.registerBeforeRenderCb(curTimeSeconds => pillarMat.setCurTimeSeconds(curTimeSeconds));
  for (const geometry of geometries) {
    const mesh = new THREE.Mesh(geometry, pillarMat);
    mesh.castShadow = true;
    mesh.receiveShadow = true;
    mesh.position.set(0, 0, 0);
    viz.scene.add(mesh);
  }

  const crystalMat = buildCustomShader(
    {
      color: new THREE.Color(0x24170d),
      metalness: 0.89,
      roughness: 0.312,
      ambientLightScale: 2.4,
      normalMap: crystalNormalMap,
      normalScale: 1.5,
      uvTransform: new THREE.Matrix3().scale(0.5, 0.5),
    },
    {
      emissiveShader: crystalEmissiveShader,
    },
    {
      useTriplanarMapping: true,
      materialClass: MaterialClass.Crystal,
    }
  );
  viz.registerBeforeRenderCb(curTimeSeconds => crystalMat.setCurTimeSeconds(curTimeSeconds));

  addCrystals(viz, basaltEngine, basaltCtx, crystalMat);

  const standalonePillarIndices = basaltEngine.basalt_take_standalone_pillar_mesh_indices(basaltCtx);
  const standalonePillarVertices = basaltEngine.basalt_take_standalone_pillar_mesh_vertices(basaltCtx);
  const standalonePillarNormals = basaltEngine.basalt_take_standalone_pillar_mesh_normals(basaltCtx);
  const standalonePillarTransforms =
    basaltEngine.basalt_take_standalone_pillar_instance_transforms(basaltCtx);
  basaltEngine.basalt_free(basaltCtx);

  const standalonePillarGeometry = new THREE.BufferGeometry();
  standalonePillarGeometry.setAttribute('position', new THREE.BufferAttribute(standalonePillarVertices, 3));
  standalonePillarGeometry.setAttribute('normal', new THREE.BufferAttribute(standalonePillarNormals, 3));
  standalonePillarGeometry.setIndex(new THREE.BufferAttribute(standalonePillarIndices, 1));

  const standalonePillars = new THREE.InstancedMesh(
    standalonePillarGeometry,
    distantPillerMat,
    standalonePillarTransforms.length / 16
  );
  standalonePillars.instanceMatrix.set(standalonePillarTransforms);
  standalonePillars.castShadow = false;
  standalonePillars.receiveShadow = false;
  viz.scene.add(standalonePillars);

  const dirLight = new THREE.DirectionalLight(0xffe6e1, 3.6);
  dirLight.position.set(142 * 1.1, 65 * 1.1, 380 * 1.1);
  dirLight.target.position.set(0, 0, 0);

  dirLight.castShadow = true;
  dirLight.shadow.mapSize.width = 2048 * 2;
  dirLight.shadow.mapSize.height = 2048 * 2;
  dirLight.shadow.radius = 4;
  dirLight.shadow.blurSamples = 16;
  viz.renderer.shadowMap.type = THREE.VSMShadowMap;
  dirLight.shadow.bias = -0.0001;

  dirLight.shadow.camera.near = 8;
  dirLight.shadow.camera.far = 460;
  dirLight.shadow.camera.left = -200;
  dirLight.shadow.camera.right = 280;
  dirLight.shadow.camera.top = 34;
  dirLight.shadow.camera.bottom = -120;

  // const shadowCameraHelper = new THREE.CameraHelper(dirLight.shadow.camera);
  // viz.scene.add(shadowCameraHelper);

  dirLight.shadow.camera.updateProjectionMatrix();
  dirLight.shadow.camera.updateMatrixWorld();

  viz.scene.add(dirLight);
  viz.scene.add(dirLight.target);

  configureDefaultPostprocessingPipeline({
    viz,
    quality: vizConf.graphics.quality,
    addMiddlePasses: (composer, viz, quality) => {
      if (vizConf.graphics.quality > GraphicsQuality.Low) {
        const n8aoPass = new N8AOPostPass(
          viz.scene,
          viz.camera,
          viz.renderer.domElement.width,
          viz.renderer.domElement.height
        );
        composer.addPass(n8aoPass);
        n8aoPass.gammaCorrection = false;
        n8aoPass.configuration.intensity = 2;
        n8aoPass.configuration.aoRadius = 5;
        // \/ this breaks rendering and makes the background black if enabled
        // n8aoPass.configuration.halfRes = vizConf.graphics.quality <= GraphicsQuality.Medium;
        n8aoPass.setQualityMode(
          {
            [GraphicsQuality.Low]: 'Performance',
            [GraphicsQuality.Medium]: 'Low',
            [GraphicsQuality.High]: 'High',
          }[vizConf.graphics.quality]
        );
      }

      const volumetricPass = new VolumetricPass(viz.scene, viz.camera, {
        fogMinY: -60,
        fogMaxY: -40,
        fogColorHighDensity: new THREE.Vector3(0.04, 0.024, 0.02),
        fogColorLowDensity: new THREE.Vector3(0.1, 0.1, 0.1),
        ambientLightColor: new THREE.Color(0x4d2424),
        ambientLightIntensity: 1.2,
        heightFogStartY: -70,
        heightFogEndY: -55,
        heightFogFactor: 0.14,
        maxRayLength: 1000,
        minStepLength: 0.1,
        noiseBias: 0.7,
        noisePow: 2.4,
        fogFadeOutRangeY: 8,
        fogFadeOutPow: 0.6,
        fogDensityMultiplier: 0.32,
        postDensityMultiplier: 1.7,
        noiseMovementPerSecond: new THREE.Vector2(4.1, 4.1),
        globalScale: 1,
        halfRes: true,
        compositor: { edgeRadius: 4, edgeStrength: 2 },
        ...{
          [GraphicsQuality.Low]: { baseRaymarchStepCount: 20 },
          [GraphicsQuality.Medium]: { baseRaymarchStepCount: 40 },
          [GraphicsQuality.High]: { baseRaymarchStepCount: 80 },
        }[quality],
      });
      composer.addPass(volumetricPass);
      viz.registerBeforeRenderCb(curTimeSeconds => volumetricPass.setCurTimeSeconds(curTimeSeconds));
    },
    extraParams: { toneMappingExposure: 1.48 },
    postEffects: (() => {
      const toneMappingEffect = new ToneMappingEffect({
        mode: ToneMappingMode.LINEAR,
      });

      return [toneMappingEffect];
    })(),
  });

  return {
    spawnLocation: 'spawn',
    gravity: 30,
    player: {
      moveSpeed: { onGround: 10, inAir: 13 },
      colliderSize: { height: 2.2, radius: 0.8 },
      jumpVelocity: 12,
      oobYThreshold: -20,
      dashConfig: {
        enable: true,
      },
    },
    sfx: { land: { materialLandSounds: { [MaterialClass.Crystal]: playCrystalLandSound } } },
    debugPos: true,
    locations,
    legacyLights: false,
  };
};
