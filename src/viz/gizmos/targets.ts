import * as THREE from 'three';

import {
  type GizmoTarget,
  type HandleContext,
  type Transform3,
  copyTransform3,
  makeTransform3,
} from './gizmoTypes';

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

import type { GizmoValue, Transform3 as ApiTransform3, TreeDef } from 'src/geoscript/geotoyAPIClient';
import { composeTransform3 } from 'src/geoscript/runner/worldMatrixCache';
import { composeInstance0World, getNodeAncestorChain } from 'src/viz/scenes/geoscriptPlayground/treeOps';

export interface InstanceTargetCallbacks {
  onChange?(phase: 'preview' | 'commit', nodeId: string, instanceId: string, transform: ApiTransform3): void;
}

/**
 * Edits one placement (the `instances` entry with id `instanceId`) of a geoscript tree
 * node. There's no persistent scene graph to attach to (the scene is rebuilt each run),
 * so the render matrix is composed from the ancestor chain (each ancestor at instance 0 —
 * the representative copy). Editing the shared instance moves every copy that uses it.
 */
export class InstanceTarget implements GizmoTarget {
  constructor(
    private readonly nodeId: string,
    private readonly instanceId: string,
    private readonly getTree: () => TreeDef,
    private readonly callbacks: InstanceTargetCallbacks = {}
  ) {}

  // Composes the representative copy's world into `world` and its parent's into
  // `parentWorld` in one O(depth) ancestor walk. False if the instance no longer exists.
  private compose(world: THREE.Matrix4, parentWorld: THREE.Matrix4): boolean {
    const tree = this.getTree();
    const node = tree.nodes[this.nodeId];
    const inst = node?.instances.find(i => i.id === this.instanceId);
    if (!node || !inst) return false;
    parentWorld.identity();
    const chain = getNodeAncestorChain(tree, this.nodeId);
    if (chain) {
      for (let i = chain.length - 1; i >= 1; i--) {
        parentWorld.multiply(composeTransform3(_scratchMat, chain[i].instances[0]));
      }
    }
    world.copy(parentWorld).multiply(composeTransform3(_scratchMat, inst));
    return true;
  }

  getRenderMatrix(out: THREE.Matrix4): THREE.Matrix4 {
    return this.compose(out, _scratchMatB) ? out : out.identity();
  }

  getParentWorldMatrix(out: THREE.Matrix4): THREE.Matrix4 {
    return this.compose(_scratchMatB, out) ? out : out.identity();
  }

  getLocalTransform(out: Transform3): Transform3 {
    const t = this.getTree().nodes[this.nodeId]?.instances.find(i => i.id === this.instanceId);
    return t ? copyTransform3(out, t) : out;
  }

  getEulerOrder(): THREE.EulerOrder {
    return 'YXZ';
  }

  applyLocalTransform(t: Readonly<Transform3>, phase: 'preview' | 'commit'): void {
    this.callbacks.onChange?.(phase, this.nodeId, this.instanceId, {
      pos: [t.pos[0], t.pos[1], t.pos[2]],
      rot: [t.rot[0], t.rot[1], t.rot[2]],
      scale: [t.scale[0], t.scale[1], t.scale[2]],
    });
  }
}

export interface HandleTargetCallbacks {
  onChange?(phase: 'preview' | 'commit', nodeId: string, handleId: string, value: GizmoValue): void;
  /** Extra world transform composed before the node chain (e.g. a level placement's matrix). */
  getBaseMatrix?(): THREE.Matrix4 | null;
  /** Current handle value override; defaults to the tree's `node.handles[handleId]`. */
  getStoredValue?(): GizmoValue | undefined;
}

/**
 * Edits a `gizmo(...)` handle value stored in `node.handles[handleId]`. The handle
 * lives in the node's local space (where its geoscript renders), so it's drawn composed
 * with the node's representative (instance 0) world matrix. vec3 handles are translate-only;
 * `delta` handles store the offset from `origin`, `absolute` ones store the position itself.
 * `origin` is the runtime-reported anchor (from the rendered-gizmos channel).
 *
 * `getTree` may return null for single-module programs with no tree (level-def geoscript
 * assets); the base matrix is then the whole node-world transform.
 */
export class HandleTarget implements GizmoTarget {
  constructor(
    private readonly nodeId: string,
    private readonly handleId: string,
    // Resolved fresh each frame — origin/mode/transform follow the run channel without
    // rebuilding the target, so the gizmo never holds a stale anchor.
    private readonly getContext: () => HandleContext | null,
    private readonly getTree: () => TreeDef | null,
    private readonly callbacks: HandleTargetCallbacks = {}
  ) {}

  // Full world of the node's representative (instance 0) copy, root → node inclusive,
  // composed onto the optional base matrix.
  private nodeWorld(out: THREE.Matrix4): boolean {
    out.identity();
    const base = this.callbacks.getBaseMatrix?.();
    if (base) out.copy(base);
    const tree = this.getTree();
    if (!tree) return true;
    return composeInstance0World(tree, this.nodeId, out, _scratchMat);
  }

  private localTransform(out: Transform3, ctx: HandleContext): Transform3 {
    const stored = this.callbacks.getStoredValue
      ? this.callbacks.getStoredValue()
      : this.getTree()?.nodes[this.nodeId]?.handles?.[this.handleId];
    if (ctx.kind === 'transform') {
      const src = (stored?.value as ApiTransform3 | undefined) ??
        ctx.transform ?? { pos: [...ctx.origin], rot: [0, 0, 0], scale: [1, 1, 1] };
      return copyTransform3(out, src);
    }
    out.rot[0] = out.rot[1] = out.rot[2] = 0;
    out.scale[0] = out.scale[1] = out.scale[2] = 1;
    const v = stored?.value as [number, number, number] | undefined;
    const base = ctx.mode === 'delta' ? ctx.origin : (v ?? ctx.origin);
    const d = ctx.mode === 'delta' ? (v ?? [0, 0, 0]) : [0, 0, 0];
    out.pos[0] = base[0] + d[0];
    out.pos[1] = base[1] + d[1];
    out.pos[2] = base[2] + d[2];
    return out;
  }

  getRenderMatrix(out: THREE.Matrix4): THREE.Matrix4 {
    const ctx = this.getContext();
    if (!ctx || !this.nodeWorld(out)) return out.identity();
    return out.multiply(composeTransform3(_scratchMatB, this.localTransform(_handleScratchT, ctx)));
  }

  getParentWorldMatrix(out: THREE.Matrix4): THREE.Matrix4 {
    return this.nodeWorld(out) ? out : out.identity();
  }

  getLocalTransform(out: Transform3): Transform3 {
    const ctx = this.getContext();
    return ctx ? copyTransform3(out, this.localTransform(_handleScratchT, ctx)) : out;
  }

  getEulerOrder(): THREE.EulerOrder {
    return 'YXZ';
  }

  applyLocalTransform(t: Readonly<Transform3>, phase: 'preview' | 'commit'): void {
    const ctx = this.getContext();
    if (!ctx) return;
    let value: GizmoValue;
    if (ctx.kind === 'transform') {
      value = {
        kind: 'transform',
        mode: ctx.mode,
        value: {
          pos: [t.pos[0], t.pos[1], t.pos[2]],
          rot: [t.rot[0], t.rot[1], t.rot[2]],
          scale: [t.scale[0], t.scale[1], t.scale[2]],
        },
      };
    } else {
      const pos: [number, number, number] =
        ctx.mode === 'delta'
          ? [t.pos[0] - ctx.origin[0], t.pos[1] - ctx.origin[1], t.pos[2] - ctx.origin[2]]
          : [t.pos[0], t.pos[1], t.pos[2]];
      value = { kind: 'vec3', mode: ctx.mode, value: pos };
    }
    this.callbacks.onChange?.(phase, this.nodeId, this.handleId, value);
  }
}

const _handleScratchT: Transform3 = makeTransform3();
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
