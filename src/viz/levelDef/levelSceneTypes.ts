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
  /**
   * Set when this object is opaque content of a pointer node (a composition baked mesh):
   * raycast/selection resolves to `owner`, and it lives outside the node tree
   * (`nodeById`/`children`) — parked on `owner.opaqueParts`. It still uses the normal leaf
   * plumbing (allLevelObjects, physics, deferred materials).
   */
  owner?: LevelGroup;
}

/**
 * Runtime view of an `ObjectGroupDef` minus its `children` field. The runtime
 * hierarchy lives only in `LevelGroup.children`; serialize via `serializeGroup`
 * when a wire-format `ObjectGroupDef` is needed.
 */
export type LevelGroupBody = Omit<ObjectGroupDef, 'children'>;

export interface LevelGroup {
  id: string;
  object: THREE.Group;
  def: LevelGroupBody;
  children: LevelSceneNode[];
  /** True when this group was produced by a generator (read-only in the editor). */
  generated: boolean;
  /**
   * Set when this group is the runtime expansion of a `geotoyComposition` placement. Holds the
   * source leaf `ObjectDef` (the persisted pointer: asset/material/nocollide); its transform is
   * stale after edits — use the live group transform. Drives composition-aware clone/restore so
   * the def only ever round-trips the pointer, never the expanded children.
   */
  compositionDef?: ObjectDef;
  /**
   * Opaque materialized content (a composition's baked meshes). Rendered under `.object` and
   * selected/transformed/deleted as this group, never serialized as nodes. Unlike `children`
   * (editable nodes), these are absent from `nodeById`; they reuse leaf plumbing via their `owner`.
   */
  opaqueParts?: LevelObject[];
}

export type LevelSceneNode = LevelObject | LevelGroup;

export const isLevelGroup = (n: LevelSceneNode): n is LevelGroup => 'children' in n;

/** False for read-only generator output / composition internals — gates editor mutations
 *  (transform, copy, delete, reparent, drag). */
export const isEditable = (n: LevelSceneNode): boolean => !n.generated;

export type CompositionNode = LevelGroup & { compositionDef: ObjectDef };

/** An opaque composition placement: a pointer group whose expanded children never serialize. */
export const isCompositionNode = (n: LevelSceneNode): n is CompositionNode =>
  isLevelGroup(n) && !!n.compositionDef;

export interface LevelLight {
  id: string;
  light: THREE.Light;
  target?: THREE.Object3D;
  def: LightDef;
}
