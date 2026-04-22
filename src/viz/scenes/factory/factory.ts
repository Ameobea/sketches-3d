import * as THREE from 'three';
import { N8AOPostPass } from 'n8ao';

import type { Viz } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import type { SceneConfig } from '..';
import { ParkourManager } from 'src/viz/parkour/ParkourManager.svelte';
import { Score, type ScoreThresholds } from 'src/viz/parkour/timeDisplayTypes';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { rwritable } from 'src/viz/util/TransparentWritable';
import { buildPylonsCheckpointMaterial } from 'src/viz/parkour/regions/pylons/materials';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import {
  SkyStack,
  HorizonMode,
  starsLayer,
  cloudsLayer,
  buildingsLayer,
  groundLayer,
  gradientBackground,
} from 'src/viz/SkyStack';

const collectMeshes = (obj: THREE.Object3D): THREE.Mesh[] => {
  const out: THREE.Mesh[] = [];
  obj.traverse(child => {
    if (child instanceof THREE.Mesh) {
      out.push(child);
    }
  });
  return out;
};

export const processLoadedScene = (viz: Viz, loadedWorld: THREE.Group, vizConf: VizConfig): SceneConfig => {
  const playerHeight = 5;
  const playerRadius = 1.5;
  const playerMesh = new THREE.Mesh(
    new THREE.CapsuleGeometry(playerRadius, playerHeight, 16, 16),
    buildCustomShader(
      {
        color: new THREE.Color(0x8d3d9f),
        metalness: 0.18,
        roughness: 0.82,
      },
      {},
      { noOcclusion: true }
    )
  );
  playerMesh.castShadow = false;
  playerMesh.receiveShadow = true;

  const dashToken = new THREE.Group();
  dashToken.name = 'dash_token';
  dashToken.visible = false;
  const dashTokenCore = new THREE.Mesh(
    new THREE.SphereGeometry(0.45, 12, 12),
    new THREE.MeshStandardMaterial()
  );
  dashTokenCore.name = 'core';
  const dashTokenRing = new THREE.Mesh(
    new THREE.TorusGeometry(0.72, 0.1, 8, 24),
    new THREE.MeshStandardMaterial()
  );
  dashTokenRing.name = 'ring';
  dashToken.add(dashTokenCore, dashTokenRing);
  loadedWorld.add(dashToken);

  const scoreThresholds: ScoreThresholds = {
    [Score.SPlus]: Infinity,
    [Score.S]: Infinity,
    [Score.A]: Infinity,
    [Score.B]: Infinity,
  };
  const pkManager = new ParkourManager(
    viz,
    loadedWorld,
    vizConf,
    {
      spawn: {
        pos: new THREE.Vector3(-48, 3, -14),
        rot: new THREE.Vector3(-0.35, -Math.PI / 2, 0),
      },
    },
    scoreThresholds,
    undefined,
    'factory',
    true,
    {
      gravity: 220,
      gravityShaping: {
        riseMultiplier: 1.0,
        apexMultiplier: 0.6,
        fallMultiplier: 1.2,
        apexThreshold: 4.0,
        kneeWidth: 0.1,
      },
      player: {
        playerColliderShape: 'capsule',
        mesh: playerMesh,
        colliderSize: { height: playerHeight, radius: playerRadius },
        playerShadow: { radius: playerRadius, intensity: 0.85 },
        moveSpeed: { onGround: 18.9, inAir: 21.6 },
        jumpVelocity: 76,
        terminalVelocity: 180,
        maxPenetrationDepth: 0.008,
        dashConfig: {
          chargeConfig: { curCharges: rwritable(0) },
          dashMagnitude: 16,
          useExternalVelocity: true,
          minDashDelaySeconds: 0.3,
        },
        coyoteTimeSeconds: 0.135,
        externalVelocityGroundDampingFactor: new THREE.Vector3(0.99999995, 0.99999995, 0.99999995),
        maxSlopeRadians: 1.4,
        oobYThreshold: -200,
        slopeSlide: {
          minAngle: 1.1,
          maxSpeed: 80,
        },
      },
      viewMode: {
        type: 'thirdPerson',
        distance: 15,
        cameraFOV: 75,
        zoomEnabled: true,
        maxZoomDistance: 50,
      },
    }
  );

  // SDF-morph demo ground, tiled to infinity. One hashed orbit point per cell, smin'd
  // across a 3x3 neighborhood so blobs merge seamlessly across cell boundaries. Paint
  // output is pumped into HDR and rendered via the SkyStack emissive attachment so
  // colors skip AgX tone mapping and drive the bloom pass for an aggressive glow.
  const groundPaintShader = `
    uniform vec3 uGroundBgColor;
    uniform vec3 uBlobColorOuter;
    uniform vec3 uBlobColorInner;
    uniform float uTileSize;
    uniform float uEmissiveBoost;

    float sdCircle(vec2 p, vec2 c, float r) {
      return length(p - c) - r;
    }

    // Polynomial smooth-min from Inigo Quilez — bounded width \`k\`, C1 continuous.
    float smin(float a, float b, float k) {
      float h = max(k - abs(a - b), 0.0) / k;
      return min(a, b) - h * h * k * 0.25;
    }

    // Deterministic point inside cell \`id\`, orbiting a hashed base offset. Stays within
    // the cell bounds so 3x3 neighbor sampling is sufficient for smin continuity.
    vec2 cellPoint(vec2 id, float T) {
      vec2 h = vec2(hash(id + 3.7), hash(id + 19.1));
      float phase = h.x * 6.2831853;
      vec2 jitter = (h - 0.5) * (T * 0.25);
      vec2 orbit = vec2(sin(uTime * 0.027 + phase),
                        cos(uTime * 0.021 + phase)) * (T * 0.22);
      return id * T + jitter + orbit;
    }

    // Radius grows with the screen-space derivative to reduce sub-pixel
    float cellRadius(vec2 id, float T, float aaW) {
      float base = T * (0.12 + 0.08 * hash(id + 7.3));
      return min(max(base, aaW * 0.85), T * 0.35);
    }

    float cellTwinkle(vec2 id) {
      float phase = hash(id + 31.7) * 6.2831853;
      float rate = 0.5 + 0.3 * hash(id + 43.1);
      return 0.85 + 0.15 * sin(uTime * rate + phase);
    }

    vec4 paintGround_$ID(vec2 uv, vec2 uvDeriv, vec3 dir, float invDist) {
      float T = uTileSize;
      vec2 cellId = floor(uv / T + 0.5);
      float aaW = max(uvDeriv.x, uvDeriv.y);

      float d = 1e9;
      for (int j = -1; j <= 1; j++) {
        for (int i = -1; i <= 1; i++) {
          vec2 nId = cellId + vec2(float(i), float(j));
          d = smin(d, sdCircle(uv, cellPoint(nId, T), cellRadius(nId, T, aaW)), T * 0.2);
        }
      }

      // Derivative-aware edge AA, from: "The Best Darn Grid Shader (Yet)",
      // Ben Golus, https://bgolus.medium.com/the-best-darn-grid-shader-yet-727f9278b9d8
      float edge = 1.0 - smoothstep(-aaW, aaW, d);

      // Amplitude fade past Nyquist, also from Ben Golus's article
      //
      // As derivatives approach the tile size, tiles pack sub-pixel and the edge term
      // shimmers between 0 and 1 depending on exactly where the fragment lands. Lerp
      // toward the per-tile average coverage (roughly π·r²/T² ≈ 0.1 for our radius
      // distribution) so sub-pixel regions read as a uniform dim glow instead of aliasing.
      float avgCoverage = 0.1;
      float lodT = clamp(aaW / T * 2. - 1., 0., 1.);
      edge = mix(edge, avgCoverage, lodT);

      float innerT = smoothstep(0., -T * 0.1, d);
      // more emissive boost close to the horizon where the circles are smaller and less
      // likely to trigger bloom on their own, and less boost up close where they can bloom
      // aggressively without washing out the scene.
      float boost = mix(0.5, uEmissiveBoost, smoothstep(0.14, 0., -dir.y));
      vec3 blobCol = mix(uBlobColorOuter, uBlobColorInner, innerT) * boost;

      // Twinkle strength ramps 0 → 1 as we look from steeply-down toward the horizon,
      // so close blobs sit still (no distracting pulse) while distant ones shimmer like
      // city lights from altitude.
      float twinkleMix = 1. - smoothstep(0., 0.5, -dir.y);
      blobCol *= mix(1., cellTwinkle(cellId), twinkleMix);

      return vec4(mix(uGroundBgColor, blobCol, edge), 1.);
    }
  `;

  // Sky sub-pipeline — owns the emissive RT; other bypass meshes composite on
  // top of its output. 180deg linear-gradient(#0d1522 → #0f1f2f → #454b59 →
  // #714f4d); inverted from CSS because our convention is 0=horizon, 1=zenith.
  const skyStack = new SkyStack(
    viz,
    {
      horizonOffset: -0.025,
      horizonBlend: 0.03,
      layers: [
        // Higher cloud bank in front of everything — covers silhouettes and
        // dims windows/stars behind it.
        cloudsLayer({
          id: 'cloudsFront',
          zIndex: 40,
          color: 0x2a2030,
          highColor: 0x554050,
          intensity: 1,
          center: 0.048,
          width: 0.08,
          sharpness: 0.12,
          scale: [0.9, 5, 0.9],
          speed: [0.015, 0, -0.01],
          octaves: 1,
          bias: 9.95,
          pow: 1,
        }),
        buildingsLayer({
          id: 'buildings',
          zIndex: 30,
          color: 0xff7a3a,
          colorAlt: 0xffd07a,
          intensity: 0.8,
          buildingCount: 300,
          buildingPresence: 0.75,
          buildingGap: 0.2,
          buildingMinHeight: 0.015,
          buildingMaxHeight: 0.08,
          floorsMin: 4,
          floorsMax: 18,
          windowsMin: 2,
          windowsMax: 5,
          maxFloorStride: 2,
          maxWindowStride: 1,
          litFractionMin: 0.25,
          litFractionMax: 0.75,
          windowWidth: 0.45,
          windowHeight: 0.5,
          twinkleSpeed: 5.0,
          twinkleDepth: 0.38,
          // groundElev sits 0.01 below the shared horizon offset so the
          // cityscape tucks just under the horizon line.
          groundElev: -0.01,
          // Approximates the former gradient-following silhouette: horizon
          // gradient color (0x714f4d) darkened ~15%.
          silhouetteColor: 0x110c0b,
        }),
        // Low wispy haze band behind the cityscape — sits above the gradient
        // but gets occluded by silhouettes + windows.
        cloudsLayer({
          id: 'cloudsBack',
          zIndex: 20,
          color: 0x1a1c28,
          highColor: 0x3a3550,
          intensity: 0.55,
          center: 0.055,
          width: 0.08,
          sharpness: 0.18,
          scale: [1.2, 16, 1.2],
          speed: [0.01, 0, 0.008],
          octaves: 4,
          bias: 0.05,
          pow: 1.2,
        }),
        starsLayer({
          id: 'stars',
          zIndex: 10,
          color: 0xe6ecff,
          intensity: 0.35,
          density: 180,
          threshold: 0.045,
          size: 0.04,
          twinkleSpeed: 9.0,
          twinkleDepth: 0.3,
          minElev: 0.04,
        }),
        groundLayer({
          id: 'ground',
          zIndex: 5,
          height: 120,
          // Retreat the paint from the horizon band — SkyStack's buildings layer
          // takes over the distant-light duty here, and pushing the fade down kills
          // the worst of the sub-pixel aliasing from SDF blobs shrinking past Nyquist.
          horizonFadeStart: 0.03,
          horizonFadeEnd: 0.06,
          atmosphericTint: {
            // Dark warm red, sitting in the same hue family as the sky's lowest stop
            // (0x714f4d). Blobs reddening and dimming as they recede mimics the
            // sky-bleed-through effect even when the ground doesn't overlap the sky.
            range: 0.57,
            strength: 0.75,
            color: 0x160303,
          },
          paintShader: groundPaintShader,
          uniforms: {
            uGroundBgColor: { value: new THREE.Color(0x0a0508) },
            uBlobColorOuter: { value: new THREE.Color(0x1c0508) },
            uBlobColorInner: { value: new THREE.Color(0xff7a4a) },
            uTileSize: { value: 80.0 },
            // HDR multiplier on the blob color — drives the bloom pass and skips
            // AgX since the paint is routed through the emissive attachment.
            uEmissiveBoost: { value: 3.5 },
          },
        }),
      ],
      background: gradientBackground({
        stops: [
          { position: 0.0, color: 0x714f4d },
          { position: 0.354, color: 0x454b59 },
          { position: 0.698, color: 0x0f1f2f },
          { position: 1.0, color: 0x0d1522 },
        ],
        horizonMode: HorizonMode.SolidBelow,
        belowColor: 0x060301,
        lutResolution: {
          [GraphicsQuality.Low]: 32,
          [GraphicsQuality.Medium]: 64,
          [GraphicsQuality.High]: 128,
        }[vizConf.graphics.quality],
      }),
    },
    viz.renderer.domElement.width,
    viz.renderer.domElement.height
  );
  viz.registerBeforeRenderCb(curTimeSeconds => skyStack.setTime(curTimeSeconds));

  configureDefaultPostprocessingPipeline({
    viz,
    quality: vizConf.graphics.quality,
    toneMapping: { mode: 'agx', exposure: 1.2 },
    autoUpdateShadowMap: true,
    emissiveBypass: true,
    skyBypassTonemap: false,
    skyStack,
    emissiveBloom:
      vizConf.graphics.quality > GraphicsQuality.Low
        ? { intensity: 4.0, levels: 4, luminanceThreshold: 0.02, radius: 0.3, luminanceSoftKnee: 0.02 }
        : null,
    fogShader: `vec4 getFogEffect(vec3 worldPos, vec3 cameraPos, vec3 playerPos, float depth, float curTimeSeconds) {
          // Sky pixels sit at the far plane; skip fogging so the gradient sky is untouched.
          if (depth >= 0.9999) {
            return vec4(0.0);
          }
          float distToPlayer = distance(worldPos.xz, playerPos.xz) + 0.01 * abs(worldPos.y - playerPos.y);
          float fogFactor = smoothstep(140., 310., distToPlayer);
          return vec4(vec3(0.0002, 0.0002, 0.0002), fogFactor);
        }`,
    addMiddlePasses: (composer, viz, quality) => {
      let n8aoPass: typeof N8AOPostPass | null = null;
      if (quality > GraphicsQuality.Low) {
        n8aoPass = new N8AOPostPass(
          viz.scene,
          viz.camera,
          viz.renderer.domElement.width,
          viz.renderer.domElement.height
        );
        n8aoPass.gammaCorrection = false;
        n8aoPass.configuration.intensity = 4;
        n8aoPass.configuration.aoRadius = 2.5;
        n8aoPass.configuration.halfRes = quality <= GraphicsQuality.Medium;
        n8aoPass.setQualityMode(
          {
            [GraphicsQuality.Low]: 'Performance',
            [GraphicsQuality.Medium]: 'Low',
            [GraphicsQuality.High]: 'Medium',
          }[quality]
        );
        composer.addPass(n8aoPass, 3);
      }
    },
  });

  const handle = viz.levelLoadHandle!;

  handle.setMaterialFactories({
    checkpoint: viz => {
      const mat = buildPylonsCheckpointMaterial(viz);
      return { material: mat, onAssigned: mesh => mat.setMesh(mesh) };
    },
  });

  handle.parkourObjects.then(parkourObjs => {
    const checkpointMeshes = parkourObjs.flatMap(obj => collectMeshes(obj.object));
    pkManager.setMaterials(
      {
        dashToken: {
          core: new THREE.MeshStandardMaterial({ color: 0x9effe1, emissive: 0x1a322e, roughness: 0.4 }),
          ring: new THREE.MeshStandardMaterial({ color: 0xffd464, emissive: 0x36290a, roughness: 0.3 }),
        },
      },
      { checkpointMeshes }
    );
  });

  return pkManager.buildSceneConfig();
};
