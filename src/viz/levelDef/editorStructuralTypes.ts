import type { Viz } from 'src/viz';
import type { LevelObject, LevelSceneNode } from './levelSceneTypes';
import type { TransformSnapshot } from './TransformHandler';

/**
 * Narrow context passed to structural mutation operations.
 * LevelEditor satisfies this interface structurally.
 */
export interface StructuralCtx {
  viz: Viz;
  rootNodes: LevelSceneNode[];
  nodeById: Map<string, LevelSceneNode>;
  allLevelObjects: LevelObject[];

  registerMeshes(levelObj: LevelObject): void;
  unregisterMeshes(levelObj: LevelObject): void;
  syncPhysics(levelObj: LevelObject): void;
  removePhysics(levelObj: LevelObject): void;
}

/** Identifies where in the logical tree a node lives. */
export type ParentRef = { type: 'root' } | { type: 'group'; groupId: string };

/** A node's position within its parent's children (or rootNodes). */
export interface Placement {
  parent: ParentRef;
  index: number;
}

/**
 * A detached (or not-yet-attached) subtree, bundled with the placement it
 * was captured from. Sufficient to re-attach the subtree at the same position.
 */
export interface RuntimeSubtree {
  root: LevelSceneNode;
  placement: Placement;
  /** Root-local transform captured for this specific placement. */
  transform: TransformSnapshot;
  /** All leaf LevelObjects in the subtree (pre-collected for O(1) registration). */
  leaves: LevelObject[];
}

export type StructuralOp =
  | { type: 'attach_subtree'; subtree: RuntimeSubtree }
  | { type: 'detach_subtree'; subtree: RuntimeSubtree };
