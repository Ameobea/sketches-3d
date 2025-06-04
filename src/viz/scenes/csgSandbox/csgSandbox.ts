import * as THREE from 'three';

import type { Viz } from 'src/viz';
import type { VizConfig } from 'src/viz/conf';
import type { SceneConfig } from '..';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { buildGrayFossilRockMaterial } from 'src/viz/materials/GrayFossilRock/GrayFossilRockMaterial';
import { initManifoldWasm } from 'src/viz/wasmComp/manifold';

export const processLoadedScene = async (
  viz: Viz,
  _loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  const manifoldInitPromise = initManifoldWasm();
  const loader = new THREE.ImageBitmapLoader();
  const debugMatPromise = buildGrayFossilRockMaterial(
    loader,
    { uvTransform: new THREE.Matrix3().scale(0.2, 0.2), color: 0xcccccc },
    {},
    { useGeneratedUVs: false, useTriplanarMapping: true, tileBreaking: undefined }
  );

  const ambientLight = new THREE.AmbientLight(0xffffff, 1.2);
  viz.scene.add(ambientLight);

  const dirLight = new THREE.DirectionalLight(0xffffff, 2.4);
  dirLight.position.set(-20, 50, 0);
  viz.scene.add(dirLight);

  dirLight.castShadow = true;
  dirLight.shadow.mapSize.width = 2048 * 2;
  dirLight.shadow.mapSize.height = 2048 * 2;
  dirLight.shadow.radius = 4;
  dirLight.shadow.blurSamples = 16;
  viz.renderer.shadowMap.type = THREE.VSMShadowMap;
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

  const [csg] = await Promise.all([
    import('../../wasmComp/csg_sandbox').then(async engine => {
      await engine.default();
      return engine;
    }),
    manifoldInitPromise,
  ]);

  const mesh0 = new THREE.SphereGeometry(9, 7, 7);
  // const mesh0 = new THREE.TorusKnotGeometry(5, 1.5, 16, 8);
  // const mesh0 = new THREE.BoxGeometry(5, 5, 5);
  if (!mesh0.index) {
    const indices = new Uint16Array(mesh0.attributes.position.count);
    for (let i = 0; i < indices.length; i += 1) {
      indices[i] = i;
    }
    mesh0.setIndex(new THREE.BufferAttribute(indices, 1));
  }

  const mesh1 = new THREE.TorusKnotGeometry(8, 2.5, 63, 28);
  // const mesh1 = new THREE.BoxGeometry(12, 12, 12);

  const mesh0Ptr = csg.create_mesh(
    new Uint32Array(mesh0.index!.array),
    new Float32Array(mesh0.attributes.position.array)
  );
  const mesh1Ptr = csg.create_mesh(
    new Uint32Array(mesh1.index!.array),
    new Float32Array(mesh1.attributes.position.array.map(v => v + 0.002))
  );

  const ctx = csg.csg_sandbox_init(mesh0Ptr, mesh1Ptr);
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

  // for (let vtxIx = 0; vtxIx < vertices.length / 3; vtxIx += 1) {
  //   const vtx = new THREE.Vector3(vertices[vtxIx * 3], vertices[vtxIx * 3 + 1], vertices[vtxIx * 3 + 2]);
  //   const normal = new THREE.Vector3(normals[vtxIx * 3], normals[vtxIx * 3 + 1], normals[vtxIx * 3 + 2]);

  //   if (normal.length() < 0.01) {
  //     // add a marker to indicate this
  //     const box = new THREE.Mesh(
  //       new THREE.BoxGeometry(0.1, 0.1, 0.1),
  //       new THREE.MeshBasicMaterial({ color: 0xff0000 })
  //     );
  //     box.position.set(vtx.x, vtx.y, vtx.z);
  //     box.userData.nocollide = true;
  //     viz.scene.add(box);
  //     continue;
  //   }

  //   const arrow = new THREE.ArrowHelper(normal, vtx, 1.5, 0xff0000, 0.2, 0.08);
  //   arrow.userData.nocollide = true;
  //   viz.scene.add(arrow);
  // }

  let wireframeActive = false;
  const wireframeMat = new THREE.MeshBasicMaterial({ color: 0x00ff00, wireframe: true });
  const debugMat = await debugMatPromise;
  // const debugMat = new THREE.MeshNormalMaterial();
  const mesh = new THREE.Mesh(geometry, debugMat as THREE.Material);
  mesh.castShadow = true;
  mesh.receiveShadow = true;
  viz.scene.add(mesh);

  // add a platform to stand on
  const platformGeo = new THREE.BoxGeometry(50, 1, 50);
  const platformMat = new THREE.MeshPhysicalMaterial({ color: 0x003300, flatShading: true });
  const platform = new THREE.Mesh(platformGeo, platformMat);
  platform.position.set(0, -7, 0);
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
      colliderSize: { height: 2.2, radius: 0.8 },
      jumpVelocity: 12,
      oobYThreshold: -10,
      dashConfig: { enable: true, useExternalVelocity: true },
      externalVelocityAirDampingFactor: new THREE.Vector3(0.32, 0.3, 0.32),
      externalVelocityGroundDampingFactor: new THREE.Vector3(0.9992, 0.9992, 0.9992),
    },
    debugPos: true,
    locations: {
      spawn: {
        pos: [1.15471613407135, 16.7756818532943726, -0.19975419342517853],
        rot: [-0.8227963267948929, -48.78199999999914, 0],
      },
    },
    customControlsEntries: [
      {
        key: 'v',
        label: 'Toggle Wireframe',
        action: () => {
          wireframeActive = !wireframeActive;
          mesh.material = wireframeActive ? wireframeMat : debugMat;
        },
      },
    ],
    legacyLights: false,
  };
};
