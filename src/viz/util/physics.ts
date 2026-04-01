import * as THREE from 'three';

import type { Viz } from 'src/viz';

type CollisionHandle = unknown;

interface CollisionRemover {
  removeCollisionObject: (collisionObj: CollisionHandle, meshName?: string) => void;
}

export const withPhysicsContext = (
  viz: Pick<Viz, 'fpCtx' | 'collisionWorldLoadedCbs'>,
  cb: (fpCtx: NonNullable<Viz['fpCtx']>) => void
) => {
  if (viz.fpCtx) {
    cb(viz.fpCtx);
  } else {
    viz.collisionWorldLoadedCbs.push(cb);
  }
};

export const clearPhysicsBinding = (
  object: THREE.Object3D,
  fpCtx: CollisionRemover,
  meshName: string = object.name
) => {
  if (object.userData.rigidBody) {
    fpCtx.removeCollisionObject(object.userData.rigidBody, meshName);
    object.userData.rigidBody = undefined;
    return true;
  }

  if (object.userData.collisionObj) {
    fpCtx.removeCollisionObject(object.userData.collisionObj, meshName);
    object.userData.collisionObj = undefined;
    return true;
  }

  return false;
};

export const clearPhysicsBindings = (
  object: THREE.Object3D,
  fpCtx: CollisionRemover,
  opts: { meshesOnly?: boolean } = {}
) => {
  object.traverse(child => {
    if (opts.meshesOnly && !(child instanceof THREE.Mesh)) {
      return;
    }

    clearPhysicsBinding(child, fpCtx);
  });
};
