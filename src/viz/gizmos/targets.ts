import * as THREE from 'three';

import type { GizmoTarget, Transform3 } from './gizmoTypes';

export interface Object3DTargetCallbacks {
  onChange?(phase: 'preview' | 'commit', obj: THREE.Object3D): void;
}

export class Object3DTarget implements GizmoTarget {
  constructor(
    private readonly obj: THREE.Object3D,
    private readonly callbacks: Object3DTargetCallbacks = {}
  ) {}

  get object(): THREE.Object3D {
    return this.obj;
  }

  getRenderMatrix(out: THREE.Matrix4): THREE.Matrix4 {
    this.obj.updateMatrixWorld(true);
    return out.copy(this.obj.matrixWorld);
  }

  getParentWorldMatrix(out: THREE.Matrix4): THREE.Matrix4 {
    if (this.obj.parent) {
      this.obj.parent.updateMatrixWorld(true);
      return out.copy(this.obj.parent.matrixWorld);
    }
    return out.identity();
  }

  getLocalTransform(out: Transform3): Transform3 {
    const p = this.obj.position;
    const r = this.obj.rotation;
    const s = this.obj.scale;
    out.pos[0] = p.x;
    out.pos[1] = p.y;
    out.pos[2] = p.z;
    out.rot[0] = r.x;
    out.rot[1] = r.y;
    out.rot[2] = r.z;
    out.scale[0] = s.x;
    out.scale[1] = s.y;
    out.scale[2] = s.z;
    return out;
  }

  getEulerOrder(): THREE.EulerOrder {
    return this.obj.rotation.order;
  }

  applyLocalTransform(t: Readonly<Transform3>, phase: 'preview' | 'commit'): void {
    this.obj.position.set(t.pos[0], t.pos[1], t.pos[2]);
    this.obj.rotation.set(t.rot[0], t.rot[1], t.rot[2]);
    this.obj.scale.set(t.scale[0], t.scale[1], t.scale[2]);
    this.callbacks.onChange?.(phase, this.obj);
  }
}

export interface PivotChild {
  obj: THREE.Object3D;
  startWorldMatrix: THREE.Matrix4;
  startParentWorldMatrix: THREE.Matrix4;
}

export interface PivotTargetCallbacks {
  onChange?(phase: 'preview' | 'commit', children: ReadonlyArray<PivotChild>): void;
}

/**
 * Synthesises a pivot at the centroid of `children` and applies the world-space
 * delta `currentPivotWorld * inv(startPivotWorld)` to each child.  Rotation
 * sweeps children around the centroid (Blender group-rotate behaviour).
 */
export class PivotTarget implements GizmoTarget {
  private pivotLocal: Transform3 = {
    pos: [0, 0, 0],
    rot: [0, 0, 0],
    scale: [1, 1, 1],
  };
  private startPivotWorldInv: THREE.Matrix4;
  private startPivotWorld: THREE.Matrix4;
  private children: PivotChild[];

  constructor(
    objects: ReadonlyArray<THREE.Object3D>,
    private readonly callbacks: PivotTargetCallbacks = {}
  ) {
    if (objects.length === 0) throw new Error('PivotTarget needs at least one object');

    const centroid = new THREE.Vector3();
    for (const obj of objects) {
      obj.updateMatrixWorld(true);
      centroid.add(obj.getWorldPosition(new THREE.Vector3()));
    }
    centroid.divideScalar(objects.length);
    this.pivotLocal.pos = [centroid.x, centroid.y, centroid.z];

    this.startPivotWorld = new THREE.Matrix4().compose(
      centroid,
      new THREE.Quaternion(),
      new THREE.Vector3(1, 1, 1)
    );
    this.startPivotWorldInv = new THREE.Matrix4().copy(this.startPivotWorld).invert();

    this.children = objects.map(
      (obj): PivotChild => ({
        obj,
        startWorldMatrix: new THREE.Matrix4().copy(obj.matrixWorld),
        startParentWorldMatrix: obj.parent
          ? new THREE.Matrix4().copy(obj.parent.matrixWorld)
          : new THREE.Matrix4(),
      })
    );
  }

  /** Reset baselines so the next drag's deltas are measured from the current state. */
  rebaseline(): void {
    for (const c of this.children) {
      c.obj.updateMatrixWorld(true);
      c.startWorldMatrix.copy(c.obj.matrixWorld);
      if (c.obj.parent) {
        c.obj.parent.updateMatrixWorld(true);
        c.startParentWorldMatrix.copy(c.obj.parent.matrixWorld);
      } else {
        c.startParentWorldMatrix.identity();
      }
    }
    const centroid = new THREE.Vector3();
    for (const c of this.children) centroid.add(c.obj.getWorldPosition(new THREE.Vector3()));
    centroid.divideScalar(this.children.length);
    this.pivotLocal.pos = [centroid.x, centroid.y, centroid.z];
    this.pivotLocal.rot = [0, 0, 0];
    this.pivotLocal.scale = [1, 1, 1];
    this.startPivotWorld.compose(centroid, _zeroQuat, _oneVec);
    this.startPivotWorldInv.copy(this.startPivotWorld).invert();
  }

  getChildren(): ReadonlyArray<PivotChild> {
    return this.children;
  }

  getRenderMatrix(out: THREE.Matrix4): THREE.Matrix4 {
    _scratchPos.set(this.pivotLocal.pos[0], this.pivotLocal.pos[1], this.pivotLocal.pos[2]);
    _scratchEuler.set(this.pivotLocal.rot[0], this.pivotLocal.rot[1], this.pivotLocal.rot[2], 'XYZ');
    _scratchQuat.setFromEuler(_scratchEuler);
    _scratchScale.set(this.pivotLocal.scale[0], this.pivotLocal.scale[1], this.pivotLocal.scale[2]);
    return out.compose(_scratchPos, _scratchQuat, _scratchScale);
  }

  getParentWorldMatrix(out: THREE.Matrix4): THREE.Matrix4 {
    return out.identity();
  }

  getLocalTransform(out: Transform3): Transform3 {
    out.pos[0] = this.pivotLocal.pos[0];
    out.pos[1] = this.pivotLocal.pos[1];
    out.pos[2] = this.pivotLocal.pos[2];
    out.rot[0] = this.pivotLocal.rot[0];
    out.rot[1] = this.pivotLocal.rot[1];
    out.rot[2] = this.pivotLocal.rot[2];
    out.scale[0] = this.pivotLocal.scale[0];
    out.scale[1] = this.pivotLocal.scale[1];
    out.scale[2] = this.pivotLocal.scale[2];
    return out;
  }

  getEulerOrder(): THREE.EulerOrder {
    return 'XYZ';
  }

  applyLocalTransform(t: Readonly<Transform3>, phase: 'preview' | 'commit'): void {
    this.pivotLocal.pos[0] = t.pos[0];
    this.pivotLocal.pos[1] = t.pos[1];
    this.pivotLocal.pos[2] = t.pos[2];
    this.pivotLocal.rot[0] = t.rot[0];
    this.pivotLocal.rot[1] = t.rot[1];
    this.pivotLocal.rot[2] = t.rot[2];
    this.pivotLocal.scale[0] = t.scale[0];
    this.pivotLocal.scale[1] = t.scale[1];
    this.pivotLocal.scale[2] = t.scale[2];

    _scratchPos.set(t.pos[0], t.pos[1], t.pos[2]);
    _scratchEuler.set(t.rot[0], t.rot[1], t.rot[2], 'XYZ');
    _scratchQuat.setFromEuler(_scratchEuler);
    _scratchScale.set(t.scale[0], t.scale[1], t.scale[2]);
    _scratchMat.compose(_scratchPos, _scratchQuat, _scratchScale);

    const delta = _scratchMatB.copy(_scratchMat).multiply(this.startPivotWorldInv);

    for (const c of this.children) {
      const newChildWorld = _scratchMatC.copy(delta).multiply(c.startWorldMatrix);
      _scratchMatD.copy(c.startParentWorldMatrix).invert().multiply(newChildWorld);
      _scratchMatD.decompose(_decomposePos, _decomposeQuat, _decomposeScale);
      c.obj.position.copy(_decomposePos);
      c.obj.quaternion.copy(_decomposeQuat);
      c.obj.scale.copy(_decomposeScale);
    }

    this.callbacks.onChange?.(phase, this.children);
  }
}

import type { Transform3 as ApiTransform3, TreeDef } from 'src/geoscript/geotoyAPIClient';
import { buildWorldMatrixCache } from 'src/geoscript/runner/geoscriptRunner';
import { buildParentMap } from 'src/viz/scenes/geoscriptPlayground/treeOps';

export interface TreeNodeTargetCallbacks {
  onChange?(phase: 'preview' | 'commit', id: string, transform: ApiTransform3): void;
}

/**
 * Edits a geoscript tree node.  Walks the tree's parent chain to compute the
 * render matrix — there's no stable THREE.js scene graph to attach to since the
 * scene is rebuilt from scratch each run.
 */
export class TreeNodeTarget implements GizmoTarget {
  constructor(
    private readonly nodeId: string,
    private readonly getTree: () => TreeDef,
    private readonly callbacks: TreeNodeTargetCallbacks = {}
  ) {}

  get id(): string {
    return this.nodeId;
  }

  getRenderMatrix(out: THREE.Matrix4): THREE.Matrix4 {
    const tree = this.getTree();
    const parentMap = buildParentMap(tree);
    const cache = buildWorldMatrixCache(tree, parentMap);
    const m = cache.get(this.nodeId);
    return m ? out.copy(m) : out.identity();
  }

  getParentWorldMatrix(out: THREE.Matrix4): THREE.Matrix4 {
    const tree = this.getTree();
    const parentMap = buildParentMap(tree);
    const parentId = parentMap.get(this.nodeId);
    if (!parentId) return out.identity();
    const cache = buildWorldMatrixCache(tree, parentMap);
    const m = cache.get(parentId);
    return m ? out.copy(m) : out.identity();
  }

  getLocalTransform(out: Transform3): Transform3 {
    const tree = this.getTree();
    const node = tree.nodes[this.nodeId];
    if (!node) return out;
    const t = node.transform;
    out.pos[0] = t.pos[0];
    out.pos[1] = t.pos[1];
    out.pos[2] = t.pos[2];
    out.rot[0] = t.rot[0];
    out.rot[1] = t.rot[1];
    out.rot[2] = t.rot[2];
    out.scale[0] = t.scale[0];
    out.scale[1] = t.scale[1];
    out.scale[2] = t.scale[2];
    return out;
  }

  getEulerOrder(): THREE.EulerOrder {
    return 'YXZ';
  }

  applyLocalTransform(t: Readonly<Transform3>, phase: 'preview' | 'commit'): void {
    const transform: ApiTransform3 = {
      pos: [t.pos[0], t.pos[1], t.pos[2]],
      rot: [t.rot[0], t.rot[1], t.rot[2]],
      scale: [t.scale[0], t.scale[1], t.scale[2]],
    };
    this.callbacks.onChange?.(phase, this.nodeId, transform);
  }
}

const _scratchPos = new THREE.Vector3();
const _scratchScale = new THREE.Vector3();
const _scratchQuat = new THREE.Quaternion();
const _scratchEuler = new THREE.Euler();
const _scratchMat = new THREE.Matrix4();
const _scratchMatB = new THREE.Matrix4();
const _scratchMatC = new THREE.Matrix4();
const _scratchMatD = new THREE.Matrix4();
const _decomposePos = new THREE.Vector3();
const _decomposeQuat = new THREE.Quaternion();
const _decomposeScale = new THREE.Vector3();
const _zeroQuat = new THREE.Quaternion();
const _oneVec = new THREE.Vector3(1, 1, 1);
