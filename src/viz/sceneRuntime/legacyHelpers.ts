import type * as THREE from 'three';

import type { BtRigidBody } from 'src/ammojs/ammoTypes';
import type { SceneRuntime } from './SceneRuntime';

/**
 * Legacy helper that replicates the old ParkourManager.makeSpinner behavior
 * using SceneRuntime's ticker/lifecycle system directly.
 *
 * Sets up a kinematic mesh that rotates around its Y axis at `rpm` revolutions
 * per minute.  Handles reset (snap back to initial rotation) and destroy
 * (clean up Ammo transform).
 */
export const makeSpinner = (runtime: SceneRuntime, mesh: THREE.Mesh, rpm: number) => {
  const fpCtx = runtime.fpCtx;
  if (!fpCtx) {
    throw new Error('SceneRuntime: physics not ready');
  }

  const rigidBody = mesh.userData.rigidBody as BtRigidBody;
  rigidBody.setCollisionFlags(2); // CF_KINEMATIC_OBJECT
  rigidBody.setActivationState(4); // DISABLE_DEACTIVATION
  const tfn = new fpCtx.Ammo.btTransform();
  tfn.setIdentity();
  tfn.setOrigin(fpCtx.btvec3(mesh.position.x, mesh.position.y, mesh.position.z));
  const initialRot = mesh.rotation.y;
  const rps = rpm / 60;

  const makeSpinnerTicker = () => ({
    tick: (physicsTime: number) => {
      tfn.setEulerZYX(0, initialRot - rps * physicsTime * Math.PI * 2, 0);
      rigidBody.setWorldTransform(tfn);
    },
  });

  runtime.registerTicker(makeSpinnerTicker(), { mesh, body: rigidBody });

  runtime.registerResetCb(() => {
    tfn.setEulerZYX(0, initialRot, 0);
    rigidBody.setWorldTransform(tfn);
    runtime.registerTicker(makeSpinnerTicker(), { mesh, body: rigidBody });
  });

  runtime.registerDestroyCb(() => {
    runtime.fpCtx?.Ammo.destroy(tfn);
  });
};

interface MakeSliderArgs {
  getPos: (curTimeSeconds: number, secondsSinceSpawn: number) => THREE.Vector3;
  despawnCond?: (mesh: THREE.Mesh, curTimeSeconds: number) => boolean;
  /** default true */
  removeOnReset?: boolean;
  spawnTimeSeconds?: number;
}

/**
 * Legacy helper that replicates the old ParkourManager.makeSlider behavior
 * using SceneRuntime's ticker/lifecycle system directly.
 */
export const makeSlider = (
  runtime: SceneRuntime,
  mesh: THREE.Mesh,
  { getPos, despawnCond, spawnTimeSeconds, removeOnReset = true }: MakeSliderArgs
) => {
  const fpCtx = runtime.fpCtx;
  if (!fpCtx) {
    throw new Error('SceneRuntime: physics not ready');
  }

  const resolvedSpawnTimeSeconds = spawnTimeSeconds ?? fpCtx.getPhysicsTime();

  let rigidBody = mesh.userData.rigidBody as BtRigidBody | undefined;
  if (!rigidBody) {
    if (mesh.userData.collisionObj) {
      throw new Error('Unhandled case where slider has collision object but no rigid body');
    }

    fpCtx.addTriMesh(mesh, 'kinematic');
    rigidBody = mesh.userData.rigidBody as BtRigidBody;
  } else {
    rigidBody.setCollisionFlags(2); // CF_KINEMATIC_OBJECT
    rigidBody.setActivationState(4); // DISABLE_DEACTIVATION
  }

  const tfn = new fpCtx.Ammo.btTransform();
  tfn.setIdentity();
  tfn.setOrigin(fpCtx.btvec3(mesh.position.x, mesh.position.y, mesh.position.z));
  tfn.setEulerZYX(mesh.rotation.x, mesh.rotation.y, mesh.rotation.z);

  let disposed = false;
  let removedFromWorld = false;
  let cleanupQueued = false;
  let tickerHandle = registerSliderTicker();

  function removeSliderFromWorld() {
    if (removedFromWorld) return;
    runtime.viz.scene.remove(mesh);
    fpCtx!.removeCollisionObject(rigidBody!);
    removedFromWorld = true;
  }

  function queueCleanup() {
    if (cleanupQueued) return;
    cleanupQueued = true;
    runtime.queuePhysicsAction(() => {
      cleanupQueued = false;
      runtime.unregisterTicker(tickerHandle);
      if (removeOnReset) {
        removeSliderFromWorld();
        return;
      }
      const startPos = getPos(resolvedSpawnTimeSeconds, 0);
      mesh.position.copy(startPos);
      tfn.setOrigin(fpCtx!.btvec3(startPos.x, startPos.y, startPos.z));
      rigidBody!.setWorldTransform(tfn);
      tickerHandle = registerSliderTicker();
    });
  }

  function registerSliderTicker() {
    disposed = false;
    return runtime.registerTicker(
      {
        tick: physicsTime => {
          if (disposed) return;

          if (despawnCond?.(mesh, physicsTime)) {
            disposed = true;
            queueCleanup();
            return;
          }

          const secondsSinceSpawn = physicsTime - resolvedSpawnTimeSeconds;
          const newPos = getPos(physicsTime, secondsSinceSpawn);
          tfn.setOrigin(fpCtx!.btvec3(newPos.x, newPos.y, newPos.z));
          rigidBody!.setWorldTransform(tfn);
        },
      },
      { mesh, body: rigidBody }
    );
  }

  runtime.registerResetCb(() => {
    disposed = false;
    cleanupQueued = false;
    if (removeOnReset) {
      removeSliderFromWorld();
      return;
    }
    const startPos = getPos(resolvedSpawnTimeSeconds, 0);
    mesh.position.copy(startPos);
    tfn.setOrigin(fpCtx!.btvec3(startPos.x, startPos.y, startPos.z));
    rigidBody!.setWorldTransform(tfn);
    tickerHandle = registerSliderTicker();
  });

  runtime.registerDestroyCb(() => fpCtx!.Ammo.destroy(tfn));
};
