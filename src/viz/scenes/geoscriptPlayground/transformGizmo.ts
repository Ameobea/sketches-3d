import type * as THREE from 'three';

import type { Transform3, TreeDef } from 'src/geoscript/geotoyAPIClient';
import { CustomGizmo } from 'src/viz/gizmos/customGizmo';
import { TreeNodeTarget } from 'src/viz/gizmos/targets';

export type GizmoMode = 'translate' | 'rotate' | 'scale';
export type GizmoSpace = 'world' | 'local';

export interface TransformGizmoCallbacks {
  onDraggingChanged(isDragging: boolean): void;
  /** Fires every frame during a drag (preview) and once on commit. */
  onTransformChange(id: string, transform: Transform3): void;
  onDragEnd(id: string): void;
}

/** Geotoy adapter around `CustomGizmo`; caller must `update()` per frame. */
export class TransformGizmo {
  private readonly gizmo: CustomGizmo;
  private readonly overlay: THREE.Scene;
  private readonly getTree: () => TreeDef;
  private readonly callbacks: TransformGizmoCallbacks;
  private attachedId: string | null = null;

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
      onDragStart: () => callbacks.onDraggingChanged(true),
      onDragEnd: () => {
        callbacks.onDraggingChanged(false);
        if (this.attachedId) callbacks.onDragEnd(this.attachedId);
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

  /** `null` / root / unknown id all detach. */
  syncTo(nodeId: string | null, tree: TreeDef): void {
    if (this.gizmo.isDragging()) return; // don't yank mid-drag

    if (nodeId === null || nodeId === tree.rootId || !tree.nodes[nodeId]) {
      if (this.attachedId !== null) {
        this.gizmo.setTarget(null);
        this.attachedId = null;
      }
      return;
    }

    if (this.attachedId === nodeId) return;
    this.attachedId = nodeId;
    this.gizmo.setTarget(
      new TreeNodeTarget(nodeId, this.getTree, {
        onChange: (_phase, id, transform) => this.callbacks.onTransformChange(id, transform),
      })
    );
  }
}
