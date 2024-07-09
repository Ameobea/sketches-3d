import type { VizState } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import * as THREE from 'three';
import { N8AOPostPass } from 'n8ao';
import type { SceneConfig } from '..';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';

const locations = {
  spawn: {
    pos: new THREE.Vector3(0, 80, 0),
    rot: new THREE.Vector3(-0.1, 1.378, 0),
  },
};

export const processLoadedScene = async (
  viz: VizState,
  loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  const ambientLight = new THREE.AmbientLight(0xffffff, 0.8);
  viz.scene.add(ambientLight);

  // TODO: use web worker
  const basaltEngine = await import('../../wasmComp/basalt').then(async engine => {
    await engine.default();
    return engine;
  });
  const basaltCtx = basaltEngine.basalt_gen();
  const vertices = basaltEngine.basalt_take_vertices(basaltCtx);
  const indices = basaltEngine.basalt_take_indices(basaltCtx);
  const normals = basaltEngine.basalt_take_normals(basaltCtx);
  basaltEngine.basalt_free(basaltCtx);

  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.BufferAttribute(new Float32Array(vertices), 3));
  geometry.setAttribute('normal', new THREE.BufferAttribute(new Float32Array(normals), 3));
  const needsU32Indices = indices.some(i => i > 65535);
  geometry.setIndex(
    new THREE.BufferAttribute(needsU32Indices ? new Uint32Array(indices) : new Uint16Array(indices), 1)
  );

  // const debugMat = new THREE.MeshBasicMaterial({ color: 0xff0000, wireframe: true });
  const basicMat = new THREE.MeshPhongMaterial({
    color: 0x6720f8,
    specular: 0x000000,
    shininess: 0,
    flatShading: true,
  });
  const mesh = new THREE.Mesh(geometry, basicMat);
  mesh.castShadow = true;
  mesh.receiveShadow = true;
  mesh.position.set(0, 0, 0);
  viz.scene.add(mesh);
  viz.collisionWorldLoadedCbs.push(fpCtx => fpCtx.addTriMesh(mesh));

  const dirLight = new THREE.DirectionalLight(0xffffff, 14.5);
  dirLight.position.set(242, 65, 220);
  dirLight.target.position.set(0, 0, 0);

  dirLight.castShadow = true;
  dirLight.shadow.needsUpdate = true;
  dirLight.shadow.autoUpdate = true;
  dirLight.shadow.mapSize.width = 2048 * 4;
  dirLight.shadow.mapSize.height = 2048 * 4;
  dirLight.shadow.radius = 4;
  dirLight.shadow.blurSamples = 16;
  viz.renderer.shadowMap.type = THREE.VSMShadowMap;
  dirLight.shadow.bias = -0.0001;

  dirLight.shadow.camera.near = 0.008;
  dirLight.shadow.camera.far = 360;
  dirLight.shadow.camera.left = -280;
  dirLight.shadow.camera.right = 180;
  dirLight.shadow.camera.top = 20;
  dirLight.shadow.camera.bottom = -120;

  const shadowCameraHelper = new THREE.CameraHelper(dirLight.shadow.camera);
  viz.scene.add(shadowCameraHelper);

  dirLight.shadow.camera.updateProjectionMatrix();
  dirLight.shadow.camera.updateMatrixWorld();

  viz.scene.add(dirLight);
  viz.scene.add(dirLight.target);

  configureDefaultPostprocessingPipeline(viz, vizConf.graphics.quality, (composer, viz, quality) => {
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
    n8aoPass.configuration.halfRes = vizConf.graphics.quality <= GraphicsQuality.Low;
    n8aoPass.setQualityMode(
      {
        [GraphicsQuality.Low]: 'Low',
        [GraphicsQuality.Medium]: 'Low',
        [GraphicsQuality.High]: 'Medium',
      }[vizConf.graphics.quality]
    );
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