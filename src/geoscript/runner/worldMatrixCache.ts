import * as THREE from 'three';

import type { NodeDef, Transform3, TreeDef } from '../geotoyAPIClient';

export interface NodeWorldInstance {
  world: THREE.Matrix4;
  /** Instance indices from root → this node, inclusive. Identifies one materialized copy. */
  path: number[];
}
export type WorldMatrixCache = Map<string, NodeWorldInstance[]>;

const _scratchEuler = new THREE.Euler();
const _scratchQuat = new THREE.Quaternion();
const _scratchPos = new THREE.Vector3();
const _scratchScale = new THREE.Vector3();

/** Transform3 → Matrix4 with the geoscript 'YXZ' euler convention. Writes into `out`. */
export const composeTransform3 = (out: THREE.Matrix4, t: Transform3): THREE.Matrix4 => {
  _scratchEuler.set(t.rot[0], t.rot[1], t.rot[2], 'YXZ');
  _scratchQuat.setFromEuler(_scratchEuler);
  _scratchPos.set(t.pos[0], t.pos[1], t.pos[2]);
  _scratchScale.set(t.scale[0], t.scale[1], t.scale[2]);
  return out.compose(_scratchPos, _scratchQuat, _scratchScale);
};

/** Matrix4 → Transform3 with the geoscript 'YXZ' euler convention (inverse of composeTransform3). */
export const decomposeTransform3 = (m: THREE.Matrix4): Transform3 => {
  m.decompose(_scratchPos, _scratchQuat, _scratchScale);
  _scratchEuler.setFromQuaternion(_scratchQuat, 'YXZ');
  return {
    pos: [_scratchPos.x, _scratchPos.y, _scratchPos.z],
    rot: [_scratchEuler.x, _scratchEuler.y, _scratchEuler.z],
    scale: [_scratchScale.x, _scratchScale.y, _scratchScale.z],
  };
};

/** Canonical string key for an instance path — the identity shared by reuse keys and fast-path lookup. */
export const instancePathKey = (path: number[]): string => path.join(',');

/**
 * Memoized world-matrix lookup. A node resolves to multiple rendered copies — the
 * cartesian product of instance counts along its ancestor chain — so each entry is a
 * list of `{ world = parent.world × node.instances[i], path }`. Computed once per
 * `populateScene` call; pulls every ancestor through `parentMap` with a cycle guard.
 */
export const buildWorldMatrixCache = (tree: TreeDef, parentMap: Map<string, string>): WorldMatrixCache => {
  const cache: WorldMatrixCache = new Map();
  const localMats = (node: NodeDef): THREE.Matrix4[] =>
    node.instances.map(t => composeTransform3(new THREE.Matrix4(), t));
  const get = (id: string, visiting: Set<string>): NodeWorldInstance[] => {
    const cached = cache.get(id);
    if (cached) return cached;
    const node = tree.nodes[id];
    if (!node || visiting.has(id)) {
      const fallback: NodeWorldInstance[] = [{ world: new THREE.Matrix4(), path: [] }];
      cache.set(id, fallback);
      return fallback;
    }
    visiting.add(id);
    const parentId = parentMap.get(id);
    const parents = parentId
      ? get(parentId, visiting)
      : [{ world: new THREE.Matrix4(), path: [] as number[] }];
    const mats = localMats(node);
    const out: NodeWorldInstance[] = [];
    for (const pi of parents) {
      for (let i = 0; i < mats.length; i++) {
        out.push({ world: pi.world.clone().multiply(mats[i]), path: [...pi.path, i] });
      }
    }
    visiting.delete(id);
    cache.set(id, out);
    return out;
  };
  for (const id of Object.keys(tree.nodes)) get(id, new Set());
  return cache;
};
