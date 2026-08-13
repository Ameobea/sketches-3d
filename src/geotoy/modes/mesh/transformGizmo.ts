import type * as THREE from 'three';

import type { GizmoValue, Transform3, TreeDef } from 'src/geoscript/geotoyAPIClient';
import { CustomGizmo } from 'src/viz/gizmos/customGizmo';
import { HandleTarget, InstanceTarget } from 'src/viz/gizmos/targets';
import {
  type GizmoTarget,
  type GizmoTargetRef,
  type HandleContext,
  gizmoTargetRefsEqual,
} from 'src/viz/gizmos/gizmoTypes';

export type { HandleContext };

export type GizmoMode = 'translate' | 'rotate' | 'scale';
export type GizmoSpace = 'world' | 'local';

export interface TransformGizmoCallbacks {
  onDraggingChanged(isDragging: boolean): void;
  /** Fires once at the start of a drag with the ref being edited. */
  onDragStart?(ref: GizmoTargetRef): void;
  /** Fires every frame during a drag (preview) and once on commit. */
  onTransformChange(ref: GizmoTargetRef, transform: Transform3): void;
  /** Like `onTransformChange` but for a `gizmo(...)` handle ref. */
  onHandleChange?(nodeId: string, handleId: string, value: GizmoValue): void;
  onDragEnd(ref: GizmoTargetRef): void;
}

/** Geotoy adapter around `CustomGizmo`; caller must `update()` per frame. */
export class TransformGizmo {
  private readonly gizmo: CustomGizmo;
  private readonly overlay: THREE.Scene;
  private readonly getTree: () => TreeDef;
  private readonly callbacks: TransformGizmoCallbacks;
  private attachedRef: GizmoTargetRef | null = null;
  /** Supplies origin/kind/mode for a handle ref; set by the editor once it has run output. */
  private handleContextResolver: ((nodeId: string, handleId: string) => HandleContext | null) | null = null;

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

  setHandleContextResolver(fn: (nodeId: string, handleId: string) => HandleContext | null): void {
    this.handleContextResolver = fn;
  }

  /** Bind an arbitrary target (spline points etc.); null releases. Bypasses ref tracking —
   *  the caller must keep `syncTo` from firing while a custom target is attached. */
  setCustomTarget(target: GizmoTarget | null): void {
    if (this.gizmo.isDragging()) return;
    this.attachedRef = null;
    this.gizmo.setAxisMask([true, true, true]);
    this.gizmo.setTarget(target);
  }

  private detach(): void {
    if (this.attachedRef !== null) {
      this.gizmo.setTarget(null);
      this.attachedRef = null;
    }
  }

  /** `null` / root / unknown node all detach. */
  syncTo(ref: GizmoTargetRef | null, tree: TreeDef): void {
    if (this.gizmo.isDragging()) return; // don't yank mid-drag

    if (ref && ref.kind === 'handle') {
      const resolve = this.handleContextResolver;
      // Handles are valid on any node incl. `_root` (only instance targeting excludes root).
      if (!resolve || !tree.nodes[ref.nodeId]) {
        this.detach();
        return;
      }
      // The target reads its context per-frame via `resolve`, so origin/mode/transform stay
      // fresh without rebuilding — restoring the equal-ref dedupe the instance path has.
      if (gizmoTargetRefsEqual(this.attachedRef, ref)) return;
      this.attachedRef = ref;
      this.gizmo.setAxisMask(resolve(ref.nodeId, ref.name)?.axes ?? [true, true, true]);
      this.gizmo.setTarget(
        new HandleTarget(ref.nodeId, ref.name, () => resolve(ref.nodeId, ref.name), this.getTree, {
          onChange: (_phase, nodeId, handleId, value) =>
            this.callbacks.onHandleChange?.(nodeId, handleId, value),
        })
      );
      return;
    }

    const node = ref && ref.kind === 'instance' ? tree.nodes[ref.nodeId] : undefined;
    const detached =
      ref === null ||
      ref.kind !== 'instance' ||
      ref.nodeId === tree.rootId ||
      !node ||
      !node.instances.some(i => i.id === ref.instanceId);
    if (detached) {
      this.detach();
      return;
    }

    if (gizmoTargetRefsEqual(this.attachedRef, ref)) return;
    this.attachedRef = ref;
    this.gizmo.setAxisMask([true, true, true]);
    this.gizmo.setTarget(
      new InstanceTarget(ref.nodeId, ref.instanceId, this.getTree, {
        onChange: (_phase, nodeId, instanceId, transform) =>
          this.callbacks.onTransformChange({ kind: 'instance', nodeId, instanceId }, transform),
      })
    );
  }
}
