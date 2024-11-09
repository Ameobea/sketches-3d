import type { VizState } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import * as THREE from 'three';
import { N8AOPostPass } from 'n8ao';
import type { SceneConfig } from '..';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { generateNormalMapFromTexture, loadTexture } from 'src/viz/textureLoading';
import './graphvizDebug';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';

const locations = {
  spawn: {
    pos: new THREE.Vector3(0, 3, 0),
    rot: new THREE.Vector3(-0.1, 1.378, 0),
  },
};

export const processLoadedScene = async (
  viz: VizState,
  loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  const ambientLight = new THREE.AmbientLight(0xffffff, 1.8);
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
    import('../../wasmComp/tessellation_sandbox').then(async engine => {
      await engine.default();
      return engine;
    }),
  ]);
  const pylonMaterial = buildCustomShader(
    {
      color: new THREE.Color(0xbbbbbb),
      metalness: 0.18,
      roughness: 0.92,
      map: towerPlinthPedestalTextureCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(0.08, 0.08),
      mapDisableDistance: null,
      normalScale: 5.2,
      useDisplacementNormals: true,
    },
    {
      // displacementShader: `
      //   float getDisplacement(vec3 pos, vec3 normal, float curTimeSeconds) {
      //     // return 1.8;
      //     float displacement = abs(0.2 * noise(pos.xz * 4.) + 0.5 * noise(pos.xz * 2.) + 1.5 * noise(pos.xz * 1.));
      //     return displacement;
      //     // displacement = 0.0;
      //     displacement = displacement + sin(curTimeSeconds*2. + pos.x * 2.5) * sin((curTimeSeconds+1.)*2. + pos.y * 2.5) * sin((curTimeSeconds+5.)*2. + pos.z * 2.5) * 0.2;
      //     return abs(displacement);
      //   }
      // `,
      // includeNoiseShadersVertex: true,
    },
    {
      usePackedDiffuseNormalGBA: true,
      useTriplanarMapping: true,
      randomizeUVOffset: true,
    }
  );
  viz.registerBeforeRenderCb(curTimeSeconds => pylonMaterial.setCurTimeSeconds(curTimeSeconds));
  const debugMat = new THREE.MeshBasicMaterial({ color: 0xff0000, wireframe: true });
  const normalMat = new THREE.MeshNormalMaterial({
    // side: THREE.DoubleSide,
  });

  const toAdd: THREE.Object3D[] = [];
  const toRemove: THREE.Object3D[] = [];

  loadedWorld.traverse(obj => {
    if (!(obj instanceof THREE.Mesh)) {
      return;
    }

    const mesh = obj as THREE.Mesh;
    mesh.material = normalMat;
    if (mesh.name !== 'repro2') {
      // toRemove.push(mesh);
      // return;
    }
    if (
      mesh.name === 'another' ||
      mesh.name === 'repro' ||
      mesh.name === 'repro2' ||
      mesh.name === 'woah' ||
      mesh.name === 'test' ||
      mesh.name === 'Plane001'
    ) {
      toRemove.push(mesh);
      return;
    }
    console.log(mesh.name);
    if (!mesh.geometry.index) {
      throw new Error('Expected geometry to have index');
    }
    const geom = mesh.geometry;

    const verts = geom.attributes.position.array;
    if (!(verts instanceof Float32Array)) {
      throw new Error('Expected vertices to be Float32Array');
    }
    const indices = geom.index!.array;
    if (!(indices instanceof Uint32Array) && !(indices instanceof Uint16Array)) {
      throw new Error('Expected indices to be Uint32Array or Uint16Array');
    }

    const targetEdgeLength = 0.4;
    const sharpEdgeThresholdRads = 1.3;
    const tessCtx = tessellationEngine.tessellate_mesh(
      verts,
      new Uint32Array(indices),
      targetEdgeLength,
      sharpEdgeThresholdRads,
      1,
      0
    );
    const newVerts = tessellationEngine.tessellate_mesh_ctx_get_vertices(tessCtx);
    const newDisplacementNormals = tessellationEngine.tessellate_mesh_ctx_get_displacement_normals(tessCtx);
    const newShadingNormals = tessellationEngine.tessellate_mesh_ctx_get_shading_normals(tessCtx);
    const newIndices = tessellationEngine.tessellate_mesh_ctx_get_indices(tessCtx);
    tessellationEngine.tessellate_mesh_ctx_free(tessCtx);

    console.log(`tessellated from ${verts.length / 3} to ${newVerts.length / 3} verts`);

    const newGeometry = new THREE.BufferGeometry();
    newGeometry.setAttribute('position', new THREE.BufferAttribute(newVerts, 3));
    newGeometry.setAttribute('normal', new THREE.BufferAttribute(newShadingNormals, 3));
    newGeometry.setAttribute('displacementNormal', new THREE.BufferAttribute(newDisplacementNormals, 3));
    newGeometry.setIndex(new THREE.BufferAttribute(newIndices, 1));
    // if (mesh.name === 'woah') {
    //   console.log(newDisplacementNormals);
    // }

    // if (mesh.name === 'woah') {
    //   for (let vtxIx = 0; vtxIx < newVerts.length / 3; vtxIx++) {
    //     const vtx = new THREE.Vector3(newVerts[vtxIx * 3], newVerts[vtxIx * 3 + 1], newVerts[vtxIx * 3 + 2]);
    //     // if (
    //     //   Math.abs(newDisplacementNormals[vtxIx * 3] - -0.18689574301242828) < 0.001 ||
    //     //   Math.abs(newDisplacementNormals[vtxIx * 3] - -0.04491600766777992) < 0.001
    //     // ) {
    //     //   continue;
    //     // }
    //     const normal = new THREE.Vector3(
    //       newDisplacementNormals[vtxIx * 3],
    //       newDisplacementNormals[vtxIx * 3 + 1],
    //       newDisplacementNormals[vtxIx * 3 + 2]
    //     );
    //     const arrow = new THREE.ArrowHelper(normal, vtx, 1.5, 0xff0000, 0.2, 0.08);
    //     arrow.userData.nocollide = true;
    //     loadedWorld.add(arrow);
    //   }
    // }

    const newMesh = new THREE.Mesh(newGeometry, mesh.name === 'woah' ? normalMat : pylonMaterial);
    // newMesh.visible = mesh.name === 'repro2';
    newMesh.castShadow = true;
    newMesh.receiveShadow = true;
    newMesh.userData.nocollide = true;
    // add the lower-poly mesh to the collision world
    viz.collisionWorldLoadedCbs.push(fpCtx => void fpCtx.addTriMesh(mesh));
    // setInterval(() => {
    //   if (newMesh.material === pylonMaterial) {
    //     (newMesh as any).material = debugMat;
    //   } else {
    //     newMesh.material = pylonMaterial;
    //   }
    // }, 5500);
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

  const dirLight = new THREE.DirectionalLight(0xffffff, 4.5);
  dirLight.position.set(42, 9, 0);
  dirLight.target.position.set(0, 0, 0);

  dirLight.castShadow = true;
  dirLight.shadow.needsUpdate = true;
  dirLight.shadow.autoUpdate = true;
  dirLight.shadow.mapSize.width = 2048 * 1;
  dirLight.shadow.mapSize.height = 2048 * 1;
  dirLight.shadow.bias = -0.000001;

  dirLight.shadow.camera.near = 8;
  dirLight.shadow.camera.far = 80;
  dirLight.shadow.camera.left = -80;
  dirLight.shadow.camera.right = 80;
  dirLight.shadow.camera.top = 20;
  dirLight.shadow.camera.bottom = -20;

  dirLight.shadow.camera.updateProjectionMatrix();
  dirLight.shadow.camera.updateMatrixWorld();

  // const shadowCameraHelper = new THREE.CameraHelper(dirLight.shadow.camera);
  // viz.scene.add(shadowCameraHelper);

  viz.scene.add(dirLight);
  viz.scene.add(dirLight.target);

  configureDefaultPostprocessingPipeline(viz, vizConf.graphics.quality, (composer, viz, quality) => {
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
      // n8aoPass.configuration.halfRes = vizConf.graphics.quality <= GraphicsQuality.Low;
      n8aoPass.setQualityMode(
        {
          [GraphicsQuality.Low]: 'Performance',
          [GraphicsQuality.Medium]: 'Low',
          [GraphicsQuality.High]: 'Medium',
        }[vizConf.graphics.quality]
      );
    }
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
      },
    },
    debugPos: true,
    locations,
    legacyLights: false,
  };
};
