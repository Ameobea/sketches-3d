import * as THREE from 'three';

import type { BtQuaternion, BtRigidBody, BtTransform, BtVec3 } from 'src/ammojs/ammoTypes';
import type { Behavior, BehaviorHandle } from './types';

interface BehaviorEntry {
  behavior: Behavior;
  id: number;
  removed: boolean;
}

/**
 * An entity is a scene object with attached behaviors.  It bundles the Three.js
 * object, an optional Bullet rigid body, and a base transform so that behavior
 * logic can work in local (offset) space while the entity lives at an arbitrary
 * position in the scene.
 */
export class Entity {
  public readonly id: string;
  public readonly object: THREE.Object3D;
  public body: BtRigidBody | null;
  /** The original placement transform from the level def. */
  public readonly baseTransform: THREE.Matrix4;

  private behaviors: BehaviorEntry[] = [];
  private nextBehaviorId = 0;

  // Reusable scratch objects to avoid per-tick allocations
  private readonly _pos = new THREE.Vector3();
  private readonly _quat = new THREE.Quaternion();
  private readonly _scale = new THREE.Vector3();

  // Ammo helpers — set by SceneRuntime after physics is ready
  /** @internal */ _btTransform: BtTransform | null = null;
  /** @internal */ _btvec3: ((x: number, y: number, z: number) => BtVec3) | null = null;
  /** @internal */ _btQuat: BtQuaternion | null = null;

  constructor(id: string, object: THREE.Object3D, body: BtRigidBody | null = null) {
    this.id = id;
    this.object = object;
    this.body = body;

    // Capture the initial world-space transform as the base
    this.baseTransform = new THREE.Matrix4();
    object.updateWorldMatrix(true, false);
    this.baseTransform.copy(object.matrixWorld);
  }

  addBehavior(behavior: Behavior): BehaviorHandle {
    const entry: BehaviorEntry = {
      behavior,
      id: this.nextBehaviorId++,
      removed: false,
    };
    this.behaviors.push(entry);
    return {
      remove: () => {
        entry.removed = true;
      },
    };
  }

  removeBehavior(handle: BehaviorHandle): void {
    handle.remove();
  }

  /**
   * Set the entity's world-space transform, updating both the Three.js object
   * and the Bullet rigid body (if present).
   */
  setTransform(matrix: THREE.Matrix4): void {
    matrix.decompose(this._pos, this._quat, this._scale);

    this.object.position.copy(this._pos);
    this.object.quaternion.copy(this._quat);
    this.object.scale.copy(this._scale);

    if (this.body && this._btTransform && this._btvec3) {
      this._btTransform.setOrigin(this._btvec3(this._pos.x, this._pos.y, this._pos.z));
      if (this._btQuat) {
        this._btQuat.setValue(this._quat.x, this._quat.y, this._quat.z, this._quat.w);
        this._btTransform.setRotation(this._btQuat);
      }
      this.body.setWorldTransform(this._btTransform);
    }
  }

  /**
   * Convenience: set position only, preserving current rotation and scale.
   */
  setPosition(x: number, y: number, z: number): void {
    this.object.position.set(x, y, z);

    if (this.body && this._btTransform && this._btvec3) {
      this._btTransform.setOrigin(this._btvec3(x, y, z));
      this.body.setWorldTransform(this._btTransform);
    }
  }

  /** @internal Called by SceneRuntime each physics tick with relative elapsed time. */
  _tickBehaviors(elapsed: number): void {
    let needsSweep = false;
    for (const entry of this.behaviors) {
      if (entry.removed) {
        needsSweep = true;
        continue;
      }
      const result = entry.behavior.tick?.(elapsed, this);
      if (result === 'remove') {
        entry.removed = true;
        needsSweep = true;
      }
    }
    if (needsSweep) {
      this.behaviors = this.behaviors.filter(e => !e.removed);
    }
  }

  /** @internal Called by SceneRuntime on reset. */
  _resetBehaviors(): void {
    for (const entry of this.behaviors) {
      if (!entry.removed) {
        entry.behavior.onReset?.();
      }
    }
  }

  /** @internal Called by SceneRuntime on destroy. */
  _destroyBehaviors(): void {
    for (const entry of this.behaviors) {
      if (!entry.removed) {
        entry.behavior.onDestroy?.();
      }
    }
    this.behaviors = [];
  }

  /** Returns true if this entity has any behaviors attached. */
  hasBehaviors(): boolean {
    return this.behaviors.length > 0;
  }
}
