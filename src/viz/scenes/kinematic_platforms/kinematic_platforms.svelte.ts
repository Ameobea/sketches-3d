import * as THREE from 'three';

import type { Viz } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import type { SceneConfig } from '..';
import type { BtRigidBody } from 'src/ammojs/ammoTypes';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { BulletHellManager, type BulletHellEvent } from 'src/viz/bulletHell/BulletHellManager';
import { EasingFnType } from 'src/viz/util/easingFns';
import { ObjectivePadMaterial } from 'src/viz/materials/ObjectivePad/ObjectivePadMaterial';
import { buildGraySToneBricksFloorMaterial } from 'src/viz/materials/GrayStoneBricksFloor/GrayStoneBricksFloorMaterial';

const initLevel = async (viz: Viz) => {
  const fpCtx = viz.fpCtx!;
  const btvec3 = fpCtx.btvec3;

  const stoneMat = await buildGraySToneBricksFloorMaterial(new THREE.ImageBitmapLoader());

  // add a platform to stand on
  const platformGeo = new THREE.BoxGeometry(500, 1, 500);
  const platform: THREE.Mesh<THREE.BoxGeometry, THREE.Material> = new THREE.Mesh(platformGeo, stoneMat);
  platform.receiveShadow = true;
  platform.position.set(0, -7, 0);
  viz.scene.add(platform);
  fpCtx.addTriMesh(platform);

  const kinematicCubeGeo = new THREE.BoxGeometry(5, 5, 50);
  const kinematicCubeMat = new THREE.MeshStandardMaterial({ color: 0x009933 });
  const kinematicCube = new THREE.Mesh(kinematicCubeGeo, kinematicCubeMat);
  kinematicCube.position.set(-15, -4, 0);
  kinematicCube.castShadow = true;
  kinematicCube.receiveShadow = true;
  viz.scene.add(kinematicCube);
  fpCtx.addTriMesh(kinematicCube, 'kinematic');

  const btTransform = new fpCtx.Ammo.btTransform();
  const btQuat = new fpCtx.Ammo.btQuaternion(0, 0, 0, 1);
  btTransform.setIdentity();
  viz.registerBeforeRenderCb(curTimeSeconds => {
    const t = curTimeSeconds * 0.5 * 15;
    kinematicCube.position.set(-15 + Math.sin(t * 0.2) * 2, -4, 0);
    kinematicCube.rotation.set(0, t * 0.02, 0);

    kinematicCube.updateMatrixWorld();
    btTransform.setOrigin(
      btvec3(kinematicCube.position.x, kinematicCube.position.y, kinematicCube.position.z)
    );
    btQuat.setValue(
      kinematicCube.quaternion.x,
      kinematicCube.quaternion.y,
      kinematicCube.quaternion.z,
      kinematicCube.quaternion.w
    );
    btTransform.setRotation(btQuat);
    const rigidBody = kinematicCube.userData.rigidBody as BtRigidBody;
    // rigidBody.setWorldTransform(btTransform);

    // using motion state gives us interpolation for internal physics engine ticks.
    // this helps reduce collision issues from fast-moving objects by treating it as if
    // it moves steadily from its previous position to this new one over the course of
    // one animation frame.
    const motionState = rigidBody.getMotionState()!;
    motionState.setWorldTransform(btTransform);
  });

  // const bulletGeo = new THREE.SphereGeometry(2, 16, 16);
  // const bulletMat = buildCustomShader({ color: 0xff0000 }, {}, { materialClass: MaterialClass.Instakill });
  // const bullet = new THREE.Mesh(bulletGeo, bulletMat);
  // bullet.castShadow = true;
  // bullet.receiveShadow = true;
  // viz.scene.add(bullet);
  // fpCtx.addTriMesh(bullet);
  // const bulletCollider: BtCollisionObject = bullet.userData.collisionObj;

  // viz.registerBeforeRenderCb(curTimeSeconds => {
  //   const t = curTimeSeconds * 0.5 * 3;
  //   bullet.position.set(10 + Math.sin(t) * 20, -4, 0);
  //   const tfn = bulletCollider.getWorldTransform();
  //   tfn.setOrigin(btvec3(bullet.position.x, bullet.position.y, bullet.position.z));
  //   bulletCollider.setWorldTransform(tfn);
  // });

  const bulletHellEvents: BulletHellEvent[] = [
    {
      type: 'spawnPattern',
      pattern: { type: 'circle', count: 250, direction: 'cw', revolutions: 8 },
      pos: new THREE.Vector3(-20, -4, -20),
      time: 1,
      spawnIntervalSeconds: 0.04,
      velocity: 15,
    },
    {
      type: 'spawnPattern',
      pattern: { type: 'circle', count: 250, direction: 'cw', revolutions: 8 },
      pos: new THREE.Vector3(20, -4, 20),
      time: 3,
      spawnIntervalSeconds: 0.04,
      velocity: 15,
    },
    {
      type: 'spawnPattern',
      pattern: { type: 'circle', count: 250, direction: 'cw', revolutions: 8 },
      pos: new THREE.Vector3(-20, -4, 20),
      time: 6,
      spawnIntervalSeconds: 0.04,
      velocity: 15,
    },
    {
      type: 'spawnPattern',
      pattern: { type: 'circle', count: 250, direction: 'cw', revolutions: 8 },
      pos: new THREE.Vector3(20, -4, -20),
      time: 10,
      spawnIntervalSeconds: 0.04,
      velocity: 15,
    },
  ];
  const manager = new BulletHellManager(
    viz,
    bulletHellEvents,
    new THREE.Box3(new THREE.Vector3(-100, -100, -100), new THREE.Vector3(100, 100, 100))
  );

  viz.registerBeforeRenderCb(curTimeSeconds => ObjectivePadMaterial.setCurTimeSeconds(curTimeSeconds));
  const startPlatform = new THREE.Mesh(new THREE.BoxGeometry(3.55, 1, 3.55), ObjectivePadMaterial);
  startPlatform.position.set(-10, -6.2, -20);
  viz.scene.add(startPlatform);

  const initStartPlatform = () => {
    const startPlatformGhostObj = viz.fpCtx!.addPlayerRegionContactCb(
      { type: 'mesh', mesh: startPlatform },
      async () => {
        viz.scene.remove(startPlatform);
        viz.fpCtx!.removePlayerRegionContactCb(startPlatformGhostObj);
        const outcome = await manager.start();
        switch (outcome.type) {
          case 'win':
            // TODO
            break;
          case 'loss':
            initStartPlatform();
            break;
          default:
            outcome satisfies never;
            throw new Error('unreachable');
        }
      }
    );
    viz.scene.add(startPlatform);
  };

  initStartPlatform();
};

export const processLoadedScene = async (
  viz: Viz,
  _loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  const ambientLight = new THREE.AmbientLight(0xffffff, 0.8);
  viz.scene.add(ambientLight);

  const sunPos = new THREE.Vector3(0, 80, 0);
  const sunLight = new THREE.DirectionalLight(0xffffff, 3.6);
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

  const playerHeight = 2.2;
  const playerRadius = 0.5;
  const playerMesh = new THREE.Mesh(
    new THREE.CapsuleGeometry(playerRadius, playerHeight, 16, 16),
    buildCustomShader({
      color: new THREE.Color(0xad6dcf),
      metalness: 0.18,
      roughness: 0.82,
    })
  );
  playerMesh.castShadow = true;
  playerMesh.receiveShadow = true;

  viz.collisionWorldLoadedCbs.push(() => void initLevel(viz));

  // initPylonsPostprocessing(viz, vizConf);
  configureDefaultPostprocessingPipeline(
    viz,
    vizConf.graphics.quality,
    undefined,
    undefined,
    undefined,
    undefined,
    true
  );

  const viewMode: NonNullable<SceneConfig['viewMode']> = {
    type: 'top-down',
    cameraFocusPoint: { type: 'fixed', pos: new THREE.Vector3(0, 0, 0) },
    cameraFOV: 40,

    // \/ isometric-like
    // cameraOffset: new THREE.Vector3(0, 120, -100),
    // cameraRotation: new THREE.Euler(-0.8, Math.PI, 0, 'YXZ'),

    // \/ almost top-down
    cameraOffset: new THREE.Vector3(0, 85, -28),
    cameraRotation: new THREE.Euler(-1.3, Math.PI, 0, 'YXZ'),
  };

  return {
    spawnLocation: 'spawn',
    gravity: 60,
    player: {
      moveSpeed: { onGround: 16, inAir: 16 },
      colliderSize: { height: playerHeight, radius: playerRadius },
      playerColliderShape: 'capsule',
      // playerColliderShape: 'sphere',
      jumpVelocity: 20,
      oobYThreshold: -10,
      dashConfig: {
        enable: true,
        useExternalVelocity: true,
        sfx: { play: true, name: 'dash' },
        dashMagnitude: 20,
      },
      externalVelocityAirDampingFactor: new THREE.Vector3(0.32, 0.0, 0.32),
      externalVelocityGroundDampingFactor: new THREE.Vector3(0.999992, 0.999992, 0.999992),
      mesh: playerMesh,
    },
    viewMode,
    debugPos: true,
    debugCamera: true,
    debugPlayerKinematics: true,
    locations: {
      spawn: {
        pos: [1.154, 8.776, -0.2],
        rot: [-0.8228, 0, 0],
      },
    },
    legacyLights: false,
    goBackOnLoad: false,
    sfx: {
      neededSfx: ['dash'],
    },
    customControlsEntries: [
      {
        label: 'Top-Down View Mode',
        key: '2',
        action: () => {
          viz.sceneConf.player!.moveSpeed = { onGround: 10, inAir: 10 };
          viz.setViewMode(viewMode, EasingFnType.InOutCubic, 1.2);
        },
      },
      {
        label: 'First-Person View Mode',
        key: '1',
        action: () => {
          viz.sceneConf.player!.moveSpeed = { onGround: 10, inAir: 13 };
          viz.setViewMode({ type: 'firstPerson' }, EasingFnType.InOutCubic, 1.2);
        },
      },
      {
        label: 'Isometric-Like View Mode',
        key: '3',
        action: () => {
          viz.sceneConf.player!.moveSpeed = { onGround: 10, inAir: 13 };
          viz.setViewMode(
            {
              ...viewMode,
              cameraOffset: new THREE.Vector3(0, 60, -50),
              cameraRotation: new THREE.Euler(-0.8, Math.PI, 0, 'YXZ'),
              cameraFocusPoint: { type: 'player' },
            },
            EasingFnType.InOutCubic,
            1.2
          );
        },
      },
    ],
  };
};
