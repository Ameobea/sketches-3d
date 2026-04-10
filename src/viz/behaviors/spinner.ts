import * as THREE from 'three';

import type { BehaviorFn } from '../sceneRuntime/types';

/**
 * Rotates an entity at a constant rate. The entity's rigid body must be kinematic.
 *
 * Params:
 *   rps: [number, number, number] — rotations per second around each Euler axis
 */
const spinner: BehaviorFn = params => {
  const rps = params.rps as [number, number, number];
  const euler = new THREE.Euler();
  const mat = new THREE.Matrix4();

  return {
    tick(elapsed, entity) {
      euler.set(
        rps[0] * elapsed * Math.PI * 2,
        rps[1] * elapsed * Math.PI * 2,
        rps[2] * elapsed * Math.PI * 2
      );
      mat.makeRotationFromEuler(euler);
      entity.setTransform(entity.baseTransform.clone().multiply(mat));
    },
    onReset() {
      // Nothing to reset — tick is purely a function of elapsed time
    },
  };
};

export default spinner;
