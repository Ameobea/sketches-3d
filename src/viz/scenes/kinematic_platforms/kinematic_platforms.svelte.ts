import * as THREE from 'three';

import type { VizState } from 'src/viz';
import { type VizConfig } from 'src/viz/conf';
import type { SceneConfig } from '..';
import { buildMaterials } from '../../parkour/regions/pylons/materials';
import { initPylonsPostprocessing } from '../pkPylons/postprocessing';
import type { BtRigidBody } from 'src/ammojs/ammoTypes';

const initLevel = async (viz: VizState, matsPromise: ReturnType<typeof buildMaterials>) => {
  const fpCtx = viz.fpCtx!;
  const btvec3 = fpCtx.btvec3;

  const { pylonMaterial } = await matsPromise;

  // add a platform to stand on
  const platformGeo = new THREE.BoxGeometry(500, 1, 500);
  const platform = new THREE.Mesh(platformGeo, pylonMaterial);
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
    kinematicCube.position.set(-15 + Math.sin(t) * 2, -4, 0);
    kinematicCube.rotation.set(0, t * 0.2, 0);

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

  const bulletCubeGeo = new THREE.BoxGeometry(2, 2, 2);
  const bulletCubeMat = new THREE.MeshStandardMaterial({ color: 0xff0000 });
  const bulletCube = new THREE.Mesh(bulletCubeGeo, bulletCubeMat);
  bulletCube.castShadow = true;
  bulletCube.receiveShadow = true;
  viz.scene.add(bulletCube);
  const bulletCollider = fpCtx.addPlayerRegionContactCb(
    {
      type: 'box',
      halfExtents: new THREE.Vector3(
        bulletCubeGeo.parameters.width / 2,
        bulletCubeGeo.parameters.height / 2,
        bulletCubeGeo.parameters.depth / 2
      ),
      pos: bulletCube.position,
    },
    () => console.log('entered'),
    () => console.log('left')
  );

  viz.registerBeforeRenderCb(curTimeSeconds => {
    const t = curTimeSeconds * 0.5 * 3;
    bulletCube.position.set(10 + Math.sin(t) * 20, -4, 0);
    const tfn = bulletCollider.getWorldTransform();
    tfn.setOrigin(btvec3(bulletCube.position.x, bulletCube.position.y, bulletCube.position.z));
    bulletCollider.setWorldTransform(tfn);
  });
};

export const processLoadedScene = async (
  viz: VizState,
  loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  const ambientLight = new THREE.AmbientLight(0xffffff, 0.8);
  viz.scene.add(ambientLight);

  const matsPromise = buildMaterials(viz, loadedWorld);

  const sunPos = new THREE.Vector3(200, 290, -135);
  const sunLight = new THREE.DirectionalLight(0xffffff, 3.6);
  sunLight.position.copy(sunPos);
  viz.scene.add(sunLight);

  viz.collisionWorldLoadedCbs.push(() => void initLevel(viz, matsPromise));

  initPylonsPostprocessing(viz, vizConf);

  return {
    spawnLocation: 'spawn',
    gravity: 30,
    player: {
      moveSpeed: { onGround: 10, inAir: 13 },
      colliderSize: { height: 2.2, radius: 0.8 },
      playerColliderShape: 'cylinder',
      jumpVelocity: 12,
      oobYThreshold: -10,
      dashConfig: {
        enable: true,
        useExternalVelocity: true,
        sfx: { play: true, name: 'dash' },
      },
      externalVelocityAirDampingFactor: new THREE.Vector3(0.32, 0.3, 0.32),
      externalVelocityGroundDampingFactor: new THREE.Vector3(0.9992, 0.9992, 0.9992),
    },
    debugPos: true,
    debugCamera: true,
    debugPlayerKinematics: true,
    locations: {
      spawn: {
        pos: [1.154, 8.776, -0.2],
        rot: [-0.8228, -48.782, 0],
      },
    },
    legacyLights: false,
    goBackOnLoad: false,
    sfx: {
      neededSfx: ['dash'],
    },
  };
};
