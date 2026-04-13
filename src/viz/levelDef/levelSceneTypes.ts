import type * as THREE from 'three';

import type { LightDef, ObjectDef, ObjectGroupDef } from './types';

export interface LevelObject {
  id: string;
  assetId: string;
  /**
   * The placed Three.js object. Will be a THREE.Mesh for single-mesh assets
   * (gltf or single-output geoscript) or a THREE.Group for multi-mesh geoscript output.
   * Use `.traverse()` to reach individual meshes for material assignment.
   */
  object: THREE.Object3D;
  def: ObjectDef;
  /** True when this object was produced by a generator (read-only in the editor). */
  generated: boolean;
}

export interface LevelGroup {
  id: string;
  object: THREE.Group;
  def: ObjectGroupDef;
  children: LevelSceneNode[];
  /** True when this group was produced by a generator (read-only in the editor). */
  generated: boolean;
}

export type LevelSceneNode = LevelObject | LevelGroup;

export const isLevelGroup = (n: LevelSceneNode): n is LevelGroup => 'children' in n;

export interface LevelLight {
  id: string;
  light: THREE.Light;
  target?: THREE.Object3D;
  def: LightDef;
}
