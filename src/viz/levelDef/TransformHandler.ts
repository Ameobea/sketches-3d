import type * as THREE from 'three';

import { CustomGizmo } from 'src/viz/gizmos/customGizmo';
import type { GizmoTarget } from 'src/viz/gizmos/gizmoTypes';
import { Object3DTarget, PivotTarget } from 'src/viz/gizmos/targets';

import type { LevelSceneNode } from './levelSceneTypes';
import { isEditable } from './levelSceneTypes';
import {
  applySnapshot,
  snapshotTransform,
  snapshotsEqual,
  type ReplayableTransformDelta,
  type TransformSnapshot,
} from '../util/transformSnapshot';

export type TransformMode = 'translate' | 'rotate' | 'scale';

export type { TransformSnapshot, ReplayableTransformDelta };
export {
  applySnapshot,
  snapshotTransform,
  snapshotWorldTransform,
  snapshotsEqual,
  worldToLocalSnapshot,
} from '../util/transformSnapshot';

export interface TransformDragResult {
  entries: Array<{ node: LevelSceneNode; before: TransformSnapshot; after: TransformSnapshot }>;
  /** Replayable delta, if applicable (single non-group selection only). */
  replayable: ReplayableTransformDelta | null;
}

/** Callbacks the TransformHandler invokes during and after drag operations. */
export interface TransformHandlerCallbacks {
  /** Called when the gizmo starts/stops dragging (use to disable/enable orbit). */
  onDraggingChanged(isDragging: boolean): void;
  /** Called when a drag completes with changed transforms. */
  onDragComplete(result: TransformDragResult): void;
  /** Called during drag for live preview (e.g. syncing transform display). */
  onObjectChange(): void;
  /** Drag routing while an EditorMode is active (the mode owns whatever the gizmo is attached to). */
  onModeDragStart(): void;
  onModeDragEnd(): void;
  onModeDrag(): void;
  isModeActive(): boolean;
  /** Called during light drag for live preview. */
  onLightObjectChange(): void;
  /** Called when a light drag completes. */
  onLightDragComplete(): void;
  /** Whether a light is currently selected. */
  isLightSelected(): boolean;
}

/**
 * Wraps `CustomGizmo` with editor-specific routing: mode / light / regular-object
 * drags fan out to different callback channels.
 */
export class TransformHandler {
  readonly gizmo: CustomGizmo;
  private mode: TransformMode = 'translate';
  private space: 'world' | 'local' = 'world';

  private dragStartSnapshots = new Map<LevelSceneNode, TransformSnapshot>();
  private lightIsDragging = false;
  /** Empty when attached to a non-node Object3D (light, CSG sub-mesh). */
  private attachedNodes: LevelSceneNode[] = [];
  private pivotTarget: PivotTarget | null = null;
  /** True while a caller-supplied `GizmoTarget` is attached; drag routing is bypassed. */
  private customTargetActive = false;

  lastReplayableAction: ReplayableTransformDelta | null = null;

  private callbacks: TransformHandlerCallbacks;
  private overlayScene: THREE.Scene;

  constructor(
    camera: THREE.Camera,
    domElement: HTMLCanvasElement,
    overlayScene: THREE.Scene,
    callbacks: TransformHandlerCallbacks
  ) {
    this.callbacks = callbacks;
    this.overlayScene = overlayScene;

    this.gizmo = new CustomGizmo(camera, domElement, {
      onDragStart: () => this.onDragStart(),
      onDragEnd: () => this.onDragEnd(),
      onDrag: () => this.onDrag(),
    });
    this.gizmo.setMode(this.mode);
    this.gizmo.setSpace(this.space);
    overlayScene.add(this.gizmo);
  }

  update() {
    this.gizmo.update();
  }

  dispose() {
    this.overlayScene.remove(this.gizmo);
    this.gizmo.dispose();
  }

  getMode(): TransformMode {
    return this.mode;
  }

  setMode(mode: TransformMode) {
    this.mode = mode;
    this.gizmo.setMode(mode);
  }

  getSpace(): 'world' | 'local' {
    return this.space;
  }

  setSpace(space: 'world' | 'local') {
    this.space = space;
    this.gizmo.setSpace(space);
  }

  toggleSpace() {
    this.setSpace(this.space === 'world' ? 'local' : 'world');
  }

  /** For non-LevelSceneNode targets (lights, CSG sub-meshes). */
  attach(object: THREE.Object3D) {
    this.resetAttachment();
    this.gizmo.setTarget(new Object3DTarget(object));
  }

  /**
   * Attach an arbitrary `GizmoTarget` (gizmo handles, spline points). The target receives
   * preview/commit phases directly via `applyLocalTransform`, so the owner handles
   * write-back/undo itself and the mode/light/object drag routing is bypassed.
   */
  attachTarget(target: GizmoTarget, axisMask?: [boolean, boolean, boolean]) {
    this.resetAttachment();
    this.customTargetActive = true;
    this.gizmo.setAxisMask(axisMask ?? [true, true, true]);
    this.gizmo.setTarget(target);
  }

  detach() {
    this.resetAttachment();
    this.gizmo.setTarget(null);
  }

  private resetAttachment() {
    this.attachedNodes = [];
    this.pivotTarget = null;
    if (this.customTargetActive) {
      this.customTargetActive = false;
      this.gizmo.setAxisMask([true, true, true]);
    }
  }

  /** Single → `Object3DTarget`; multi → `PivotTarget` at the centroid. */
  attachToSelection(nodes: LevelSceneNode[]) {
    this.resetAttachment();
    this.attachedNodes = nodes;

    if (nodes.length === 0) {
      this.gizmo.setTarget(null);
      return;
    }

    if (nodes.length === 1) {
      this.gizmo.setTarget(new Object3DTarget(nodes[0].object));
      return;
    }

    const pivot = new PivotTarget(nodes.map(n => n.object));
    this.pivotTarget = pivot;
    this.gizmo.setTarget(pivot);
  }

  private onDragStart() {
    this.callbacks.onDraggingChanged(true);
    if (this.customTargetActive) return;

    if (this.callbacks.isModeActive()) {
      this.callbacks.onModeDragStart();
      return;
    }

    if (this.callbacks.isLightSelected()) {
      this.lightIsDragging = true;
      return;
    }

    this.dragStartSnapshots.clear();
    for (const node of this.attachedNodes) {
      this.dragStartSnapshots.set(node, snapshotTransform(node.object));
    }
  }

  private onDrag() {
    if (this.customTargetActive) return;
    if (this.callbacks.isModeActive()) {
      this.callbacks.onModeDrag();
    } else if (this.callbacks.isLightSelected()) {
      this.callbacks.onLightObjectChange();
    } else {
      this.callbacks.onObjectChange();
    }
  }

  private onDragEnd() {
    this.callbacks.onDraggingChanged(false);
    if (this.customTargetActive) return;

    if (this.callbacks.isModeActive()) {
      this.callbacks.onModeDragEnd();
      return;
    }

    if (this.callbacks.isLightSelected() && this.lightIsDragging) {
      this.lightIsDragging = false;
      this.callbacks.onLightDragComplete();
      return;
    }

    if (this.dragStartSnapshots.size === 0) return;

    const entries: TransformDragResult['entries'] = [];
    for (const [node, before] of this.dragStartSnapshots) {
      const after = snapshotTransform(node.object);
      if (!snapshotsEqual(before, after)) {
        entries.push({ node, before, after });
      }
    }
    this.dragStartSnapshots.clear();

    if (entries.length === 0) return;

    let replayable: ReplayableTransformDelta | null = null;
    if (entries.length === 1) {
      const { before, after } = entries[0];
      replayable = {
        positionDelta: [
          after.position[0] - before.position[0],
          after.position[1] - before.position[1],
          after.position[2] - before.position[2],
        ],
        rotationDelta: [
          after.rotation[0] - before.rotation[0],
          after.rotation[1] - before.rotation[1],
          after.rotation[2] - before.rotation[2],
        ],
        scaleFactor: [
          before.scale[0] !== 0 ? after.scale[0] / before.scale[0] : 1,
          before.scale[1] !== 0 ? after.scale[1] / before.scale[1] : 1,
          before.scale[2] !== 0 ? after.scale[2] / before.scale[2] : 1,
        ],
      };
    }

    if (replayable) {
      this.lastReplayableAction = replayable;
    }

    // Reset pivot baselines so the next drag's deltas are measured from here.
    this.pivotTarget?.rebaseline();

    this.callbacks.onDragComplete({ entries, replayable });
  }

  /** Shift+R: re-applies the last delta to `node`.  Returns null when there's nothing to replay. */
  replayLastAction(node: LevelSceneNode): { before: TransformSnapshot; after: TransformSnapshot } | null {
    if (!this.lastReplayableAction || !isEditable(node)) return null;

    const delta = this.lastReplayableAction;
    const obj = node.object;
    const before = snapshotTransform(obj);

    const after: TransformSnapshot = {
      position: [
        before.position[0] + delta.positionDelta[0],
        before.position[1] + delta.positionDelta[1],
        before.position[2] + delta.positionDelta[2],
      ],
      rotation: [
        before.rotation[0] + delta.rotationDelta[0],
        before.rotation[1] + delta.rotationDelta[1],
        before.rotation[2] + delta.rotationDelta[2],
      ],
      scale: [
        before.scale[0] * delta.scaleFactor[0],
        before.scale[1] * delta.scaleFactor[1],
        before.scale[2] * delta.scaleFactor[2],
      ],
    };

    applySnapshot(obj, after);
    return { before, after };
  }
}
