import * as THREE from 'three';

import type { Viz } from 'src/viz';
import type { VizConfig } from 'src/viz/conf';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { loadNamedTextures } from 'src/viz/textureLoading';
import type { SceneConfig } from '..';

// Minimal "grooves" field: periodic horizontal channels along world Y
const GROOVES_HEIGHT_SHADER = /* glsl */ `
float getPomHeight(vec3 pos, vec3 normal, float curTimeSeconds) {
  float cell = fract(pos.y * 3.0);
  return 1.0 - smoothstep(0.08, 0.42, abs(cell - 0.5));
}
`;

// Carved square-tile relief for the inline-emissive surfaces.
const TILE_HEIGHT_SHADER = /* glsl */ `
float getPomHeight(vec3 pos, vec3 normal, float curTimeSeconds) {
  vec2 g = abs(fract(pos.xy * 1.5 + pos.zx * 0.0) - 0.5);
  float groove = smoothstep(0.46, 0.5, max(g.x, g.y));
  return groove;
}
`;

// Glowing runic-grid sigil in world space. Bright HDR blue (>1 so it blooms hard)
// on the grid lines, black elsewhere — `inlineEmissiveBypass` reads the dark base
// through (luminance coverage) and composites only the sigil. With POM active `pos`
// is the displaced hit, so the sigil tracks the carved relief.
const SIGIL_EMISSIVE_SHADER = /* glsl */ `
vec3 getCustomEmissive(vec3 pos, vec3 emissive, float curTimeSeconds, SceneCtx ctx) {
  vec2 c = fract(pos.xy * 1.5) - 0.5;
  vec2 g = abs(c);
  float grid = smoothstep(0.06, 0.0, min(g.x, g.y));
  float ring = smoothstep(0.04, 0.0, abs(length(c) - 0.32));
  float mask = clamp(grid + ring, 0.0, 1.0);
  float pulse = 0.52 + 0.48 * sin(curTimeSeconds * 1.5 + pos.y * 2.);
  pulse = pow(pulse, 1.8);
  return emissive + vec3(0.25, 1.3, 6.0) * (4.0 * pulse) * mask;
}
`;

export const processLoadedScene = async (
  viz: Viz,
  _loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  viz.camera.near = 0.1;
  viz.camera.far = 2000;
  viz.camera.updateProjectionMatrix();

  viz.scene.background = new THREE.Color(0x223044);
  viz.scene.add(new THREE.AmbientLight(0xffffff, 0.35));

  viz.renderer.shadowMap.enabled = true;
  viz.renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  const sun = new THREE.DirectionalLight(0xffffff, 2.4);
  sun.position.set(10, 14, 8);
  sun.castShadow = true;
  sun.shadow.mapSize.set(2048, 2048);
  sun.shadow.camera.near = 1;
  sun.shadow.camera.far = 80;
  sun.shadow.camera.left = -30;
  sun.shadow.camera.right = 30;
  sun.shadow.camera.top = 30;
  sun.shadow.camera.bottom = -30;
  viz.scene.add(sun);

  const loader = new THREE.ImageBitmapLoader();
  const { diffuse } = await loadNamedTextures(loader, {
    diffuse: 'https://i.ameo.link/amf.png',
  });

  // --- Existing plain bounded-silhouette POM (regression: still works with the
  //     emissive pipeline now enabled) ---
  const pomMat = buildCustomShader(
    {
      map: diffuse,
      color: 0x9b8f7e,
      roughness: 0.85,
      metalness: 0,
      uvTransform: new THREE.Matrix3().scale(0.2, 0.2),
      mapDisableDistance: null,
    },
    { pomHeightShader: GROOVES_HEIGHT_SHADER },
    {
      useTriplanarMapping: true,
      pom: { depth: 0.15, steps: 24, lodFadeStart: 400, lodFadeRange: 100, boundedSilhouette: true },
    }
  );

  // --- Inline emissive, standard POM (dark base + glowing sigils, marched once) ---
  const inlineSigilMat = buildCustomShader(
    {
      color: 0x0a0a16,
      roughness: 0.6,
      metalness: 0,
      uvTransform: new THREE.Matrix3().scale(0.2, 0.2),
      mapDisableDistance: null,
    },
    { pomHeightShader: TILE_HEIGHT_SHADER, emissiveShader: SIGIL_EMISSIVE_SHADER },
    {
      useTriplanarMapping: true,
      inlineEmissiveBypass: true,
      pom: {
        depth: 0.12,
        steps: 24,
        lodFadeStart: 400,
        lodFadeRange: 100,
        applyReliefNormal: true,
        boundedSilhouette: true,
      },
    }
  );

  // --- Inline emissive + bounded-silhouette POM combo (the harder case) ---
  const inlineBoundedMat = buildCustomShader(
    {
      color: 0x0a0a16,
      roughness: 0.6,
      metalness: 0,
      uvTransform: new THREE.Matrix3().scale(0.2, 0.2),
      mapDisableDistance: null,
    },
    { pomHeightShader: TILE_HEIGHT_SHADER, emissiveShader: SIGIL_EMISSIVE_SHADER },
    {
      useTriplanarMapping: true,
      inlineEmissiveBypass: true,
      pom: {
        depth: 0.18,
        steps: 28,
        lodFadeStart: 400,
        lodFadeRange: 100,
        boundedSilhouette: true,
        applyReliefNormal: true,
      },
    }
  );

  // --- Whole-mesh emissive bypass (parity reference for bloom/color) ---
  const bypassRefMat = buildCustomShader(
    { color: 0x3322ff, roughness: 1, ambientLightScale: 6 },
    {},
    { disableToneMapping: true }
  );

  viz.registerBeforeRenderCb(curTimeSeconds => {
    pomMat.setCurTimeSeconds(curTimeSeconds);
    inlineSigilMat.setCurTimeSeconds(curTimeSeconds);
    inlineBoundedMat.setCurTimeSeconds(curTimeSeconds);
  });

  const ground = new THREE.Mesh(
    new THREE.BoxGeometry(60, 1, 60),
    new THREE.MeshStandardMaterial({ color: 0x3a3a3a, roughness: 0.9 })
  );
  ground.position.set(0, -0.5, 0);
  ground.receiveShadow = true;
  viz.scene.add(ground);

  // Plain bounded POM, pushed back for regression comparison.
  const wall = new THREE.Mesh(new THREE.BoxGeometry(8, 6, 1.5), pomMat);
  wall.position.set(-9, 3.5, -4);
  viz.scene.add(wall);

  const plainSphere = new THREE.Mesh(new THREE.SphereGeometry(2.5, 64, 48), pomMat);
  plainSphere.position.set(-9, 4, 1);
  viz.scene.add(plainSphere);

  // Inline-emissive standard-POM wall, front and center.
  const sigilWall = new THREE.Mesh(new THREE.BoxGeometry(7, 6, 1.5), inlineSigilMat);
  sigilWall.position.set(0, 3.5, 0);
  sigilWall.castShadow = true;
  sigilWall.receiveShadow = true;
  viz.scene.add(sigilWall);

  // Inline-emissive bounded-silhouette sphere (carved silhouette + sigils).
  const sigilSphere = new THREE.Mesh(new THREE.SphereGeometry(2.6, 96, 64), inlineBoundedMat);
  sigilSphere.position.set(7, 3.5, 0);
  sigilSphere.castShadow = true;
  viz.scene.add(sigilSphere);

  // Occluder: a plain box that can be walked between camera and the sigils to
  // verify occlusion (sigils must not bleed through it).
  const occluder = new THREE.Mesh(
    new THREE.BoxGeometry(2, 4, 2),
    new THREE.MeshStandardMaterial({ color: 0x555555, roughness: 0.9 })
  );
  occluder.position.set(3.5, 2, 6);
  occluder.castShadow = true;
  occluder.receiveShadow = true;
  viz.scene.add(occluder);

  // Whole-mesh bypass parity reference.
  const bypassRef = new THREE.Mesh(new THREE.SphereGeometry(1.2, 48, 32), bypassRefMat);
  bypassRef.position.set(0, 8, -3);
  viz.scene.add(bypassRef);

  const ppController = configureDefaultPostprocessingPipeline({
    viz,
    quality: vizConf.graphics.quality,
    emissiveBypass: true,
    emissiveBloom: { intensity: 1.9, radius: 0.4, luminanceThreshold: 0 },
    pomExitBuffers: true,
  });
  ppController.addEmissiveBypassObject(bypassRef);
  ppController.rescanBypassMeshes(viz.scene);

  return {
    locations: {
      spawn: {
        pos: new THREE.Vector3(0, 4, 14),
        rot: new THREE.Vector3(0, 0, 0),
      },
    },
    spawnLocation: 'spawn',
    viewMode: {
      type: 'orbit',
      pos: new THREE.Vector3(2, 5, 16),
      target: new THREE.Vector3(0, 3.5, 0),
    },
  };
};
