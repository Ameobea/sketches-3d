import type * as THREE from 'three';

import type { Transform3, TreeDef } from 'src/geoscript/geotoyAPIClient';
import { CustomGizmo } from 'src/viz/gizmos/customGizmo';
import { InstanceTarget } from 'src/viz/gizmos/targets';
import { type GizmoTargetRef, gizmoTargetRefsEqual } from 'src/viz/gizmos/gizmoTypes';

export type GizmoMode = 'translate' | 'rotate' | 'scale';
export type GizmoSpace = 'world' | 'local';

export interface TransformGizmoCallbacks {
  onDraggingChanged(isDragging: boolean): void;
  /** Fires once at the start of a drag with the ref being edited. */
  onDragStart?(ref: GizmoTargetRef): void;
  /** Fires every frame during a drag (preview) and once on commit. */
  onTransformChange(ref: GizmoTargetRef, transform: Transform3): void;
  onDragEnd(ref: GizmoTargetRef): void;
}

/** Geotoy adapter around `CustomGizmo`; caller must `update()` per frame. */
export class TransformGizmo {
  private readonly gizmo: CustomGizmo;
  private readonly overlay: THREE.Scene;
  private readonly getTree: () => TreeDef;
  private readonly callbacks: TransformGizmoCallbacks;
  private attachedRef: GizmoTargetRef | null = null;

  constructor(
    camera: THREE.Camera,
    domElement: HTMLCanvasElement,
    overlayScene: THREE.Scene,
    getTree: () => TreeDef,
    callbacks: TransformGizmoCallbacks
  ) {
    this.overlay = overlayScene;
    this.getTree = getTree;
    this.callbacks = callbacks;
    this.gizmo = new CustomGizmo(camera, domElement, {
      onDragStart: () => {
        callbacks.onDraggingChanged(true);
        if (this.attachedRef) callbacks.onDragStart?.(this.attachedRef);
      },
      onDragEnd: () => {
        callbacks.onDraggingChanged(false);
        if (this.attachedRef) callbacks.onDragEnd(this.attachedRef);
      },
    });
    overlayScene.add(this.gizmo);
  }

  update() {
    this.gizmo.update();
  }

  dispose() {
    this.overlay.remove(this.gizmo);
    this.gizmo.dispose();
  }

  getMode(): GizmoMode {
    return this.gizmo.getMode();
  }
  setMode(mode: GizmoMode) {
    this.gizmo.setMode(mode);
  }

  getSpace(): GizmoSpace {
    return this.gizmo.getSpace();
  }
  setSpace(space: GizmoSpace) {
    this.gizmo.setSpace(space);
  }
  toggleSpace() {
    this.gizmo.setSpace(this.gizmo.getSpace() === 'world' ? 'local' : 'world');
  }

  dragging(): boolean {
    return this.gizmo.isDragging();
  }

  /** `null` / root / unknown node all detach. `handle` refs are unused until M3. */
  syncTo(ref: GizmoTargetRef | null, tree: TreeDef): void {
    if (this.gizmo.isDragging()) return; // don't yank mid-drag

    const node = ref && ref.kind === 'instance' ? tree.nodes[ref.nodeId] : undefined;
    const detached =
      ref === null ||
      ref.kind !== 'instance' ||
      ref.nodeId === tree.rootId ||
      !node ||
      ref.index < 0 ||
      ref.index >= node.instances.length;
    if (detached) {
      if (this.attachedRef !== null) {
        this.gizmo.setTarget(null);
        this.attachedRef = null;
      }
      return;
    }

    if (gizmoTargetRefsEqual(this.attachedRef, ref)) return;
    this.attachedRef = ref;
    this.gizmo.setTarget(
      new InstanceTarget(ref.nodeId, ref.index, this.getTree, {
        onChange: (_phase, nodeId, index, transform) =>
          this.callbacks.onTransformChange({ kind: 'instance', nodeId, index }, transform),
      })
    );
  }
}
