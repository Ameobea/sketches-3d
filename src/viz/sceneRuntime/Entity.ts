import * as THREE from 'three';

import type { BtQuaternion, BtRigidBody, BtTransform, BtVec3 } from 'src/ammojs/ammoTypes';
import type { Viz } from 'src/viz';
import type { MaterialClass } from '../shaders/customShader.types';
import type { Behavior, BehaviorHandle } from './types';

interface BehaviorEntry {
  behavior: Behavior;
  id: number;
  removed: boolean;
}

let nextNumericId = 1;

/**
 * An entity is a scene object with attached state (a single physics body,
 * material class, optional behaviors).  It is the unified owner of
 * per-collidable metadata in the runtime: landing SFX, camera-barrier flags,
 * custom surface triggers, and behavior lifecycle all live here.
 *
 * **Invariant:** one entity = one leaf mesh = at most one rigid body.
 *
 * Entity creation is decoupled from physics readiness: an Entity can exist
 * before the collision world is up.  The body is attached later via
 * {@link _setBody} (called from `addTriMesh` / `addCollisionObject`), at
 * which point `numericId` gets reflected into the Ammo user index so the
 * player-land path can map back.
 */
export class Entity {
  public readonly id: string;
  /**
   * Per-process unique integer.  Stored in the Ammo `btRigidBody` user index
   * so that collision events (player landing on a surface) can map the contact
   * back to the owning Entity.
   */
  public readonly numericId: number;
  public object: THREE.Mesh;
  public body: BtRigidBody | null = null;
  /** The original placement transform from the level def. */
  public readonly baseTransform: THREE.Matrix4;
  public materialClass: MaterialClass | undefined = undefined;
  /**
   * Object-level override for the camera "non-permeable" barrier flag.  `undefined`
   * means "defer to the assigned material's `userData.nonPermeable`".  An explicit
   * `true`/`false` on the entity wins over the material default.
   */
  public nonPermeable: boolean | undefined = undefined;

  private viz: Viz;
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

  constructor(viz: Viz, id: string, object: THREE.Mesh) {
    this.viz = viz;
    this.id = id;
    this.numericId = nextNumericId++;
    this.object = object;

    // Capture the initial world-space transform as the base
    this.baseTransform = new THREE.Matrix4();
    object.updateWorldMatrix(true, false);
    this.baseTransform.copy(object.matrixWorld);
  }

  /** @internal Called by BulletPhysics when a body is registered for this entity. */
  _setBody(body: BtRigidBody): void {
    if (this.body !== null) {
      throw new Error(`Entity "${this.id}": _setBody called but entity already has a body`);
    }
    this.body = body;
  }

  /** @internal Called by BulletPhysics when the body is removed from this entity. */
  _clearBody(): void {
    if (this.body === null) {
      throw new Error(`Entity "${this.id}": _clearBody called but entity has no body`);
    }
    this.body = null;
  }

  /**
   * Set the entity's material class.  Safe to call multiple times; the SFX
   * lazy-load is announced exactly once per entity.  A caller may upgrade
   * `undefined` → a real class (for levelDef materials that finish loading
   * after physics registration), but cannot change an already-set class.
   */
  setMaterialClass(mc: MaterialClass): void {
    if (this.materialClass !== undefined) {
      return;
    }
    this.materialClass = mc;
    this.viz.sfxManager.onMaterialClassPresent(mc);
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
   * and the attached Bullet rigid body (if any).
   */
  setTransform(matrix: THREE.Matrix4): void {
    matrix.decompose(this._pos, this._quat, this._scale);

    this.object.position.copy(this._pos);
    this.object.quaternion.copy(this._quat);
    this.object.scale.copy(this._scale);

    if (!this.body || !this._btTransform || !this._btvec3) {
      return;
    }
    this._btTransform.setOrigin(this._btvec3(this._pos.x, this._pos.y, this._pos.z));
    if (this._btQuat) {
      this._btQuat.setValue(this._quat.x, this._quat.y, this._quat.z, this._quat.w);
      this._btTransform.setRotation(this._btQuat);
    }
    this.body.setWorldTransform(this._btTransform);
  }

  /**
   * Convenience: set position only, preserving current rotation and scale.
   */
  setPosition(x: number, y: number, z: number): void {
    this.object.position.set(x, y, z);

    if (!this.body || !this._btTransform || !this._btvec3) {
      return;
    }
    this._btTransform.setOrigin(this._btvec3(x, y, z));
    this.body.setWorldTransform(this._btTransform);
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
