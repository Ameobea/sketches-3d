import * as THREE from 'three';

import type { VizState } from 'src/viz';
import type { VizConfig } from 'src/viz/conf';
import type { SceneConfig } from '..';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';

export const processLoadedScene = async (
  viz: VizState,
  _loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  const ambientLight = new THREE.AmbientLight(0xffffff, 1.8);
  viz.scene.add(ambientLight);

  const csg = await import('../../wasmComp/csg_sandbox').then(async engine => {
    await engine.default();
    return engine;
  });

  const ctx = csg.csg_sandbox_init();
  const indices = csg.csg_sandbox_take_indices(ctx);
  const vertices = csg.csg_sandbox_take_vertices(ctx);
  const normals = csg.csg_sandbox_take_normals(ctx);
  csg.csg_sandbox_free(ctx);

  const geometry = new THREE.BufferGeometry();
  const needsU32Indices = indices.some(i => i > 65535);
  geometry.setIndex(
    new THREE.BufferAttribute(needsU32Indices ? new Uint32Array(indices) : new Uint16Array(indices), 1)
  );
  geometry.setAttribute('position', new THREE.BufferAttribute(new Float32Array(vertices), 3));
  geometry.setAttribute('normal', new THREE.BufferAttribute(new Float32Array(normals), 3));

  const debugMat = new THREE.MeshBasicMaterial({ color: 0x00ff00, wireframe: true });
  // const debugMat = new THREE.MeshPhysicalMaterial({ color: 0x00ff00, flatShading: true });
  const mesh = new THREE.Mesh(geometry, debugMat);
  viz.scene.add(mesh);

  // add a platform to stand on
  const platformGeo = new THREE.BoxGeometry(50, 1, 50);
  const platformMat = new THREE.MeshPhysicalMaterial({ color: 0x003300, flatShading: true });
  const platform = new THREE.Mesh(platformGeo, platformMat);
  platform.position.set(0, -5, 0);
  viz.scene.add(platform);

  viz.collisionWorldLoadedCbs.push(fpCtx => {
    fpCtx.addTriMesh(mesh);
    fpCtx.addTriMesh(platform);
  });

  // configureDefaultPostprocessingPipeline(viz, vizConf.graphics.quality);

  return {
    spawnLocation: 'spawn',
    gravity: 30,
    player: {
      moveSpeed: { onGround: 10, inAir: 13 },
      colliderCapsuleSize: { height: 2.2, radius: 0.8 },
      jumpVelocity: 12,
      oobYThreshold: -10,
      dashConfig: { enable: false },
    },
    debugPos: true,
    locations: { spawn: { pos: new THREE.Vector3(0, 10, 0), rot: new THREE.Vector3(-0.1, 1.378, 0) } },
    legacyLights: false,
  };
};
