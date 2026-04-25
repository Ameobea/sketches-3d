import * as THREE from 'three';

import type { Viz } from 'src/viz';
import type { BulletPhysics } from 'src/viz/collision';

type PhysicsBindingCtx = Pick<BulletPhysics, 'removeCollisionObject' | 'getEntity'>;

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
  fpCtx: PhysicsBindingCtx,
  meshName: string = object.name
) => {
  const entity = fpCtx.getEntity(object);
  if (entity?.body) {
    fpCtx.removeCollisionObject(entity.body, meshName);
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
  fpCtx: PhysicsBindingCtx,
  opts: { meshesOnly?: boolean } = {}
) => {
  object.traverse(child => {
    if (opts.meshesOnly && !(child instanceof THREE.Mesh)) {
      return;
    }

    clearPhysicsBinding(child, fpCtx);
  });
};
