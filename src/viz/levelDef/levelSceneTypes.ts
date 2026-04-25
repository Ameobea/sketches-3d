import type * as THREE from 'three';

import type { Entity } from '../sceneRuntime/Entity';
import type { LightDef, ObjectDef, ObjectGroupDef } from './types';

export interface LevelObject {
  id: string;
  assetId: string;
  /**
   * The placed Three.js mesh.
   *
   * **Invariant:** a leaf object always resolves to a single `THREE.Mesh`.
   * Multi-mesh content must be represented as explicit groups of leaf objects.
   */
  object: THREE.Mesh;
  def: ObjectDef;
  /** True when this object was produced by a generator (read-only in the editor). */
  generated: boolean;
  /**
   * The entity wrapping this LevelObject.  Owns at most one collision body,
   * material class, and behavior attachments.  Created at placement time
   * (possibly before physics is ready); the body is attached later when
   * physics registers.
   */
  entity: Entity;
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
