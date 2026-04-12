import * as THREE from 'three';
import { TransformControls } from 'three/examples/jsm/controls/TransformControls.js';

import type { LevelSceneNode } from './levelSceneTypes';
import { isLevelGroup } from './levelSceneTypes';

export type TransformMode = 'translate' | 'rotate' | 'scale';

export interface TransformSnapshot {
  position: [number, number, number];
  rotation: [number, number, number];
  scale: [number, number, number];
}

// --- Standalone snapshot helpers (usable without a TransformHandler instance) ---

const SNAP_EPS = 1e-6;

export const snapshotTransform = (obj: THREE.Object3D): TransformSnapshot => {
  const r = obj.rotation;
  return {
    position: obj.position.toArray() as [number, number, number],
    rotation: [r.x, r.y, r.z],
    scale: obj.scale.toArray() as [number, number, number],
  };
};

export const applySnapshot = (obj: THREE.Object3D, snap: TransformSnapshot) => {
  obj.position.fromArray(snap.position);
  obj.rotation.set(snap.rotation[0], snap.rotation[1], snap.rotation[2]);
  obj.scale.fromArray(snap.scale);
};

export const snapshotsEqual = (a: TransformSnapshot, b: TransformSnapshot): boolean => {
  for (let i = 0; i < 3; i++) {
    if (Math.abs(a.position[i] - b.position[i]) > SNAP_EPS) return false;
    if (Math.abs(a.rotation[i] - b.rotation[i]) > SNAP_EPS) return false;
    if (Math.abs(a.scale[i] - b.scale[i]) > SNAP_EPS) return false;
  }
  return true;
};

/**
 * Captures the relative change of a transform operation so it can be replayed
 * on a different object via Shift+R (similar to Blender's "repeat last").
 */
export interface ReplayableTransformDelta {
  positionDelta: [number, number, number];
  rotationDelta: [number, number, number];
  scaleFactor: [number, number, number];
}

export interface TransformDragResult {
  entries: Array<{ node: LevelSceneNode; before: TransformSnapshot; after: TransformSnapshot }>;
  /** Replayable delta, if applicable (single non-group selection only). */
  replayable: ReplayableTransformDelta | null;
}

/** Callbacks the TransformHandler invokes during and after drag operations. */
export interface TransformHandlerCallbacks {
  /** Called when TransformControls starts/stops dragging (to disable/enable orbit). */
  onDraggingChanged(isDragging: boolean): void;
  /** Called when a drag completes with changed transforms. */
  onDragComplete(result: TransformDragResult): void;
  /** Called during drag for live preview (e.g. syncing transform display). */
  onObjectChange(): void;
  /** Called when CSG controller should handle drag start. */
  onCsgDragStart(): void;
  /** Called when CSG controller should handle drag end. */
  onCsgDragEnd(): void;
  /** Called when CSG controller should handle object change. */
  onCsgObjectChange(): void;
  /** Called during light drag for live preview. */
  onLightObjectChange(): void;
  /** Called when a light drag completes. */
  onLightDragComplete(): void;
  /** Whether CSG mode is active. */
  isCsgActive(): boolean;
  /** Whether a light is currently selected. */
  isLightSelected(): boolean;
}

/**
 * Manages TransformControls, drag snapshots, and transform mode/space.
 *
 * Created when the editor enters edit mode, disposed when it exits.
 * The owner (LevelEditor) provides callbacks for side-effects like
 * undo push, physics sync, and API save.
 */
export class TransformHandler {
  readonly controls: TransformControls;
  private mode: TransformMode = 'translate';
  private space: 'world' | 'local' = 'world';

  /** Snapshots captured at drag start for the currently selected nodes. */
  private dragStartSnapshots = new Map<LevelSceneNode, TransformSnapshot>();
  /** True while a light is being dragged. */
  private lightIsDragging = false;

  lastReplayableAction: ReplayableTransformDelta | null = null;

  private callbacks: TransformHandlerCallbacks;

  /** Reference to nodes being transformed (set by attachToSelection). */
  private attachedNodes: LevelSceneNode[] = [];

  /** Pivot used for multi-select transforms. */
  private pivot: THREE.Object3D | null = null;
  private pivotStartPosition = new THREE.Vector3();
  private pivotStartScale = new THREE.Vector3();

  constructor(
    camera: THREE.Camera,
    domElement: HTMLCanvasElement,
    overlayScene: THREE.Scene,
    callbacks: TransformHandlerCallbacks
  ) {
    this.callbacks = callbacks;

    this.controls = new TransformControls(camera, domElement);
    this.controls.setMode(this.mode);
    this.controls.setSpace(this.space);

    this.controls.addEventListener('dragging-changed', (e: any) => {
      callbacks.onDraggingChanged(e.value);

      if (callbacks.isCsgActive()) {
        if (e.value) callbacks.onCsgDragStart();
        else callbacks.onCsgDragEnd();
        return;
      }

      if (e.value) {
        this.onDragStart();
      } else {
        this.onDragEnd();
      }
    });

    this.controls.addEventListener('objectChange', () => {
      if (callbacks.isCsgActive()) {
        callbacks.onCsgObjectChange();
      } else if (callbacks.isLightSelected()) {
        callbacks.onLightObjectChange();
      } else {
        this.onObjectChange();
        callbacks.onObjectChange();
      }
    });

    overlayScene.add(this.controls);
  }

  dispose(overlayScene: THREE.Scene) {
    overlayScene.remove(this.controls);
    this.controls.dispose();
    if (this.pivot) {
      this.pivot.removeFromParent();
      this.pivot = null;
    }
  }

  getMode(): TransformMode {
    return this.mode;
  }

  setMode(mode: TransformMode) {
    this.mode = mode;
    this.controls.setMode(mode);
  }

  getSpace(): 'world' | 'local' {
    return this.space;
  }

  setSpace(space: 'world' | 'local') {
    this.space = space;
    this.controls.setSpace(space);
  }

  toggleSpace() {
    this.setSpace(this.space === 'world' ? 'local' : 'world');
  }

  attach(object: THREE.Object3D) {
    this.controls.attach(object);
  }

  detach() {
    this.controls.detach();
  }

  /**
   * Attach the transform gizmo to the given selection.
   * For a single node, attaches directly. For multiple nodes,
   * creates a pivot at their centroid.
   */
  attachToSelection(nodes: LevelSceneNode[]) {
    this.attachedNodes = nodes;

    // Clean up any previous pivot
    if (this.pivot) {
      this.pivot.removeFromParent();
      this.pivot = null;
    }

    if (nodes.length === 0) {
      this.controls.detach();
      return;
    }

    if (nodes.length === 1) {
      this.controls.attach(nodes[0].object);
      return;
    }

    // Multi-select: create pivot at centroid
    this.pivot = new THREE.Object3D();
    this.pivot.name = '__multiSelectPivot';
    const centroid = this.computeCentroid(nodes);
    this.pivot.position.copy(centroid);
    // Add to the scene so TransformControls can attach to it
    const parent = nodes[0].object.parent;
    if (parent) parent.add(this.pivot);
    this.controls.attach(this.pivot);
  }

  private computeCentroid(nodes: LevelSceneNode[]): THREE.Vector3 {
    const center = new THREE.Vector3();
    for (const node of nodes) {
      center.add(node.object.getWorldPosition(new THREE.Vector3()));
    }
    center.divideScalar(nodes.length);
    return center;
  }

  // --- Snapshot helpers (delegate to standalone functions) ---

  snapshotTransform = snapshotTransform;
  applySnapshot = applySnapshot;
  snapshotsEqual = snapshotsEqual;

  // --- Drag handling ---

  private onDragStart() {
    if (this.callbacks.isLightSelected()) {
      this.lightIsDragging = true;
      return;
    }

    // Snapshot all attached nodes
    this.dragStartSnapshots.clear();
    for (const node of this.attachedNodes) {
      this.dragStartSnapshots.set(node, this.snapshotTransform(node.object));
    }

    // For multi-select, also snapshot the pivot
    if (this.pivot) {
      this.pivotStartPosition.copy(this.pivot.position);
      this.pivotStartScale.copy(this.pivot.scale);
    }
  }

  private onDragEnd() {
    if (this.callbacks.isLightSelected() && this.lightIsDragging) {
      this.lightIsDragging = false;
      this.callbacks.onLightDragComplete();
      return;
    }

    if (this.dragStartSnapshots.size === 0) return;

    const entries: TransformDragResult['entries'] = [];
    for (const [node, before] of this.dragStartSnapshots) {
      const after = this.snapshotTransform(node.object);
      if (!this.snapshotsEqual(before, after)) {
        entries.push({ node, before, after });
      }
    }
    this.dragStartSnapshots.clear();

    if (entries.length === 0) return;

    // Compute replayable delta for single non-group selection
    let replayable: ReplayableTransformDelta | null = null;
    if (entries.length === 1 && !isLevelGroup(entries[0].node)) {
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

    this.callbacks.onDragComplete({ entries, replayable });
  }

  /**
   * Called during drag for live preview of multi-select transforms.
   * For single-select, Three.js handles the object transform directly.
   * For multi-select, we compute the delta from the pivot and apply it
   * to each selected node.
   */
  private onObjectChange() {
    if (!this.pivot || this.attachedNodes.length <= 1) return;

    const pivotDelta = new THREE.Vector3().subVectors(this.pivot.position, this.pivotStartPosition);
    const pivotScaleRatio = new THREE.Vector3(
      this.pivotStartScale.x !== 0 ? this.pivot.scale.x / this.pivotStartScale.x : 1,
      this.pivotStartScale.y !== 0 ? this.pivot.scale.y / this.pivotStartScale.y : 1,
      this.pivotStartScale.z !== 0 ? this.pivot.scale.z / this.pivotStartScale.z : 1
    );

    for (const node of this.attachedNodes) {
      const startSnap = this.dragStartSnapshots.get(node);
      if (!startSnap) continue;

      // Translation: add pivot delta to each node's start position
      if (this.mode === 'translate') {
        node.object.position.set(
          startSnap.position[0] + pivotDelta.x,
          startSnap.position[1] + pivotDelta.y,
          startSnap.position[2] + pivotDelta.z
        );
      }

      // Scale: multiply each node's start scale by pivot scale ratio
      if (this.mode === 'scale') {
        node.object.scale.set(
          startSnap.scale[0] * pivotScaleRatio.x,
          startSnap.scale[1] * pivotScaleRatio.y,
          startSnap.scale[2] * pivotScaleRatio.z
        );
      }
    }
  }

  /**
   * Replay the last transform action on the given node (Shift+R).
   * Returns the before/after snapshots if the replay was applied, null otherwise.
   */
  replayLastAction(node: LevelSceneNode): { before: TransformSnapshot; after: TransformSnapshot } | null {
    if (!this.lastReplayableAction || node.generated) return null;

    const delta = this.lastReplayableAction;
    const obj = node.object;
    const before = this.snapshotTransform(obj);

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

    this.applySnapshot(obj, after);
    return { before, after };
  }
}
