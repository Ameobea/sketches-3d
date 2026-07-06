import * as THREE from 'three';
import { N8AOPostPass } from 'n8ao';

import type { Viz } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import type { SceneConfig } from './index';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { fitAutoShadowFrustaFromScene } from 'src/viz/helpers/lights';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { CascadedShadowMapHelper } from 'src/viz/shadows/CascadedShadowMapHelper';

const USE_CSM = true;
const SHOW_CSM_DEBUG = true;

// Deterministic large-scale sandbox for validating cascaded shadow maps: a wide ground plane with a
// mix of far large structures and dense near-field small objects, so cascade density is visible at a
// glance. Baseline uses the Tier-0 whole-scene auto-fit (deliberately coarse at this scale — the
// motivation for CSM); flip `sun.castShadow` off + enable CSM once the manager exists.

const locations = {
  spawn: { pos: new THREE.Vector3(0, 3, 60), rot: new THREE.Vector3(-0.15, Math.PI, 0) },
  overview: { pos: new THREE.Vector3(-180, 90, 220), rot: new THREE.Vector3(-0.35, 2.5, 0) },
};

const mulberry32 = (seed: number) => () => {
  seed |= 0;
  seed = (seed + 0x6d2b79f5) | 0;
  let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
};

export const processLoadedScene = async (
  viz: Viz,
  _loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  viz.scene.background = new THREE.Color(0x9ec4e0);

  viz.scene.add(new THREE.AmbientLight(0xffffff, 0.35));
  viz.scene.add(new THREE.HemisphereLight(0xbfd8ef, 0x6a6350, 0.5));

  const groundMat = buildCustomShader({ color: new THREE.Color(0x8a8f96), metalness: 0.1, roughness: 0.95 });
  const bigMat = buildCustomShader({ color: new THREE.Color(0xb9b6ad), metalness: 0.15, roughness: 0.85 });
  const smallMat = buildCustomShader({ color: new THREE.Color(0xc07a5b), metalness: 0.2, roughness: 0.7 });

  const ground = new THREE.Mesh(new THREE.PlaneGeometry(2400, 2400), groundMat);
  ground.rotation.x = -Math.PI / 2;
  ground.receiveShadow = true;
  ground.castShadow = false;
  ground.name = 'ground';
  viz.scene.add(ground);
  const colliders: THREE.Mesh[] = [ground];

  const box = (sx: number, sy: number, sz: number, mat: THREE.Material) => {
    const m = new THREE.Mesh(new THREE.BoxGeometry(sx, sy, sz), mat);
    m.castShadow = true;
    m.receiveShadow = true;
    return m;
  };

  // Far large structures: tall towers + broad elevated platforms scattered across a wide area.
  const rnd = mulberry32(1337);
  for (let i = 0; i < 60; i += 1) {
    const angle = rnd() * Math.PI * 2;
    const dist = 120 + rnd() * 780;
    const x = Math.cos(angle) * dist;
    const z = Math.sin(angle) * dist;
    const h = 30 + rnd() * 120;
    const w = 8 + rnd() * 24;
    const tower = box(w, h, w, bigMat);
    tower.position.set(x, h / 2, z);
    viz.scene.add(tower);
    colliders.push(tower);
  }
  for (let i = 0; i < 10; i += 1) {
    const x = (rnd() - 0.5) * 1400;
    const z = (rnd() - 0.5) * 1400;
    const y = 40 + rnd() * 90;
    const plat = box(60 + rnd() * 120, 4, 60 + rnd() * 120, bigMat);
    plat.position.set(x, y, z);
    viz.scene.add(plat);
    colliders.push(plat);
  }

  // Dense near-field small objects to exercise the near cascade's crispness.
  for (let i = 0; i < 400; i += 1) {
    const x = (rnd() - 0.5) * 260;
    const z = (rnd() - 0.5) * 260;
    const s = 0.8 + rnd() * 3.5;
    const o = box(s, s + rnd() * 4, s, smallMat);
    o.position.set(x, o.geometry.parameters.height / 2, z);
    o.rotation.y = rnd() * Math.PI;
    viz.scene.add(o);
  }

  const sun = new THREE.DirectionalLight(0xfff4e0, 2.2);
  sun.position.set(500, 260, 320);
  sun.target.position.set(0, 0, 0);
  sun.castShadow = true;
  const mapSize = {
    [GraphicsQuality.Low]: 2048,
    [GraphicsQuality.Medium]: 4096,
    [GraphicsQuality.High]: 4096,
  }[vizConf.graphics.quality];
  sun.shadow.mapSize.set(mapSize, mapSize);
  sun.userData.autoShadowFrustum = true;
  viz.scene.add(sun);
  viz.scene.add(sun.target);

  // Baseline (Tier 0): fit one frustum to the whole scene. Coarse at this scale — the "before" shot.
  fitAutoShadowFrustaFromScene(viz.scene, [sun]);

  const controller = configureDefaultPostprocessingPipeline({
    viz,
    quality: vizConf.graphics.quality,
    addMiddlePasses: (composer, viz, quality) => {
      if (quality > GraphicsQuality.Low) {
        const n8aoPass = new N8AOPostPass(
          viz.scene,
          viz.camera,
          viz.renderer.domElement.width,
          viz.renderer.domElement.height
        );
        n8aoPass.gammaCorrection = false;
        n8aoPass.configuration.intensity = 3;
        n8aoPass.configuration.aoRadius = 6;
        n8aoPass.configuration.halfRes = quality <= GraphicsQuality.Medium;
        composer.addPass(n8aoPass, 3);
      }
    },
    csm: USE_CSM ? { light: sun, cascades: 3, maxDistance: 500, mapSize: 2048, debugBlit: true } : undefined,
  });

  if (controller.csm && SHOW_CSM_DEBUG) {
    const helper = new CascadedShadowMapHelper(controller.csm);
    viz.scene.add(helper);
    viz.registerAfterRenderCb(() => helper.update());
  }

  // Collide the ground + large structures so first-person walking works; the tiny near-field
  // objects stay walk-through.
  viz.collisionWorldLoadedCbs.push(fpCtx => {
    for (const mesh of colliders) {
      fpCtx.addTriMesh(mesh);
    }
  });

  return {
    locations,
    spawnLocation: 'spawn',
    debugPos: true,
    gravity: 40,
    player: {
      moveSpeed: { onGround: 24, inAir: 26 },
      colliderSize: { height: 2.2, radius: 0.8 },
      jumpVelocity: 32,
    },
  };
};
