import type { VizState } from 'src/viz';
import type { VizConfig } from 'src/viz/conf';
import * as THREE from 'three';
import type { SceneConfig } from '..';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { generateNormalMapFromTexture, loadTexture } from 'src/viz/textureLoading';

const locations = {
  spawn: {
    pos: new THREE.Vector3(0, 3, 0),
    rot: new THREE.Vector3(-0.1, 1.378, 0),
  },
};

export const processLoadedScene = async (
  viz: VizState,
  loadedWorld: THREE.Group,
  _vizConf: VizConfig
): Promise<SceneConfig> => {
  const ambientLight = new THREE.AmbientLight(0xffffff, 5.8);
  viz.scene.add(ambientLight);

  const loader = new THREE.ImageBitmapLoader();
  const towerPlinthPedestalTextureP = loadTexture(
    loader,
    'https://pub-80300747d44d418ca912329092f69f65.r2.dev/img-samples/000005.1476533049.png'
  );
  const [towerPlinthPedestalTextureCombinedDiffuseNormalTexture, tessellationEngine] = await Promise.all([
    towerPlinthPedestalTextureP.then(towerPlinthPedestalTexture =>
      generateNormalMapFromTexture(towerPlinthPedestalTexture, {}, true)
    ),
    import('../../wasmComp/tessellation').then(async engine => {
      await engine.default();
      return engine;
    }),
  ]);
  const pylonMaterial = buildCustomShader(
    {
      color: new THREE.Color(0x898989),
      metalness: 0.18,
      roughness: 0.82,
      map: towerPlinthPedestalTextureCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(0.08, 0.08),
      mapDisableDistance: null,
      normalScale: 5.2,
    },
    {},
    {
      usePackedDiffuseNormalGBA: true,
      useTriplanarMapping: true,
      randomizeUVOffset: true,
    }
  );
  const debugMat = new THREE.MeshBasicMaterial({ color: 0xff0000, wireframe: true });

  const toAdd: THREE.Object3D[] = [];
  const toRemove: THREE.Object3D[] = [];

  loadedWorld.traverse(obj => {
    if (!(obj instanceof THREE.Mesh)) {
      return;
    }

    const mesh = obj as THREE.Mesh;
    const nonIndexedGeometry = mesh.geometry.index ? mesh.geometry.toNonIndexed() : mesh.geometry;

    nonIndexedGeometry.computeVertexNormals();

    const verts = nonIndexedGeometry.attributes.position.array;
    if (!(verts instanceof Float32Array)) {
      throw new Error('Expected vertices to be Float32Array');
    }
    const normals = nonIndexedGeometry.attributes.normal.array;
    if (!(normals instanceof Float32Array)) {
      throw new Error('Expected normals to be Float32Array');
    }
    console.log(verts.length, normals.length);
    mesh.updateMatrixWorld();

    const targetTriangleArea = 0.1;
    const tessCtx = tessellationEngine.tessellate_mesh(verts, normals, targetTriangleArea);
    const newVerts = tessellationEngine.tessellate_mesh_ctx_get_vertices(tessCtx);
    const newNormals = tessellationEngine.tessellate_mesh_ctx_get_normals(tessCtx);
    tessellationEngine.tessellate_mesh_ctx_free(tessCtx);

    console.log(`tessellated from ${verts.length / 3} to ${newVerts.length / 3} verts`);

    const newGeometry = new THREE.BufferGeometry();
    newGeometry.setAttribute('position', new THREE.BufferAttribute(newVerts, 3));
    newGeometry.setAttribute('normal', new THREE.BufferAttribute(newNormals, 3));
    newGeometry.computeVertexNormals();

    const newMesh = new THREE.Mesh(newGeometry, pylonMaterial);
    newMesh.position.copy(mesh.position);
    newMesh.rotation.copy(mesh.rotation);
    newMesh.scale.copy(mesh.scale);
    newMesh.updateMatrix();

    toAdd.push(newMesh);
    toRemove.push(mesh);
  });

  for (const obj of toRemove) {
    loadedWorld.remove(obj);
  }
  for (const obj of toAdd) {
    loadedWorld.add(obj);
  }

  const dirLight = new THREE.DirectionalLight(0xffffff, 1.5);
  dirLight.position.set(0, 1, 0);

  viz.scene.add(dirLight);

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
      },
    },
    debugPos: true,
    locations,
    legacyLights: false,
  };
};
