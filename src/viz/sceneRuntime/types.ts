import type { Entity } from './Entity';
import type { SceneRuntime } from './SceneRuntime';

/**
 * A behavior is a bag of optional lifecycle callbacks attached to an entity.
 * This is the fundamental unit of per-entity logic in the scene runtime.
 */
export interface Behavior {
  /**
   * Called every physics tick.
   * Return `'remove'` to detach this behavior after the current tick.
   */
  tick?(elapsed: number, entity: Entity): void | 'remove';
  /** Called when the scene resets (e.g. player death / restart). */
  onReset?(): void;
  /** Called when the scene is destroyed. */
  onDestroy?(): void;
}

/**
 * Handle returned when a behavior is attached to an entity.
 * Allows removing the behavior externally.
 */
export interface BehaviorHandle {
  remove(): void;
}

/**
 * A function that creates a Behavior given params and context.
 * This is the type that behavior modules export.
 */
export type BehaviorFn = (params: Record<string, unknown>, entity: Entity, runtime: SceneRuntime) => Behavior;
