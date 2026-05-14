import * as THREE from 'three';

export interface TransformSnapshot {
  position: [number, number, number];
  rotation: [number, number, number];
  scale: [number, number, number];
}

/** Relative change of a transform op; can be re-applied to a different object. */
export interface ReplayableTransformDelta {
  positionDelta: [number, number, number];
  rotationDelta: [number, number, number];
  scaleFactor: [number, number, number];
}

const SNAP_EPS = 1e-6;

export const snapshotTransform = (obj: THREE.Object3D): TransformSnapshot => {
  const r = obj.rotation;
  return {
    position: obj.position.toArray() as [number, number, number],
    rotation: [r.x, r.y, r.z],
    scale: obj.scale.toArray() as [number, number, number],
  };
};

export const applySnapshot = (obj: THREE.Object3D, snap: TransformSnapshot) => {
  obj.position.fromArray(snap.position);
  obj.rotation.set(snap.rotation[0], snap.rotation[1], snap.rotation[2]);
  obj.scale.fromArray(snap.scale);
};

/** Snapshots `obj`'s world-space transform; calls `updateMatrixWorld` first. */
export const snapshotWorldTransform = (obj: THREE.Object3D): TransformSnapshot => {
  obj.updateMatrixWorld(true);
  const pos = new THREE.Vector3();
  const quat = new THREE.Quaternion();
  const scale = new THREE.Vector3();
  obj.matrixWorld.decompose(pos, quat, scale);
  const euler = new THREE.Euler().setFromQuaternion(quat);
  return {
    position: [pos.x, pos.y, pos.z],
    rotation: [euler.x, euler.y, euler.z],
    scale: [scale.x, scale.y, scale.z],
  };
};

/** Re-expresses a world-space snapshot in `parent`'s local space. */
export const worldToLocalSnapshot = (world: TransformSnapshot, parent: THREE.Object3D): TransformSnapshot => {
  parent.updateMatrixWorld(true);
  const worldMatrix = new THREE.Matrix4().compose(
    new THREE.Vector3().fromArray(world.position),
    new THREE.Quaternion().setFromEuler(
      new THREE.Euler(world.rotation[0], world.rotation[1], world.rotation[2])
    ),
    new THREE.Vector3().fromArray(world.scale)
  );
  const localMatrix = new THREE.Matrix4().copy(parent.matrixWorld).invert().multiply(worldMatrix);
  const pos = new THREE.Vector3();
  const quat = new THREE.Quaternion();
  const scale = new THREE.Vector3();
  localMatrix.decompose(pos, quat, scale);
  const euler = new THREE.Euler().setFromQuaternion(quat);
  return {
    position: [pos.x, pos.y, pos.z],
    rotation: [euler.x, euler.y, euler.z],
    scale: [scale.x, scale.y, scale.z],
  };
};

export const snapshotsEqual = (a: TransformSnapshot, b: TransformSnapshot): boolean => {
  for (let i = 0; i < 3; i++) {
    if (Math.abs(a.position[i] - b.position[i]) > SNAP_EPS) return false;
    if (Math.abs(a.rotation[i] - b.rotation[i]) > SNAP_EPS) return false;
    if (Math.abs(a.scale[i] - b.scale[i]) > SNAP_EPS) return false;
  }
  return true;
};
