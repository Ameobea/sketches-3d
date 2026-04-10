import * as THREE from 'three';

import type { BehaviorFn } from '../sceneRuntime/types';

/**
 * Moves an entity linearly along a velocity vector. Returns null (despawn)
 * when the entity has traveled beyond `maxDistance` from its origin.
 *
 * Params:
 *   velocity: [number, number, number] — units per second
 *   maxDistance: number — despawn after traveling this far
 */
const linearSlide: BehaviorFn = params => {
  const velocity = params.velocity as [number, number, number];
  const maxDistance = params.maxDistance as number;
  const mat = new THREE.Matrix4();
  const maxDistSq = maxDistance * maxDistance;

  return {
    tick(elapsed, entity) {
      const dx = velocity[0] * elapsed;
      const dy = velocity[1] * elapsed;
      const dz = velocity[2] * elapsed;

      if (dx * dx + dy * dy + dz * dz > maxDistSq) {
        return 'remove';
      }

      mat.makeTranslation(dx, dy, dz);
      entity.setTransform(entity.baseTransform.clone().multiply(mat));
    },
  };
};

export default linearSlide;
