import * as THREE from 'three';
import { TransformControls } from 'three/examples/jsm/controls/TransformControls.js';

import type { Transform3, TreeDef } from 'src/geoscript/geotoyAPIClient';
import { buildWorldMatrixCache } from 'src/geoscript/runner/geoscriptRunner';
import { buildParentMap } from './treeOps';

export type GizmoMode = 'translate' | 'rotate' | 'scale';
export type GizmoSpace = 'world' | 'local';

export interface TransformGizmoCallbacks {
  onDraggingChanged(isDragging: boolean): void;
  /** Fires every frame during a drag. */
  onTransformChange(id: string, transform: Transform3): void;
  onDragEnd(id: string): void;
}

/**
 * Owns a proxy parent/child pair: parent = node's ancestor world matrix, child =
 * node's local transform. Attaching TransformControls to the child makes three.js
 * interpret drags in parent space, so the child's transform reads back as the
 * new local transform.
 */
export class TransformGizmo {
  private readonly controls: TransformControls;
  private readonly proxyParent: THREE.Group;
  private readonly proxyChild: THREE.Group;
  private readonly overlay: THREE.Scene;

  private attachedId: string | null = null;
  private isDragging = false;
  private mode: GizmoMode = 'translate';
  private space: GizmoSpace = 'local';

  constructor(
    camera: THREE.Camera,
    domElement: HTMLCanvasElement,
    overlayScene: THREE.Scene,
    callbacks: TransformGizmoCallbacks
  ) {
    this.overlay = overlayScene;

    this.proxyParent = new THREE.Group();
    this.proxyParent.matrixAutoUpdate = false;
    this.proxyChild = new THREE.Group();
    this.proxyChild.rotation.order = 'YXZ';
    this.proxyParent.add(this.proxyChild);
    overlayScene.add(this.proxyParent);

    this.controls = new TransformControls(camera, domElement);
    this.controls.setMode(this.mode);
    this.controls.setSpace(this.space);
    overlayScene.add(this.controls);

    this.controls.addEventListener('dragging-changed', (e: any) => {
      this.isDragging = e.value;
      callbacks.onDraggingChanged(e.value);
      if (!e.value && this.attachedId) {
        callbacks.onDragEnd(this.attachedId);
      }
    });

    this.controls.addEventListener('objectChange', () => {
      if (!this.attachedId) return;
      callbacks.onTransformChange(this.attachedId, this.readChildTransform());
    });
  }

  dispose() {
    this.controls.detach();
    this.controls.dispose();
    this.overlay.remove(this.controls);
    this.overlay.remove(this.proxyParent);
  }

  getMode(): GizmoMode {
    return this.mode;
  }

  setMode(mode: GizmoMode) {
    this.mode = mode;
    this.controls.setMode(mode);
  }

  getSpace(): GizmoSpace {
    return this.space;
  }

  setSpace(space: GizmoSpace) {
    this.space = space;
    this.controls.setSpace(space);
  }

  toggleSpace() {
    this.setSpace(this.space === 'world' ? 'local' : 'world');
  }

  dragging(): boolean {
    return this.isDragging;
  }

  /** Attach to (or move to) `nodeId`. `null` detaches. */
  syncTo(nodeId: string | null, tree: TreeDef): void {
    if (this.isDragging) return; // don't yank the gizmo out from under a live drag

    if (nodeId === null || nodeId === tree.rootId || !tree.nodes[nodeId]) {
      if (this.attachedId !== null) {
        this.controls.detach();
        this.attachedId = null;
      }
      return;
    }

    const node = tree.nodes[nodeId];
    const parentMap = buildParentMap(tree);
    const worldMatrices = buildWorldMatrixCache(tree, parentMap);
    const parentId = parentMap.get(nodeId);
    const parentWorld = parentId ? worldMatrices.get(parentId) : null;
    this.proxyParent.matrix.copy(parentWorld ?? new THREE.Matrix4());
    this.proxyParent.matrixWorldNeedsUpdate = true;

    this.proxyChild.position.set(...node.transform.pos);
    this.proxyChild.rotation.set(node.transform.rot[0], node.transform.rot[1], node.transform.rot[2]);
    this.proxyChild.scale.set(...node.transform.scale);

    if (this.attachedId !== nodeId) {
      this.controls.attach(this.proxyChild);
      this.attachedId = nodeId;
    }
  }

  private readChildTransform(): Transform3 {
    const c = this.proxyChild;
    return {
      pos: [c.position.x, c.position.y, c.position.z],
      rot: [c.rotation.x, c.rotation.y, c.rotation.z],
      scale: [c.scale.x, c.scale.y, c.scale.z],
    };
  }
}
