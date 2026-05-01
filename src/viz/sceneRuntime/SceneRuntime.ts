import * as THREE from 'three';

import type { Viz } from 'src/viz';
import { getAmmoJS, type PhysicsTickerHandle } from '../collision';
import { clearPhysicsBinding } from '../util/physics';
import { withWorldSpaceTransform } from '../util/three';
import { Entity } from './Entity';
import type { Behavior, BehaviorFn } from './types';

type PhysicsContext = NonNullable<Viz['fpCtx']>;

export interface SpawnerConfig {
  /** Seconds between spawns (relative). */
  interval: number;
  /** Seconds before the first spawn (relative to current time). Default: 0 */
  initialDelay?: number;
  /** Behaviors to attach to each spawned clone. */
  behaviors: { fn: BehaviorFn; params: Record<string, unknown> }[];
}

interface SpawnerState {
  templateId: string;
  templateObj: THREE.Object3D;
  config: SpawnerConfig;
  clones: Entity[];
  nextSpawnTime: number;
  cloneCounter: number;
}

/**
 * Manages dynamic entities and their behaviors, physics ticker lifecycle,
 * and periodic scheduling.  This is the low-level "entity engine" that
 * ParkourManager and the level def loader build on top of.
 */
export class SceneRuntime {
  public readonly viz: Viz;

  private entities: Entity[] = [];
  private entityById = new Map<string, Entity>();
  private entityEpochs = new Map<Entity, number>();
  private pendingRemovals: Entity[] = [];
  private isTicking = false;
  private managerTickerHandle: PhysicsTickerHandle | null = null;
  private fpCtxRef: PhysicsContext | null = null;
  private spawners: SpawnerState[] = [];
  private wasBehaviorsPaused = false;

  /** Additional callbacks invoked on reset, beyond entity behavior resets. */
  private resetCbs: (() => void)[] = [];
  /** Additional callbacks invoked on destroy, beyond entity behavior destruction. */
  private destroyCbs: (() => void)[] = [];

  constructor(viz: Viz) {
    this.viz = viz;

    // Pre-load physics engine
    getAmmoJS();

    viz.collisionWorldLoadedCbs.push(fpCtx => {
      this.fpCtxRef = fpCtx;
      this.initializeEntityPhysics(fpCtx);

      this.managerTickerHandle = fpCtx.registerPhysicsTicker({
        tick: (physicsTime: number) => {
          if (this.viz.behaviorsPaused) {
            // First tick of a paused span: snap every behavior-driven entity
            // back to its authored rest pose so the editor sees authored state,
            // not whatever frame the animation happened to be on.
            if (!this.wasBehaviorsPaused) {
              for (const entity of this.entities) {
                if (entity.hasBehaviors()) {
                  entity.setTransform(entity.baseTransform);
                }
              }
              this.wasBehaviorsPaused = true;
            }
            // Advance the per-entity epoch so that when behaviors resume,
            // `elapsed` doesn't jump by the pause duration.
            for (const entity of this.entities) {
              this.entityEpochs.set(entity, physicsTime);
            }
            for (const state of this.spawners) {
              state.nextSpawnTime = physicsTime + (state.config.initialDelay ?? 0);
            }
            this.flushPendingRemovals();
            return;
          }
          this.wasBehaviorsPaused = false;
          this.tickSpawners(physicsTime);
          this.tickEntities(physicsTime);
          this.flushPendingRemovals();
        },
      });
    });

    viz.registerDestroyedCb(() => this.destroy());
  }

  /** The physics context, available after the collision world is loaded. */
  get fpCtx(): PhysicsContext | null {
    return this.fpCtxRef;
  }

  // --- Entity management ---

  /**
   * Create a new Entity and attach it to this runtime for behavior ticking and
   * lifecycle management.
   */
  createEntity(id: string, object: THREE.Mesh): Entity {
    const entity = new Entity(this.viz, id, object);
    this.attachEntity(entity);
    return entity;
  }

  /**
   * Take ownership of an entity that was created outside this runtime (e.g. by
   * {@link BulletPhysics} during level-def physics registration) so its behaviors
   * tick and its lifecycle callbacks fire.
   */
  adoptEntity(entity: Entity): Entity {
    if (this.entityById.has(entity.id)) {
      return entity;
    }
    this.attachEntity(entity);
    return entity;
  }

  private attachEntity(entity: Entity): void {
    this.entityEpochs.set(entity, this.fpCtxRef?.getPhysicsTime() ?? 0);
    this.entities.push(entity);
    this.entityById.set(entity.id, entity);

    if (this.fpCtxRef) {
      this.initEntityPhysicsHelpers(entity, this.fpCtxRef);
    }
  }

  removeEntity(entity: Entity): void {
    if (this.isTicking) {
      // Defer removal to avoid corrupting the tick iteration
      this.pendingRemovals.push(entity);
      return;
    }
    this.removeEntityImmediate(entity);
  }

  private removeEntityImmediate(entity: Entity): void {
    entity._destroyBehaviors();

    // Clean up Ammo resources
    if (entity._btTransform && this.fpCtxRef) {
      this.fpCtxRef.Ammo.destroy(entity._btTransform);
      entity._btTransform = null;
      if (entity._btQuat) {
        this.fpCtxRef.Ammo.destroy(entity._btQuat);
        entity._btQuat = null;
      }
    }

    // Remove from scene
    entity.object.parent?.remove(entity.object);

    // Remove the entity's physics body if it has one.
    if (this.fpCtxRef && entity.body) {
      this.fpCtxRef.removeCollisionObject(entity.body);
    }

    const ix = this.entities.indexOf(entity);
    if (ix !== -1) {
      this.entities[ix] = this.entities[this.entities.length - 1];
      this.entities.pop();
    }
    this.entityById.delete(entity.id);
    this.entityEpochs.delete(entity);
  }

  private flushPendingRemovals(): void {
    while (this.pendingRemovals.length > 0) {
      const entity = this.pendingRemovals.pop()!;
      // Guard against double-removal (entity may already have been removed by reset/destroy)
      if (this.entityById.has(entity.id)) {
        this.removeEntityImmediate(entity);
      }
    }
  }

  getEntity(id: string): Entity | undefined {
    return this.entityById.get(id);
  }

  // --- Spawner management ---

  /**
   * Register a spawner: the template object is hidden and periodically cloned.
   * Each clone gets the specified behaviors attached.  SceneRuntime owns the
   * full clone lifecycle: physics registration, behavior wiring, cleanup on
   * despawn, and reset.
   */
  registerSpawner(templateId: string, templateObj: THREE.Object3D, config: SpawnerConfig): void {
    const fpCtx = this.fpCtxRef;
    if (!fpCtx) {
      throw new Error('SceneRuntime: physics not ready when registering spawner');
    }

    if (!(templateObj instanceof THREE.Mesh)) {
      throw new Error(`registerSpawner "${templateId}": template must be a Mesh, got ${templateObj.type}`);
    }

    // Hide the template and remove its physics so clones start fresh.
    templateObj.visible = false;
    clearPhysicsBinding(templateObj, fpCtx);

    const currentTime = fpCtx.getPhysicsTime();
    const state: SpawnerState = {
      templateId,
      templateObj,
      config,
      clones: [],
      nextSpawnTime: currentTime + (config.initialDelay ?? 0),
      cloneCounter: 0,
    };
    this.spawners.push(state);
  }

  private tickSpawners(physicsTime: number): void {
    for (const state of this.spawners) {
      while (state.nextSpawnTime <= physicsTime) {
        this.spawnClone(state);
        state.nextSpawnTime += state.config.interval;
      }
    }
  }

  private spawnClone(state: SpawnerState): void {
    const fpCtx = this.fpCtxRef!;

    const clone = state.templateObj.clone() as THREE.Mesh;
    clone.visible = true;
    const parent = state.templateObj.parent ?? this.viz.scene;
    parent.add(clone);

    const cloneEntity = this.createEntity(`${state.templateId}__clone_${state.cloneCounter++}`, clone);
    withWorldSpaceTransform(clone, mesh => fpCtx.addTriMesh(mesh, 'kinematic', cloneEntity));

    state.clones.push(cloneEntity);

    // Attach behaviors; wrap them to handle despawn (tick returning 'remove')
    for (const { fn, params } of state.config.behaviors) {
      const behavior = fn(params, cloneEntity, this);
      cloneEntity.addBehavior(this.wrapSpawnedBehavior(behavior, cloneEntity, state));
    }
  }

  /**
   * Wraps a behavior so that when it returns 'remove', the clone entity
   * is fully removed from the runtime.
   */
  private wrapSpawnedBehavior(inner: Behavior, cloneEntity: Entity, state: SpawnerState): Behavior {
    return {
      tick: (elapsed, entity) => {
        const result = inner.tick?.(elapsed, entity);
        if (result === 'remove') {
          this.removeEntity(cloneEntity);
          const ix = state.clones.indexOf(cloneEntity);
          if (ix !== -1) state.clones.splice(ix, 1);
          return 'remove';
        }
      },
      onReset: inner.onReset?.bind(inner),
      onDestroy: inner.onDestroy?.bind(inner),
    };
  }

  private removeAllSpawnerClones(state: SpawnerState): void {
    for (const clone of state.clones) {
      if (this.entityById.has(clone.id)) {
        this.removeEntityImmediate(clone);
      }
    }
    state.clones.length = 0;
  }

  // --- Physics ticker management (for non-entity tickers) ---

  // --- Lifecycle ---

  registerResetCb(cb: () => void): void {
    this.resetCbs.push(cb);
  }

  registerDestroyCb(cb: () => void): void {
    this.destroyCbs.push(cb);
  }

  reset(): void {
    // Remove all spawner clones and reset spawn timing
    const currentTime = this.fpCtxRef?.getPhysicsTime() ?? 0;
    for (const state of this.spawners) {
      this.removeAllSpawnerClones(state);
      state.nextSpawnTime = currentTime + (state.config.initialDelay ?? 0);
      state.cloneCounter = 0;
    }

    // Reset all (non-clone) entity behaviors and their epochs
    for (const entity of this.entities) {
      this.entityEpochs.set(entity, currentTime);
      entity._resetBehaviors();
    }

    // Run additional reset callbacks
    for (const cb of this.resetCbs) {
      cb();
    }
  }

  destroy(): void {
    this.managerTickerHandle?.unregister();
    this.managerTickerHandle = null;

    // Remove all spawner clones
    for (const state of this.spawners) {
      this.removeAllSpawnerClones(state);
    }
    this.spawners = [];

    // Destroy all entity behaviors and clean up Ammo resources
    for (const entity of this.entities) {
      entity._destroyBehaviors();
      if (entity._btTransform && this.fpCtxRef) {
        this.fpCtxRef.Ammo.destroy(entity._btTransform);
        if (entity._btQuat) {
          this.fpCtxRef.Ammo.destroy(entity._btQuat);
        }
      }
    }

    for (const cb of this.destroyCbs) {
      cb();
    }

    this.entities = [];
    this.entityById.clear();
    this.entityEpochs.clear();
    this.resetCbs = [];
    this.destroyCbs = [];
  }

  // --- Internal ---

  private tickEntities(physicsTime: number): void {
    this.isTicking = true;
    for (const entity of this.entities) {
      const epoch = this.entityEpochs.get(entity) ?? 0;
      entity._tickBehaviors(physicsTime - epoch);
    }
    this.isTicking = false;
  }

  /** Initialize Ammo helpers for all entities that don't have them yet. */
  private initializeEntityPhysics(fpCtx: PhysicsContext): void {
    for (const entity of this.entities) {
      if (!entity._btTransform) {
        this.initEntityPhysicsHelpers(entity, fpCtx);
      }
    }
  }

  /** Set up the Ammo btTransform and helpers on a single entity. */
  private initEntityPhysicsHelpers(entity: Entity, fpCtx: PhysicsContext): void {
    if (entity._btTransform) {
      return;
    }
    const tfn = new fpCtx.Ammo.btTransform();
    tfn.setIdentity();
    entity._btTransform = tfn;
    entity._btvec3 = fpCtx.btvec3;
    entity._btQuat = new fpCtx.Ammo.btQuaternion(0, 0, 0, 1);
  }
}
