import * as THREE from 'three';

import type { Viz } from 'src/viz';
import { initCollectables, type CollectablesCtx } from './collectables';
import type { TransparentWritable } from '../util/TransparentWritable';

/**
 * The names of checkpoints and dash tokens must be very particularly set.
 *
 * Checkpoints are named "checkpoint", "checkpoint001", "checkpoint002", etc.
 *
 * Dash tokens are like `dash_token_loc_ck0.001`, `dash_token_loc_ck0`, etc.
 *
 * The `n` in `ck{n}` should be set to the index of the checkpoint that is reached before
 * that dash token can be picked up.  So if the dash token is reachable before any checkpoint
 * is reached, it should be `ck0`.  This controls the respawn behavior of dash tokens when
 * respawning at checkpoints.
 */
export const initCheckpoints = (
  viz: Viz,
  loadedWorld: THREE.Group<THREE.Object3DEventMap>,
  checkpointMat: THREE.Material,
  dashTokensCtx: CollectablesCtx,
  curDashCharges: TransparentWritable<number>,
  onComplete: () => void
) => {
  let latestReachedCheckpointIx: number | null = 0;
  let dashChargesAtLastCheckpoint = 0;
  const setSpawnPoint = (pos: THREE.Vector3, rot: THREE.Vector3) => viz.setSpawnPos(pos, rot);

  const parseCheckpointIx = (name: string) => {
    // names are like "checkpoint", "checkpoint001", "checkpoint002", etc.
    const match = name.match(/checkpoint(\d+)/);
    if (!match) {
      return 0;
    }

    return parseInt(match[1], 10);
  };

  const ctx = initCollectables({
    viz,
    loadedWorld,
    collectableName: 'checkpoint',
    onCollect: checkpoint => {
      setSpawnPoint(
        checkpoint.position,
        new THREE.Vector3(viz.camera.rotation.x, viz.camera.rotation.y, viz.camera.rotation.z)
      );

      // TODO: sfx

      const checkpointIx = parseCheckpointIx(checkpoint.name);
      latestReachedCheckpointIx = checkpointIx;
      dashChargesAtLastCheckpoint = curDashCharges.current;
      if (checkpointIx === 1) {
        onComplete();
      }
    },
    material: checkpointMat,
    collisionRegionScale: new THREE.Vector3(1, 1, 1),
  });

  viz.registerOnRespawnCb(() => {
    curDashCharges.set(dashChargesAtLastCheckpoint);

    const needle = `ck${latestReachedCheckpointIx === null ? 0 : latestReachedCheckpointIx + 1}`;
    const toRestore: THREE.Object3D[] = [];
    for (const obj of dashTokensCtx.hiddenCollectables) {
      if (obj.name.includes(needle)) {
        toRestore.push(obj);
      }
    }

    dashTokensCtx.restore(toRestore);
    viz.fpCtx!.reset();
  });

  const reset = () => {
    ctx.reset();
    latestReachedCheckpointIx = null;
    dashChargesAtLastCheckpoint = 0;
  };
  return reset;
};
