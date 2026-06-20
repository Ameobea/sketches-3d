import type * as THREE from 'three';

export type GizmoMode = 'translate' | 'rotate' | 'scale';
export type GizmoSpace = 'world' | 'local';

export interface AxisMask {
  x: boolean;
  y: boolean;
  z: boolean;
}

export const ALL_AXES: AxisMask = Object.freeze({ x: true, y: true, z: true });
export const NO_AXES: AxisMask = Object.freeze({ x: false, y: false, z: false });

export interface Transform3 {
  pos: [number, number, number];
  rot: [number, number, number];
  scale: [number, number, number];
}

/** Stamped onto picker meshes' `userData.gizmoHandle`; resolves a raycast hit to a drag config. */
export type GizmoHandleId =
  | { kind: 'translate-axis'; axis: 'x' | 'y' | 'z' }
  | { kind: 'translate-plane'; axes: ['x', 'y'] | ['x', 'z'] | ['y', 'z'] }
  | { kind: 'rotate-axis'; axis: 'x' | 'y' | 'z' }
  | { kind: 'scale-axis'; axis: 'x' | 'y' | 'z' }
  | { kind: 'scale-plane'; axes: ['x', 'y'] | ['x', 'z'] | ['y', 'z'] }
  | { kind: 'scale-uniform' };

/**
 * Decouples the gizmo from what it's editing.  Implementations:
 * `Object3DTarget`, `PivotTarget`, `InstanceTarget`.  The gizmo always works
 * in world space and converts to local via `inverse(parentWorld)`.
 */
export interface GizmoTarget {
  /** Where the gizmo is drawn; basis defines visible axis directions in local-space mode. */
  getRenderMatrix(out: THREE.Matrix4): THREE.Matrix4;

  /** Identity for a root object. */
  getParentWorldMatrix(out: THREE.Matrix4): THREE.Matrix4;

  getLocalTransform(out: Transform3): Transform3;

  /** Mismatch between this and how the target stores rotations silently corrupts data on commit. */
  getEulerOrder(): THREE.EulerOrder;

  /** `preview`: per-frame during drag, no undo. `commit`: drag end, target should snapshot/persist. */
  applyLocalTransform(t: Readonly<Transform3>, phase: 'preview' | 'commit'): void;
}

/**
 * What a Geotoy viewport gizmo is bound to. `instance` names a node placement by its
 * stable id; `handle` names a `gizmo(...)` value site (M3).
 */
export type GizmoTargetRef =
  | { kind: 'instance'; nodeId: string; instanceId: string }
  | { kind: 'handle'; nodeId: string; name: string };

/** Live binding context for a `gizmo(...)` handle, resolved fresh each frame from the
 *  run channel + stored value so a `HandleTarget` never holds stale origin/mode/transform. */
export interface HandleContext {
  kind: 'vec3' | 'transform';
  mode: 'delta' | 'absolute';
  origin: [number, number, number];
  /** For `transform` handles: the runtime-reported transform (full pos/rot/scale). */
  transform?: Transform3;
  /** Translate-axis restriction for `gizmo2d`/`gizmo1d`; defaults to all axes. */
  axes?: [boolean, boolean, boolean];
}

export const gizmoTargetRefsEqual = (a: GizmoTargetRef | null, b: GizmoTargetRef | null): boolean => {
  if (a === null || b === null) return a === b;
  if (a.kind !== b.kind) return false;
  if (a.kind === 'instance' && b.kind === 'instance')
    return a.nodeId === b.nodeId && a.instanceId === b.instanceId;
  if (a.kind === 'handle' && b.kind === 'handle') return a.nodeId === b.nodeId && a.name === b.name;
  return false;
};

export const makeTransform3 = (): Transform3 => ({
  pos: [0, 0, 0],
  rot: [0, 0, 0],
  scale: [1, 1, 1],
});

export const copyTransform3 = (dst: Transform3, src: Readonly<Transform3>): Transform3 => {
  dst.pos[0] = src.pos[0];
  dst.pos[1] = src.pos[1];
  dst.pos[2] = src.pos[2];
  dst.rot[0] = src.rot[0];
  dst.rot[1] = src.rot[1];
  dst.rot[2] = src.rot[2];
  dst.scale[0] = src.scale[0];
  dst.scale[1] = src.scale[1];
  dst.scale[2] = src.scale[2];
  return dst;
};
