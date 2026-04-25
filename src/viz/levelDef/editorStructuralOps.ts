import type * as THREE from 'three';
import { type LevelObject, type LevelGroup, type LevelSceneNode, isLevelGroup } from './levelSceneTypes';
import { applySnapshot, snapshotTransform } from './TransformHandler';
import type {
  StructuralCtx,
  Placement,
  ParentRef,
  RuntimeSubtree,
  StructuralOp,
} from './editorStructuralTypes';

export interface ReplaceLeafOptions {
  /** Override the `visible` flag on the new object before it is added to the scene. */
  visible?: boolean;
  /**
   * Skip mesh registration on both the old and new object.
   * Used in CSG active-edit mode where the preview system owns raycast registration.
   */
  skipMeshRegistration?: boolean;
}

/**
 * Replace a level object's Three.js instance in place, preserving its parent in the
 * scene hierarchy. Updates physics and mesh registration.
 *
 * This is the authoritative path for in-place instance replacement. Both geo hot-reload
 * and CSG asset re-resolution go through here.
 */
export function replaceLeafInstance(
  ctx: StructuralCtx,
  levelObj: LevelObject,
  nextObject: THREE.Mesh,
  opts?: ReplaceLeafOptions
): void {
  if (!opts?.skipMeshRegistration) {
    ctx.unregisterMeshes(levelObj);
  }
  ctx.removePhysics(levelObj);

  const oldParent = levelObj.object.parent ?? ctx.viz.scene;
  oldParent.remove(levelObj.object);

  levelObj.object = nextObject;

  if (opts?.visible !== undefined) {
    nextObject.visible = opts.visible;
  }

  oldParent.add(nextObject);

  if (!opts?.skipMeshRegistration) {
    ctx.registerMeshes(levelObj);
  }
  ctx.syncPhysics(levelObj);
}

/** Collect all LevelObjects that are leaves of this subtree. */
export function collectSubtreeLeaves(node: LevelSceneNode): LevelObject[] {
  if (!isLevelGroup(node)) return [node];
  const leaves: LevelObject[] = [];
  for (const child of node.children) {
    leaves.push(...collectSubtreeLeaves(child));
  }
  return leaves;
}

/** Collect every LevelSceneNode in the subtree (root first, depth-first). */
export function collectAllSubtreeNodes(node: LevelSceneNode): LevelSceneNode[] {
  if (!isLevelGroup(node)) return [node];
  const result: LevelSceneNode[] = [node];
  for (const child of node.children) {
    result.push(...collectAllSubtreeNodes(child));
  }
  return result;
}

function findPlacementInChildren(
  children: LevelSceneNode[],
  parentRef: ParentRef,
  target: LevelSceneNode
): Placement | null {
  const idx = children.indexOf(target);
  if (idx !== -1) return { parent: parentRef, index: idx };
  for (const child of children) {
    if (isLevelGroup(child)) {
      const result = findPlacementInChildren(child.children, { type: 'group', groupId: child.id }, target);
      if (result) return result;
    }
  }
  return null;
}

/**
 * Capture the current logical placement of a node (parent ref + index within
 * the parent's children array). Throws if the node is not in the tree.
 */
export function capturePlacement(ctx: StructuralCtx, node: LevelSceneNode): Placement {
  const result = findPlacementInChildren(ctx.rootNodes, { type: 'root' }, node);
  if (!result) throw new Error(`[StructuralOps] capturePlacement: node "${node.id}" not found in tree`);
  return result;
}

/**
 * Remove a subtree from the scene and all editor tracking structures.
 * Returns a RuntimeSubtree that can be passed to attachSubtree to restore it.
 */
export function detachSubtree(ctx: StructuralCtx, node: LevelSceneNode): RuntimeSubtree {
  const placement = capturePlacement(ctx, node);
  const leaves = collectSubtreeLeaves(node);
  const transform = snapshotTransform(node.object);

  // Remove from logical tree
  if (placement.parent.type === 'root') {
    ctx.rootNodes.splice(placement.index, 1);
  } else {
    const parentGroup = ctx.nodeById.get(placement.parent.groupId) as LevelGroup;
    parentGroup.children.splice(placement.index, 1);
  }

  // Remove Three.js object from its parent
  (node.object.parent ?? ctx.viz.scene).remove(node.object);

  // Deregister leaves
  for (const leaf of leaves) {
    const idx = ctx.allLevelObjects.indexOf(leaf);
    if (idx !== -1) ctx.allLevelObjects.splice(idx, 1);
    ctx.unregisterMeshes(leaf);
    ctx.removePhysics(leaf);
  }

  // Clean up nodeById for the entire subtree
  for (const n of collectAllSubtreeNodes(node)) {
    ctx.nodeById.delete(n.id);
  }

  return { root: node, placement, transform, leaves };
}

/**
 * Insert a subtree into the scene and all editor tracking structures at the
 * placement stored in the subtree. Works for both newly-built subtrees and
 * previously-detached ones.
 */
export function attachSubtree(ctx: StructuralCtx, subtree: RuntimeSubtree): void {
  const { root, placement, transform, leaves } = subtree;

  // Resolve Three.js parent and logical children array
  let threeParent: THREE.Object3D;
  let logicalChildren: LevelSceneNode[];

  if (placement.parent.type === 'root') {
    threeParent = ctx.viz.scene;
    logicalChildren = ctx.rootNodes;
  } else {
    const parentNode = ctx.nodeById.get(placement.parent.groupId);
    if (!parentNode || !isLevelGroup(parentNode)) {
      throw new Error(`[StructuralOps] attachSubtree: parent group "${placement.parent.groupId}" not found`);
    }
    threeParent = parentNode.object;
    logicalChildren = parentNode.children;
  }

  // Insert into logical tree (clamp index to avoid out-of-bounds splice)
  const insertIdx = Math.min(placement.index, logicalChildren.length);
  logicalChildren.splice(insertIdx, 0, root);

  applySnapshot(root.object, transform);
  root.def.position = [...transform.position];
  root.def.rotation = [...transform.rotation];
  root.def.scale = [...transform.scale];

  // Add Three.js object to parent (children within a group are already parented to the group)
  threeParent.add(root.object);

  // Register all nodes in nodeById
  for (const n of collectAllSubtreeNodes(root)) {
    ctx.nodeById.set(n.id, n);
  }

  // Register leaves with editor tracking
  for (const leaf of leaves) {
    ctx.allLevelObjects.push(leaf);
    ctx.registerMeshes(leaf);
    ctx.syncPhysics(leaf);
  }
}

/**
 * Apply a structural op and return its inverse.
 * Used by the undo system to replay ops in either direction.
 */
export function applyStructuralOp(ctx: StructuralCtx, op: StructuralOp): StructuralOp {
  switch (op.type) {
    case 'attach_subtree': {
      attachSubtree(ctx, op.subtree);
      return { type: 'detach_subtree', subtree: op.subtree };
    }
    case 'detach_subtree': {
      const captured = detachSubtree(ctx, op.subtree.root);
      return { type: 'attach_subtree', subtree: captured };
    }
  }
}
