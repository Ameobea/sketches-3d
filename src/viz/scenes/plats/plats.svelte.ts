import * as THREE from 'three';

import type { Viz } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import type { SceneConfig } from '..';
import { ParkourManager } from 'src/viz/parkour/ParkourManager.svelte';
import { buildPylonsMaterials } from 'src/viz/parkour/regions/pylons/materials';
import { Score, type ScoreThresholds } from 'src/viz/parkour/timeDisplayTypes';
import { initPylonsPostprocessing } from '../pkPylons/postprocessing';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import type { Behavior } from 'src/viz/sceneRuntime/types';

const locations = {
  spawn: {
    pos: new THREE.Vector3(6, 1.56807, 5.98513),
    rot: new THREE.Vector3(0, Math.PI, 0),
  },
  1: {
    pos: new THREE.Vector3(-14.00052261352539, 1.5257837235927583, 88.56037139892578),
    rot: new THREE.Vector3(-0.15251998771375788, 3.007359865058785, 1.7550977803051575e-17),
  },
  2: {
    pos: new THREE.Vector3(-41.060359954833984, 1.527539199590683, 283.21392822265625),
    rot: new THREE.Vector3(-0.3186799999999974, 2.5848326535898063, -1.8851523916253448e-15),
  },
  end: {
    pos: new THREE.Vector3(-184.45143127441406, 25.529351806640626, 7.560695648193359),
    rot: new THREE.Vector3(-0.25176367320531634, 3.1268179607693942, 3.529677289338796e-14),
  },
};

const setupScene = (viz: Viz, loadedWorld: THREE.Group, vizConf: VizConfig) => {
  const ambientLight = new THREE.AmbientLight(0xffffff, 0.8);
  viz.scene.add(ambientLight);

  const sunPos = new THREE.Vector3(0, 80, 0);
  const sunLight = new THREE.DirectionalLight(0xffffff, 1.6);
  const shadowMapSize = {
    [GraphicsQuality.Low]: 1024,
    [GraphicsQuality.Medium]: 2048,
    [GraphicsQuality.High]: 4096,
  }[vizConf.graphics.quality];
  sunLight.castShadow = true;
  // sunLight.shadow.bias = 0.01;
  sunLight.shadow.mapSize.width = shadowMapSize;
  sunLight.shadow.mapSize.height = shadowMapSize;
  sunLight.shadow.camera.near = 0.1;
  sunLight.shadow.camera.far = 100;
  sunLight.shadow.camera.left = -250;
  sunLight.shadow.camera.right = 250;
  sunLight.shadow.camera.top = 250;
  sunLight.shadow.camera.bottom = -250;
  sunLight.shadow.camera.updateProjectionMatrix();
  sunLight.matrixWorldNeedsUpdate = true;
  sunLight.updateMatrixWorld();
  sunLight.position.copy(sunPos);
  viz.scene.add(sunLight);

  // const shadowCameraHelper = new THREE.CameraHelper(sunLight.shadow.camera);
  // viz.scene.add(shadowCameraHelper);

  const metalMat = buildCustomShader({ color: 0xdddddd, metalness: 0.8, roughness: 0.2 });

  loadedWorld.traverse(obj => {
    if (!(obj instanceof THREE.Mesh)) {
      return;
    }

    if (obj.name.startsWith('metal')) {
      console.log(obj);
      obj.material = metalMat;
    }
  });
};

export const processLoadedScene = async (
  viz: Viz,
  loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  const { checkpointMat, greenMosaic2Material, goldMaterial, shinyPatchworkStoneMaterial } =
    await buildPylonsMaterials(viz, loadedWorld);

  const scoreThresholds: ScoreThresholds = {
    [Score.SPlus]: 39,
    [Score.S]: 40,
    [Score.A]: 44,
    [Score.B]: 50,
  };

  const pkManager = new ParkourManager(
    viz,
    loadedWorld,
    vizConf,
    locations,
    scoreThresholds,
    {
      dashToken: { core: greenMosaic2Material, ring: goldMaterial },
      checkpoint: checkpointMat,
    },
    'plats',
    true
  );

  const rt = pkManager.runtime;

  viz.collisionWorldLoadedCbs.push(fpCtx => {
    const spinner1 = loadedWorld.getObjectByName('spinner1')! as THREE.Mesh;
    const spinnerEntity = fpCtx.getEntity(spinner1)!;
    spinnerEntity.body!.setCollisionFlags(2); // CF_KINEMATIC_OBJECT
    spinnerEntity.body!.setActivationState(4); // DISABLE_DEACTIVATION
    const spinnerInitialPos = spinner1.position.clone();
    const spinnerInitialRot = spinner1.rotation.y;
    const spinnerRps = 6.6 / 60;
    const spinnerMatrix = new THREE.Matrix4();
    rt.adoptEntity(spinnerEntity);
    spinnerEntity.addBehavior({
      tick: elapsed => {
        spinnerMatrix.makeRotationY(spinnerInitialRot - spinnerRps * elapsed * Math.PI * 2);
        spinnerMatrix.setPosition(spinnerInitialPos);
        spinnerEntity.setTransform(spinnerMatrix);
      },
    });

    const sliders: THREE.Mesh[] = [];
    const sideSliders: THREE.Mesh[] = [];
    const threeSliders: THREE.Mesh[] = [];
    loadedWorld.traverse(obj => {
      if (obj.name.startsWith('slider') && obj instanceof THREE.Mesh) {
        sliders.push(obj);
        obj.material = shinyPatchworkStoneMaterial;
      } else if (obj.name.startsWith('2slider') && obj instanceof THREE.Mesh) {
        sideSliders.push(obj);
        obj.material = shinyPatchworkStoneMaterial;
      } else if (obj.name.startsWith('3slider') && obj instanceof THREE.Mesh) {
        threeSliders.push(obj);
        obj.material = shinyPatchworkStoneMaterial;
      }
    });

    const makeLinearSlideBehavior =
      (startPos: THREE.Vector3, moveDir: THREE.Vector3, despawnCheck: (pos: THREE.Vector3) => boolean) =>
      (_params: Record<string, unknown>): Behavior => ({
        tick: (elapsed, entity) => {
          const x = startPos.x + moveDir.x * elapsed;
          const y = startPos.y + moveDir.y * elapsed;
          const z = startPos.z + moveDir.z * elapsed;
          if (despawnCheck(entity.object.position)) {
            return 'remove';
          }
          entity.setPosition(x, y, z);
        },
      });

    for (const slider of sliders) {
      slider.removeFromParent();

      const startPos = slider.position.clone();
      rt.registerSpawner(slider.name, slider, {
        interval: 0.7,
        initialDelay: 0.1,
        behaviors: [
          {
            fn: makeLinearSlideBehavior(startPos, new THREE.Vector3(0, 0, 15.8), pos => pos.z > 279),
            params: {},
          },
        ],
      });
    }

    for (const slider of sideSliders) {
      slider.removeFromParent();

      const startPos = slider.position.clone();
      rt.registerSpawner(slider.name, slider, {
        interval: 0.7,
        initialDelay: 0,
        behaviors: [
          {
            fn: makeLinearSlideBehavior(startPos, new THREE.Vector3(14, 0, 0), pos => pos.x > -165),
            params: {},
          },
        ],
      });
    }

    for (const slider of threeSliders) {
      slider.removeFromParent();

      const startPos = slider.position.clone();
      rt.registerSpawner(slider.name, slider, {
        interval: 0.7,
        initialDelay: 0,
        behaviors: [
          {
            fn: makeLinearSlideBehavior(startPos, new THREE.Vector3(-14, 0, 0), pos => pos.x < -209),
            params: {},
          },
        ],
      });
    }
  });

  setupScene(viz, loadedWorld, vizConf);

  initPylonsPostprocessing(viz, vizConf, true);

  return pkManager.buildSceneConfig();
};
