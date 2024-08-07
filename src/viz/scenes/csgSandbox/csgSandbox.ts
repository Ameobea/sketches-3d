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
  const ambientLight = new THREE.AmbientLight(0xffffff, 0.8);
  viz.scene.add(ambientLight);

  const dirLight = new THREE.DirectionalLight(0xffffff, 2);
  viz.scene.add(dirLight);

  const csg = await import('../../wasmComp/csg_sandbox').then(async engine => {
    await engine.default();
    return engine;
  });

  const sphere = new THREE.SphereGeometry(4, 16, 16);
  if (!sphere.index) {
    throw new Error('SphereGeometry missing index buffer');
  }

  const ctx = csg.csg_sandbox_init(
    new Uint32Array(sphere.index.array),
    new Float32Array(sphere.attributes.position.array)
  );
  const indices = csg.csg_sandbox_take_indices(ctx);
  const vertices = csg.csg_sandbox_take_vertices(ctx);
  const normals = csg.csg_sandbox_take_normals(ctx);
  const displacementNormals = csg.csg_sandbox_take_displacement_normals(ctx);
  csg.csg_sandbox_free(ctx);

  const geometry = new THREE.BufferGeometry();
  const needsU32Indices = indices.some(i => i > 65535);
  geometry.setIndex(
    new THREE.BufferAttribute(needsU32Indices ? new Uint32Array(indices) : new Uint16Array(indices), 1)
  );
  geometry.setAttribute('position', new THREE.BufferAttribute(new Float32Array(vertices), 3));
  geometry.setAttribute('normal', new THREE.BufferAttribute(new Float32Array(normals), 3));

  for (let vtxIx = 0; vtxIx < vertices.length / 3; vtxIx += 1) {
    const vtx = new THREE.Vector3(vertices[vtxIx * 3], vertices[vtxIx * 3 + 1], vertices[vtxIx * 3 + 2]);
    const normal = new THREE.Vector3(
      displacementNormals[vtxIx * 3],
      displacementNormals[vtxIx * 3 + 1],
      displacementNormals[vtxIx * 3 + 2]
    );
    const arrow = new THREE.ArrowHelper(normal, vtx, 1.5, 0xff0000, 0.2, 0.08);
    arrow.userData.nocollide = true;
    // viz.scene.add(arrow);
  }

  const interiorVtxPositions: Float32Array = csg.csg_sandbox_take_interior_vtx_positions();
  console.log(`interior vtx count: ${interiorVtxPositions.length / 3}`);
  for (let vtxIx = 0; vtxIx < interiorVtxPositions.length / 3; vtxIx += 1) {
    const [x, y, z] = interiorVtxPositions.slice(vtxIx * 3, vtxIx * 3 + 3);
    const box = new THREE.Mesh(
      new THREE.BoxGeometry(0.03, 0.03, 0.03),
      new THREE.MeshBasicMaterial({ color: 0xff0000 })
    );
    box.position.set(x, y, z);
    box.userData.nocollide = true;
    // viz.scene.add(box);
  }

  const debugMat = new THREE.MeshBasicMaterial({ color: 0x00ff00, wireframe: true });
  // const debugMat = new THREE.MeshPhongMaterial({ color: 0x00ff00, wireframe: false });
  // const debugMat = new THREE.MeshPhysicalMaterial({ color: 0x00ff00, transparent: true, opacity: 0.57 });
  // const debugMat = new THREE.MeshNormalMaterial({ side: THREE.FrontSide });
  const mesh = new THREE.Mesh(geometry, debugMat);
  viz.scene.add(mesh);

  // add a platform to stand on
  const platformGeo = new THREE.BoxGeometry(50, 1, 50);
  const platformMat = new THREE.MeshPhysicalMaterial({ color: 0x003300, flatShading: true });
  const platform = new THREE.Mesh(platformGeo, platformMat);
  platform.position.set(0, -3, 0);
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
      dashConfig: { enable: true },
    },
    debugPos: true,
    locations: {
      spawn: {
        pos: [1.15471613407135, 8.7756818532943726, -0.19975419342517853],
        rot: [-0.8227963267948929, -48.78199999999914, 0],
      },
    },
    legacyLights: false,
  };
};
