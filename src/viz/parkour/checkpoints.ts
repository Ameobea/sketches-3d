import * as THREE from 'three';

import type { Viz } from 'src/viz';
import { initCollectables } from './collectables';

/**
 * Checkpoints are named "checkpoint", "checkpoint001", "checkpoint002", etc.
 *
 * Dash token state is now snapshotted in the C++ controller when checkpoints are
 * reached, and restored from that snapshot on respawn.
 */
export const initCheckpoints = (
  viz: Viz,
  loadedWorld: THREE.Group<THREE.Object3DEventMap>,
  checkpointMat: THREE.Material | (() => THREE.Material) | undefined,
  syncDashTokensFromController: () => void,
  onComplete: () => void,
  checkpointMeshes?: THREE.Mesh[]
) => {
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
    meshes: checkpointMeshes,
    onCollect: checkpoint => {
      const worldPos = new THREE.Vector3();
      checkpoint.getWorldPosition(worldPos);
      setSpawnPoint(
        worldPos,
        new THREE.Vector3(viz.camera.rotation.x, viz.camera.rotation.y, viz.camera.rotation.z)
      );

      // TODO: sfx

      const checkpointIx = parseCheckpointIx(checkpoint.name);
      viz.fpCtx?.saveDashCheckpointState();
      if (checkpointIx === 1 || checkpoint.name.includes('win')) {
        onComplete();
      }
    },
    material: checkpointMat,
    collisionRegionScale: new THREE.Vector3(1, 1, 1),
  });

  viz.registerOnRespawnCb(() => {
    viz.fpCtx?.restoreDashCheckpointState();
    syncDashTokensFromController();
    viz.fpCtx!.resetPlayerState();
  });

  const reset = () => {
    ctx.reset();
  };
  return reset;
};
