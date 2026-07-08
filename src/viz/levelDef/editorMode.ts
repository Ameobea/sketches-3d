import type * as THREE from 'three';

import type { LevelSceneNode } from './levelSceneTypes';

/**
 * A modal editing surface that takes over parts of the main editor's input dispatch while
 * active (CSG tree editing, spline editing). At most one is active at a time; a mode sets
 * `editor.activeMode` on enter and clears it in `exit()`.
 */
export interface EditorMode {
  /** First crack at edit-mode keybinds (not consulted while typing in an input). True = consumed. */
  onKeyDown(e: KeyboardEvent): boolean;
  /** First crack at scene clicks. True = consumed. */
  interceptClick(raycaster: THREE.Raycaster, event: PointerEvent): boolean;
  /** Camera focus target for the '.' bind while active. */
  getFocusTarget(): THREE.Object3D | null;
  /** A node is about to be single-selected; the mode may exit itself. */
  onSelectNode?(node: LevelSceneNode): void;
  /** `def.inputs` changed externally (undo/redo); refresh any mode-owned views of it. */
  onInputsChanged?(node: LevelSceneNode): void;
  /** Suppress the main selection material-swap highlight while active. */
  readonly suppressSelectionHighlights?: boolean;
  /**
   * Drag routing from the shared TransformHandler for targets attached via `attach*`.
   * Targets attached via `attachTarget` bypass these — they receive preview/commit phases
   * directly through `GizmoTarget.applyLocalTransform`.
   */
  onDragStart?(): void;
  onDrag?(): void;
  onDragEnd?(): void;
  /** Idempotent. Must clear `editor.activeMode` when it points at this mode. */
  exit(): void;
}
