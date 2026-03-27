import * as THREE from 'three';
import { goto } from '$app/navigation';
import { N8AOPostPass } from 'n8ao';

import type { Viz } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import type { SceneConfig } from '..';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { VolumetricPass } from 'src/viz/shaders/volumetric/volumetric';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import {
  buildGrayFossilRockMaterial,
  GrayFossilRockTextures,
} from 'src/viz/materials/GrayFossilRock/GrayFossilRockMaterial';
import { createSignboard, type CreateSignboardArgs } from 'src/viz/helpers/signboardBuilder';
import { configureShadowMap } from 'src/viz/helpers/lights';
import { mix, smoothstep } from 'src/viz/util/util';
import { rwritable } from 'src/viz/util/TransparentWritable';
import { buildCheckpointMaterial } from 'src/viz/materials/Checkpoint/CheckpointMaterial';
import type { CheckpointMaterialOptions } from 'src/viz/materials/Checkpoint/CheckpointMaterial';
import { buildGrayStoneBricksFloorMaterial } from 'src/viz/materials/GrayStoneBricksFloor/GrayStoneBricksFloorMaterial';
import { getAmmoJS } from 'src/viz/collision';
import { MetricsAPI } from 'src/api/client';
import PlatformColorShader from './shaders/platform/color.frag?raw';
import PlatformRoughnessShader from './shaders/platform/roughness.frag?raw';
import { resolve } from '$app/paths';
import type { RouteId } from '$app/types';

const loadTextures = async () => {
  const loader = new THREE.ImageBitmapLoader();

  const bgTextureP = (async () => {
    const bgImage = await loader.loadAsync('https://i.ameo.link/ccl.avif');
    const bgTexture = new THREE.Texture(
      bgImage,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      THREE.SRGBColorSpace
    );
    bgTexture.rotation = Math.PI;
    bgTexture.mapping = THREE.EquirectangularReflectionMapping;
    bgTexture.needsUpdate = true;
    return bgTexture;
  })();

  const platformTexsP = GrayFossilRockTextures.get(loader);
  const platformMatP = buildGrayFossilRockMaterial(loader, {
    color: 0x575a5d,
    ambientLightScale: 1,
    // normalScale: 1.15,
    roughness: 0.97,
    // metalness: 0.9,
  });

  const [platformMat, bgTexture, { platformDiffuse, platformNormal }] = await Promise.all([
    platformMatP,
    bgTextureP,
    platformTexsP,
  ]);

  return { platformMat, bgTexture, platformDiffuse, platformNormal, loader };
};

export const processLoadedScene = async (
  viz: Viz,
  loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  // kick off request for physics engine wasm early.  This normally has to wait until after
  // this function returns, but we know we're going to be first-person so we can start it now
  getAmmoJS();

  const ambientLight = new THREE.AmbientLight(0xffffff, 2.8);
  viz.scene.add(ambientLight);

  const dirLight = new THREE.DirectionalLight(0xdde6f1, 2.2);
  dirLight.position.set(-160, 163, -80);
  dirLight.target.position.set(0, 0, 0);

  dirLight.castShadow = true;
  configureShadowMap({
    light: dirLight,
    renderer: viz.renderer,
    quality: vizConf.graphics.quality,
    mapSize: { low: 1024, medium: 4096, high: 4096 },
    useVsm: true,
  });
  dirLight.shadow.bias = -0.0001;

  dirLight.shadow.camera.near = 8;
  dirLight.shadow.camera.far = 300;
  dirLight.shadow.camera.left = -300;
  dirLight.shadow.camera.right = 380;
  dirLight.shadow.camera.top = 94;
  dirLight.shadow.camera.bottom = -140;

  // const shadowCameraHelper = new THREE.CameraHelper(dirLight.shadow.camera);
  // viz.scene.add(shadowCameraHelper);

  dirLight.shadow.camera.updateProjectionMatrix();
  dirLight.shadow.camera.updateMatrixWorld();

  viz.scene.add(dirLight);
  viz.scene.add(dirLight.target);

  const pointLightPos = new THREE.Vector3(-42.973, -20, -0.20153);
  const pointLightColor = new THREE.Color(0xbd6464);
  const pointLight = new THREE.PointLight(pointLightColor, 1, 0, 0);
  pointLight.castShadow = false;
  pointLight.position.copy(pointLightPos);
  viz.scene.add(pointLight);

  viz.registerBeforeRenderCb(() => {
    const pointLightActivation = 1 - smoothstep(-20, 0, viz.camera.position.y);
    pointLight.intensity = 4 * pointLightActivation;
    pointLight.position.x = mix(pointLightPos.x, viz.camera.position.x, 0.9);
    pointLight.position.z = mix(pointLightPos.z, viz.camera.position.z, 0.9);
  });

  const portalFrames: THREE.Mesh[] = [];
  const portals: THREE.Mesh[] = [];
  loadedWorld.traverse(obj => {
    if (!(obj instanceof THREE.Mesh)) {
      return;
    }

    if (obj.name.startsWith('portalframe')) {
      portalFrames.push(obj);
    } else if (obj.name.startsWith('portal')) {
      portals.push(obj);
    }
  });

  const EASY = [0.4, 0.7, 0.4] as [number, number, number];
  const NORMAL = [0.05, 0.24, 0.98] as [number, number, number];
  const HARD = [0.7, 0.5, 0.04] as [number, number, number];
  const DIFFICULT = [0.8, 0.2, 0.6] as [number, number, number];
  const CHALLENGING = [1.3, 0.3, 0.12] as [number, number, number];
  const BASEMENT_DEFAULT = [0.05, 0.35, 0.4] as [number, number, number];
  const SMOKE_ORANGE = [312 / 320, 112 / 320, 55 / 320] as [number, number, number];
  const BASALT_PURPLE = [0.05, 0.02, 0.7] as [number, number, number];

  const PortalColorByName: Record<string, [number, number, number]> = {
    tutorial: EASY,
    stone: NORMAL,
    pylons: NORMAL,
    movementv2: HARD,
    plats: HARD,
    cornered: CHALLENGING,
    stronghold: DIFFICULT,
    smoke: SMOKE_ORANGE,
    // Basement portals
    basalt: BASALT_PURPLE,
    pinklights: DIFFICULT,
    bridge2: BASEMENT_DEFAULT,
  };

  // ambientLightScale compensates for perceived-luminance differences between portal colors.
  // Rec. 709 luma weights green ~72% and blue only ~7%, so blue-heavy colors need a much
  // larger scale to cross the luminance threshold in the bloom pass. Values are hand-tuned.
  const PortalAmbientScale = new Map<[number, number, number], number>([
    [EASY, 1.86],
    [NORMAL, 12.3], // blue-heavy → very low Rec.709 luma → needs large boost
    [HARD, 2.38],
    [DIFFICULT, 4.4],
    [CHALLENGING, 4.8],
    [BASEMENT_DEFAULT, 2.3],
    [SMOKE_ORANGE, 4.7],
    [BASALT_PURPLE, 22],
  ]);

  // Per-portal noise animation direction overrides. Only needed when the default vec3(0, 1, -3) looks wrong.
  const PortalNoiseDirByName: Record<string, CheckpointMaterialOptions['noiseDir']> = {
    // stone: [-1.5, 0.5, 2],
  };

  // Per-portal noise frequency overrides. Default vec3(3.6, 0.3, 0.6).
  // Lower X to loosen horizontally cramped patterns; raise Y/Z for finer vertical/depth detail.
  const PortalNoiseFreqByName: Record<string, CheckpointMaterialOptions['noiseFreq']> = {
    stone: [2.4, 0.3, 0.6],
  };

  // Euler rotation (XYZ, radians) applied to noise sampling coords for axis-aligned portals.
  // Tilts the noise field so it's never perfectly parallel to the portal quad's world axes,
  // breaking up the flat/degenerate cross-section without needing the portal's actual transform.
  const PortalNoiseRotationByName: Record<string, CheckpointMaterialOptions['noiseRotation']> = {
    stone: [0, 0.8, 0],
    cornered: [0, 0.28, 0],
    stronghold: [0, 0.27, 0],
  };

  // Populated below; used for both bloom proximity and the portal point light.
  // isBasement: portal is in the lower level — excluded from proximity when player is at upper level.
  const portalLightEntries: Array<{ pos: THREE.Vector3; col: THREE.Color; isBasement: boolean }> = [];

  for (const portal of portals) {
    portal.userData.nocollide = true;

    const portalKey = portal.name.split('_')[1];
    const unmappedColor = PortalColorByName[portalKey];
    let color = unmappedColor;
    if (color) {
      // hacky psuedo-saturation
      const intensity = Math.pow(Math.max(...color), 1.8);
      color = color.map(c => (Math.pow(c, 1.8) / intensity) * 1.8) as [number, number, number];
    }
    portal.material = buildCheckpointMaterial(
      viz,
      color,
      { ambientLightScale: PortalAmbientScale.get(unmappedColor) ?? 2 },
      {
        noiseDir: PortalNoiseDirByName[portalKey],
        noiseFreq: PortalNoiseFreqByName[portalKey],
        noiseRotation: PortalNoiseRotationByName[portalKey],
      }
    );
    // it would be good to eventually be able to handle these transparent portals correctly so that the
    // volumetrics show up behind them, but that makes things very complicated with the depth pre-pass
    // and other render passes so isn't worth it for now
    // portal.material.depthWrite = false;
    portal.userData.noLight = true;

    if (!portal.name.includes('_') || !unmappedColor) {
      portal.visible = false;
      continue;
    }

    // Collect position + light color for this visible portal.
    // Normalize saturation-boosted color to max-channel = 1 so the hue is fully saturated
    // regardless of the original channel magnitudes.
    const worldPos = portal.getWorldPosition(new THREE.Vector3());
    const finalColor = color ?? ([0.8, 0.5, 0.6] as [number, number, number]);
    const maxCh = Math.max(...finalColor);
    portalLightEntries.push({
      pos: worldPos,
      col: new THREE.Color(finalColor[0] / maxCh, finalColor[1] / maxCh, finalColor[2] / maxCh),
      // Portals significantly below the upper platform are basement portals.
      // They're excluded from proximity detection when the player is at upper level
      // to prevent their depth below from triggering effects at the surface.
      isBasement: worldPos.y < -10,
    });
  }

  // Single unshadowed point light that snaps to the nearest portal and tints to its color.
  // Starts at zero intensity; proximity callback below drives it.
  const portalPointLight = new THREE.PointLight(0xffffff, 0, 0, 2);
  portalPointLight.castShadow = false;
  viz.scene.add(portalPointLight);

  const { platformMat, bgTexture, platformDiffuse, platformNormal, loader } = await loadTextures();
  viz.scene.background = bgTexture;

  const platform = loadedWorld.getObjectByName('platform') as THREE.Mesh;
  platform.material = platformMat;

  const lowerPlatform = loadedWorld.getObjectByName('lower_platform') as THREE.Mesh;
  lowerPlatform.material = platformMat;
  buildGrayStoneBricksFloorMaterial(
    loader,
    {
      uvTransform: new THREE.Matrix3().scale(0.148, 0.148),
      metalness: 0.513,
      mapDisableDistance: null,
      ambientLightScale: 0.3,
    },
    {
      colorShader: PlatformColorShader,
      roughnessShader: PlatformRoughnessShader,
    },
    { randomizeUVOffset: false }
  ).then(mat => {
    lowerPlatform.material = mat;
  });

  const spawnPlatformMat = buildCustomShader(
    {
      color: 0x373a3d,
      map: platformDiffuse,
      roughness: 0.95,
      metalness: 0.5,
      uvTransform: new THREE.Matrix3().scale(28.2073, 28.2073),
      normalMap: platformNormal,
      normalScale: 1.85,
      normalMapType: THREE.TangentSpaceNormalMap,
      mapDisableDistance: null,
      ambientLightScale: 1,
    },
    {},
    { tileBreaking: { type: 'neyret', patchScale: 2 } }
  );

  const spawnPlatformDarkMat = buildCustomShader(
    {
      color: 0x505558,
      map: platformDiffuse,
      roughness: 1,
      metalness: 0.8,
      uvTransform: new THREE.Matrix3().scale(8.2073, 8.2073),
      normalMap: platformNormal,
      normalScale: 1.05,
      normalMapType: THREE.TangentSpaceNormalMap,
      mapDisableDistance: null,
    },
    {
      roughnessShader: `
float getCustomRoughness(vec3 pos, vec3 normal, float baseRoughness, float curTimeSeconds, SceneCtx ctx) {
  float shinyness = pow(ctx.diffuseColor.b * 43.5, 2.5) * 0.6;
  shinyness = clamp(shinyness, 0.12, 0.58);
  return 1. - shinyness;
}`,
    },
    { tileBreaking: { type: 'neyret', patchScale: 2 } }
  );

  const portalFrameMat = buildCustomShader(
    {
      color: 0x080808,
      uvTransform: new THREE.Matrix3().scale(0.24073, 0.24073),
      normalMap: platformNormal,
      normalScale: 0.75,
      normalMapType: THREE.TangentSpaceNormalMap,
      roughness: 0.7,
      metalness: 0.1,
    },
    {},
    { useGeneratedUVs: true, randomizeUVOffset: true }
  );

  const spawnPlatform = loadedWorld.getObjectByName('spawn_platform') as THREE.Mesh;
  spawnPlatform.material = spawnPlatformMat;

  const spawnPlatformDark = loadedWorld.getObjectByName('spawn_platform_dark') as THREE.Mesh;
  spawnPlatformDark.material = spawnPlatformDarkMat;

  const addPortalFrameSign = (portalFrame: THREE.Mesh, params: CreateSignboardArgs) => {
    const sign = createSignboard({
      width: 5.75,
      height: 3,
      fontSize: 56,
      align: 'center',
      canvasWidth: 400,
      canvasHeight: 200,
      textColor: '#888',
      ...params,
    });
    sign.position.copy(portalFrame.position);
    sign.rotation.copy(portalFrame.rotation);
    sign.rotation.y = sign.rotation.y + Math.PI;
    sign.position.y += 9.3;
    // move the sign forward wrt. the direction it's facing a bit
    sign.position.addScaledVector(portalFrame.getWorldDirection(new THREE.Vector3()), -2);
    viz.scene.add(sign);
  };

  const PortalDefs: Record<string, { scene: RouteId; displayName: string }> = {
    tutorial: { scene: '/tutorial', displayName: 'TUTORIAL' },
    pylons: { scene: '/pk_pylons', displayName: 'PYLONS' },
    movementv2: { scene: '/movement_v2', displayName: 'MOVEMENT V2' },
    plats: { scene: '/plats', displayName: 'PLATS' },
    cornered: { scene: '/cornered', displayName: 'CORNERED' },
    stone: { scene: '/stone', displayName: 'STONE' },
    basalt: { scene: '/basalt', displayName: 'BASALT' },
    stronghold: { scene: '/stronghold', displayName: 'STRONGHOLD' },
    pinklights: { scene: '/pinklights', displayName: 'PINKLIGHTS' },
    smoke: { scene: '/smoke', displayName: 'SMOKE' },
    bridge2: { scene: '/bridge2', displayName: 'BRIDGE' },
  };

  for (const portalFrame of portalFrames) {
    portalFrame.material = portalFrameMat;

    const portalKey = portalFrame.name.split('_')[1];
    const portal = PortalDefs[portalKey];
    if (portal) {
      addPortalFrameSign(portalFrame, { text: portal.displayName });
    }
  }

  viz.collisionWorldLoadedCbs.push(fpCtx => {
    for (const portal of portals) {
      const key = portal.name.split('_')[1];
      const def = PortalDefs[key];

      if (def) {
        fpCtx.addPlayerRegionContactCb({ type: 'convexHull', mesh: portal }, () => {
          MetricsAPI.recordPortalTravel(def.scene.slice(1));
          goto(resolve(def.scene as `/${string}`), { keepFocus: true });
        });
      } else {
        portal.visible = false;
      }
    }
  });

  const lowerPortalsSign = createSignboard({
    width: 10,
    height: 5,
    fontSize: 16,
    align: 'center',
    canvasWidth: 400,
    canvasHeight: 200,
    textColor: '#888',
    text: "These portals go to worlds that aren't part of the main game.\n\nSome of them were created early during development and may be janky or unfinished.\n\nMost have no objective, but feel free to explore them",
  });
  lowerPortalsSign.position.set(-47.2, -33.6, 19);
  lowerPortalsSign.rotation.set(0, Math.PI / 2, 0);
  viz.scene.add(lowerPortalsSign);

  const invisibleStairSlants = loadedWorld.getObjectByName('invisible_stair_slants') as THREE.Mesh;
  invisibleStairSlants.removeFromParent();
  viz.collisionWorldLoadedCbs.push(fpCtx => fpCtx.addTriMesh(invisibleStairSlants));

  const pillars = loadedWorld.getObjectByName('pillars') as THREE.Mesh;
  pillars.material = portalFrameMat;

  const totemMat = buildCustomShader(
    {
      color: 0x242424,
      map: platformDiffuse,
      uvTransform: new THREE.Matrix3().scale(0.24073, 0.24073),
      normalMap: platformNormal,
      normalScale: 0.8,
      metalness: 0.97,
      roughness: 0.3,
    },
    {
      roughnessShader: /* glsl */ `
float getCustomRoughness(vec3 pos, vec3 normal, float baseRoughness, float curTimeSeconds, SceneCtx ctx) {
  float shinyness = pow(ctx.diffuseColor.b * 645.5, 2.5) * 0.4;
  shinyness = clamp(shinyness, 0.0, 0.9);
  return 1. - shinyness;
}`,
    },
    { useTriplanarMapping: true, antialiasRoughnessShader: true }
  );

  const totem0 = loadedWorld.getObjectByName('totem') as THREE.Mesh;
  const totem1 = loadedWorld.getObjectByName('totem001') as THREE.Mesh;
  totem0.material = totemMat;
  totem1.material = totemMat;

  const pipeline = configureDefaultPostprocessingPipeline({
    viz,
    quality: vizConf.graphics.quality,
    addMiddlePasses: (composer, viz, quality) => {
      const qualityParams = {
        [GraphicsQuality.Low]: {
          baseRaymarchStepCount: 40,
          octaveCount: 3,
          renderScale: 0.25,
          fogFadeOutRangeY: 8,
          fogFadeOutPow: 1.6,
          globalScale: 1.4,
          noisePow: 1.5,
          noiseBias: 0.5,
          jbuExtent: 1,
          jbuSpatialSigma: 1.3,
          jbuDepthSigma: 0.05,
        },
        [GraphicsQuality.Medium]: { baseRaymarchStepCount: 30 },
        [GraphicsQuality.High]: { baseRaymarchStepCount: 60 },
      }[quality];
      const volumetricPass = new VolumetricPass(viz.scene, viz.camera, {
        fogMinY: -90,
        fogMaxY: -40,
        fogColorHighDensity: new THREE.Vector3(0.024, 0.024, 0.01).multiplyScalar(0.3),
        fogColorLowDensity: new THREE.Vector3(0.035, 0.03, 0.04).multiplyScalar(0.8),
        ambientLightColor: new THREE.Color(0x5d4444),
        ambientLightIntensity: 2.2,
        heightFogStartY: -90,
        heightFogEndY: -55,
        heightFogFactor: 0.54,
        maxRayLength: 1000,
        minStepLength: 0.1,
        noiseBias: 0.1,
        noisePow: 2.4,
        fogFadeOutRangeY: 38,
        fogFadeOutPow: 0.6,
        fogDensityMultiplier: 0.82,
        postDensityMultiplier: 1.7,
        noiseMovementPerSecond: new THREE.Vector2(4.1, 4.1),
        globalScale: 1,
        halfRes: quality <= GraphicsQuality.Medium,
        ...qualityParams,
      });
      composer.addPass(volumetricPass);
      viz.registerBeforeRenderCb(curTimeSeconds => volumetricPass.setCurTimeSeconds(curTimeSeconds));

      if (vizConf.graphics.quality > GraphicsQuality.Low) {
        const n8aoPass = new N8AOPostPass(
          viz.scene,
          viz.camera,
          viz.renderer.domElement.width,
          viz.renderer.domElement.height
        );
        composer.addPass(n8aoPass);
        n8aoPass.gammaCorrection = false;
        n8aoPass.enabled = vizConf.graphics.quality > GraphicsQuality.Medium;
        n8aoPass.configuration.intensity = 2;
        n8aoPass.configuration.aoRadius = 5;
        n8aoPass.configuration.halfRes = vizConf.graphics.quality <= GraphicsQuality.Medium;
        n8aoPass.setQualityMode(
          {
            [GraphicsQuality.Low]: 'Low',
            [GraphicsQuality.Medium]: 'Low',
            [GraphicsQuality.High]: 'High',
          }[vizConf.graphics.quality]
        );
      }
    },
    toneMapping: { exposure: 0.5, mode: 'agx' },
    // toneMapping: { exposure: 1, mode: 'aces' },
    autoUpdateShadowMap: true,
    emissiveBypass: true,
    emissiveBypassAmbientIntensity: vizConf.graphics.quality > GraphicsQuality.Low ? 2.8 : 3,
    emissiveBloom:
      vizConf.graphics.quality > GraphicsQuality.Low
        ? { luminanceThreshold: 1.1, luminanceSmoothing: 0 }
        : null,
  });

  // Ramp up bloom radius + intensity as the player approaches any portal.
  // At distance >= FAR the bloom is at its resting values; at distance <= NEAR it peaks.
  // Basement portals are closer together so use tighter ranges there.
  const BLOOM_NEAR_UPPER = 5;
  const BLOOM_FAR_UPPER = 30;
  const BLOOM_NEAR_BASEMENT = 5;
  const BLOOM_FAR_BASEMENT = 18;

  const BLOOM_RADIUS_REST = 0.35;
  const BLOOM_RADIUS_PEAK = 0.15;
  const BLOOM_INTENSITY_REST = 0.8;
  const BLOOM_INTENSITY_PEAK = 1.3;
  const BLOOM_LUMINANCE_THRESHOLD_REST = 1.1;
  const BLOOM_LUMINANCE_THRESHOLD_PEAK = 1.1;
  const BLOOM_LUMINANCE_SMOOTHING_REST = 0.0;
  const BLOOM_LUMINANCE_SMOOTHING_PEAK = 0.1;

  const PORTAL_LIGHT_NEAR_UPPER = 6;
  const PORTAL_LIGHT_FAR_UPPER = 20;
  const PORTAL_LIGHT_NEAR_BASEMENT = 4;
  const PORTAL_LIGHT_FAR_BASEMENT = 14;
  const PORTAL_LIGHT_INTENSITY_PEAK = 1000;

  // How much to dim ambient + directional lights when right next to a portal.
  // Each has independent min/max so they can be tuned separately.
  const AMBIENT_INTENSITY_BASE = 2.8;
  const AMBIENT_INTENSITY_NEAR = 0.6;
  const DIR_LIGHT_INTENSITY_BASE = 2.2;
  const DIR_LIGHT_INTENSITY_NEAR = 0.3;

  const _playerPos = new THREE.Vector3();
  viz.registerBeforeRenderCb(() => {
    viz.camera.getWorldPosition(_playerPos);

    // Mirror the basement point-light condition: 1 when in basement (y <= -20), 0 at surface (y >= 0).
    const inBasementT = 1 - smoothstep(-20, 0, _playerPos.y);

    // Exclude basement portals when the player is at the upper level; they sit directly
    // beneath parts of the upper platform and would otherwise trigger effects from above.
    const activeEntries = portalLightEntries.filter(e => !e.isBasement || inBasementT > 0);

    // Use full 3D distance to pick the nearest portal...
    let minDist3D = Infinity;
    let nearestEntry = activeEntries[0] ?? portalLightEntries[0];
    for (const entry of activeEntries) {
      const d = _playerPos.distanceTo(entry.pos);
      if (d < minDist3D) {
        minDist3D = d;
        nearestEntry = entry;
      }
    }

    // ...but use horizontal distance for all proximity T values so that
    // walking over a portal (vertical separation) doesn't kill the effect.
    const dx = _playerPos.x - nearestEntry.pos.x;
    const dz = _playerPos.z - nearestEntry.pos.z;
    const hDist = Math.sqrt(dx * dx + dz * dz);

    // Lerp near/far ranges between upper and basement values as the player descends.
    const bloomNear = BLOOM_NEAR_UPPER + inBasementT * (BLOOM_NEAR_BASEMENT - BLOOM_NEAR_UPPER);
    const bloomFar = BLOOM_FAR_UPPER + inBasementT * (BLOOM_FAR_BASEMENT - BLOOM_FAR_UPPER);
    const lightNear =
      PORTAL_LIGHT_NEAR_UPPER + inBasementT * (PORTAL_LIGHT_NEAR_BASEMENT - PORTAL_LIGHT_NEAR_UPPER);
    const lightFar =
      PORTAL_LIGHT_FAR_UPPER + inBasementT * (PORTAL_LIGHT_FAR_BASEMENT - PORTAL_LIGHT_FAR_UPPER);

    // Bloom proximity
    const bloomT = 1 - Math.min(1, Math.max(0, (hDist - bloomNear) / (bloomFar - bloomNear)));
    pipeline.setEmissiveBloom({
      radius: BLOOM_RADIUS_REST + bloomT * (BLOOM_RADIUS_PEAK - BLOOM_RADIUS_REST),
      intensity: BLOOM_INTENSITY_REST + bloomT * (BLOOM_INTENSITY_PEAK - BLOOM_INTENSITY_REST),
      luminanceThreshold:
        BLOOM_LUMINANCE_THRESHOLD_REST +
        bloomT * (BLOOM_LUMINANCE_THRESHOLD_PEAK - BLOOM_LUMINANCE_THRESHOLD_REST),
      luminanceSmoothing:
        BLOOM_LUMINANCE_SMOOTHING_REST +
        bloomT * (BLOOM_LUMINANCE_SMOOTHING_PEAK - BLOOM_LUMINANCE_SMOOTHING_REST),
    });

    // Portal point light: snaps to nearest portal, tints to its color
    const lightT = 1 - Math.min(1, Math.max(0, (hDist - lightNear) / (lightFar - lightNear)));
    portalPointLight.position.copy(nearestEntry.pos);
    portalPointLight.color.copy(nearestEntry.col);
    portalPointLight.intensity = lightT * PORTAL_LIGHT_INTENSITY_PEAK;

    // Dim scene lights as the portal light ramps up to keep overall brightness balanced.
    ambientLight.intensity =
      AMBIENT_INTENSITY_BASE + lightT * (AMBIENT_INTENSITY_NEAR - AMBIENT_INTENSITY_BASE);
    dirLight.intensity =
      DIR_LIGHT_INTENSITY_BASE + lightT * (DIR_LIGHT_INTENSITY_NEAR - DIR_LIGHT_INTENSITY_BASE);
  });

  const locations = {
    spawn: {
      pos: [-66.184, 2.928, -0.201] as [number, number, number],
      rot: [0, Math.PI / 2, 0] as [number, number, number],
    },
  };

  return {
    spawnLocation: 'spawn',
    gravity: 30,
    player: {
      moveSpeed: { onGround: 10, inAir: 13 },
      colliderSize: { height: 2.2, radius: 1.14 },
      jumpVelocity: 12,
      oobYThreshold: -80,
      dashConfig: {
        enable: true,
        useExternalVelocity: true,
        sfx: { play: true, name: 'dash' },
        chargeConfig: { curCharges: rwritable(Infinity) },
      },
      externalVelocityAirDampingFactor: new THREE.Vector3(0.32, 0.3, 0.32),
      externalVelocityGroundDampingFactor: new THREE.Vector3(0.9992, 0.9992, 0.9992),
    },
    debugPos: true,
    locations,
    customControlsEntries: [
      {
        key: 'f',
        action: () => viz.fpCtx?.teleportPlayer(locations.spawn.pos, locations.spawn.rot),
        label: 'Respawn',
      },
    ],
    legacyLights: false,
    sfx: {
      neededSfx: ['dash'],
    },
  };
};
