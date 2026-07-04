import * as THREE from 'three';

import type { LevelGroup, LevelSceneNode } from './levelSceneTypes';
import { isLevelGroup, isEditable, isCompositionNode } from './levelSceneTypes';
import type { ObjectDef, ObjectGroupDef } from './types';
import type { StructuralCtx, RuntimeSubtree, StructuralOp, ParentRef } from './editorStructuralTypes';
import type { BuildCtx } from './editorNodeFactory';
import {
  buildLeafNode,
  buildGroupSubtree,
  buildCompositionGroupFromCtx,
  serializeGroup,
} from './editorNodeFactory';
import type { LevelEditorApi } from './levelEditorApi';
import {
  attachSubtree,
  detachSubtree,
  capturePlacement,
  collectSubtreeLeaves,
  applyStructuralOp,
} from './editorStructuralOps';
import type { TransformSnapshot } from './TransformHandler';
import { snapshotTransform, worldToLocalSnapshot } from './TransformHandler';
import { flattenLeaves, hasAsset } from './levelDefTreeUtils';
import { round } from './mathUtils';

export type StructuralUndoEntry = {
  type: 'structural';
  undoOps: StructuralOp[];
  redoOps: StructuralOp[];
};

/**
 * A clipboard entry from Ctrl+C. `worldTransform` is captured at copy time so
 * paste can place the clone sensibly even if the original parent is gone — in
 * that case it falls back to root and the world transform is projected into
 * root-local space.
 */
export type ClipboardEntry =
  | {
      kind: 'object';
      assetId: string;
      def: ObjectDef;
      parent: ParentRef;
      worldTransform: TransformSnapshot;
    }
  | {
      kind: 'group';
      def: ObjectGroupDef;
      parent: ParentRef;
      worldTransform: TransformSnapshot;
    }
  | {
      kind: 'composition';
      assetId: string;
      /** The leaf-pointer ObjectDef (asset/material/...); re-expanded from cached baked meshes. */
      def: ObjectDef;
      parent: ParentRef;
      worldTransform: TransformSnapshot;
    };

/** Vertical offset applied (in the resolved parent's local Y) to each clone. */
const PASTE_Y_OFFSET = 0.5;

type MutationCtx = StructuralCtx & BuildCtx;

/**
 * Coordinator for structural mutations in the level editor. Sits above
 * `editorStructuralOps.ts`: applies ops locally, derives persistence calls
 * from them, sequences server sync, and bundles compound structural undo
 * entries for higher-level operations. LevelEditor delegates here and reacts
 * to the returned results for selection / UI updates.
 */
export class EditorMutationController {
  private structuralSyncQueue: Promise<void> = Promise.resolve();

  constructor(
    private ctx: MutationCtx,
    private api: LevelEditorApi,
    /** Push a structural undo entry onto the undo stack. */
    private undoPush: (entry: StructuralUndoEntry) => void,
    /** Purge matching entries from both undo and redo stacks. */
    private undoPurge: (pred: (entry: any) => boolean) => void
  ) {}

  enqueueStructuralSync(task: () => Promise<void>): Promise<void> {
    this.structuralSyncQueue = this.structuralSyncQueue
      .catch(err => console.error('[EditorMutationController] structural sync queue error:', err))
      .then(async () => {
        try {
          await task();
        } catch (err) {
          console.error('[EditorMutationController] structural sync failed:', err);
        }
      });
    return this.structuralSyncQueue;
  }

  /**
   * Translate a list of structural ops into concrete persistence calls.
   *
   * - A node that is both detached and re-attached in the same op list is
   *   classified as a move (reparent).
   * - A detach with no matching re-attach is a delete.
   * - An attach with no matching detach is a restore.
   */
  async syncStructuralOps(ops: StructuralOp[]): Promise<void> {
    const detachedIds = new Set(ops.filter(op => op.type === 'detach_subtree').map(op => op.subtree.root.id));
    const movedIds = new Set<string>();
    for (const op of ops) {
      if (op.type === 'attach_subtree' && detachedIds.has(op.subtree.root.id)) {
        movedIds.add(op.subtree.root.id);
      }
    }

    // Reparents (moved nodes)
    for (const op of ops) {
      if (op.type !== 'attach_subtree' || !movedIds.has(op.subtree.root.id)) continue;
      await this.api.reparentNodes(
        [{ id: op.subtree.root.id, transform: op.subtree.transform }],
        op.subtree.placement.parent.type === 'group' ? op.subtree.placement.parent.groupId : undefined,
        op.subtree.placement.index
      );
    }

    // Deletes (detached only)
    for (const op of ops) {
      if (op.type === 'detach_subtree' && !movedIds.has(op.subtree.root.id)) {
        await this.api.sendDelete(op.subtree.root.id);
      }
    }

    // Restores (attached only)
    for (const op of ops) {
      if (op.type === 'attach_subtree' && !movedIds.has(op.subtree.root.id)) {
        await this.api.restoreSubtree(op.subtree);
      }
    }
  }

  /**
   * Apply ops from a structural undo/redo entry and enqueue persistence.
   * Returns the last attached node (to select) or null (to deselect).
   */
  applyStructuralUndoEntry(entry: StructuralUndoEntry, direction: 'undo' | 'redo'): LevelSceneNode | null {
    const ops = direction === 'undo' ? entry.undoOps : entry.redoOps;
    let nodeToSelect: LevelSceneNode | null = null;
    for (const op of ops) {
      applyStructuralOp(this.ctx, op);
      if (op.type === 'attach_subtree') nodeToSelect = op.subtree.root;
    }
    void this.enqueueStructuralSync(() => this.syncStructuralOps(ops));
    return nodeToSelect;
  }

  async spawnLeaf(
    assetId: string,
    materialId: string | undefined,
    position: [number, number, number]
  ): Promise<LevelSceneNode | null> {
    const isComposition = this.ctx.compositionBaked?.has(assetId) ?? false;
    if (!isComposition && !this.ctx.prototypes.has(assetId)) {
      console.warn(`[EditorMutationController] No prototype for asset "${assetId}"`);
      return null;
    }
    const newDef = await this.api.sendAdd({ asset: assetId, material: materialId, position });
    if (!newDef) return null;
    const root = isComposition
      ? buildCompositionGroupFromCtx(this.ctx, newDef)
      : buildLeafNode(this.ctx, assetId, newDef);
    if (!root) return null;
    return this._finalizeSpawnNode(root);
  }

  /** No undo entry — matches the pre-mutation-controller behavior. */
  async spawnGroup(position: [number, number, number]): Promise<LevelGroup | null> {
    const newDef = await this.api.sendAddGroup({ position });
    if (!newDef) return null;

    const groupObj = new THREE.Group();
    groupObj.position.fromArray(newDef.position ?? [0, 0, 0]);

    // Strip `children` from the runtime def — the hierarchy lives only on `LevelGroup.children`.
    const { children: _omitChildren, ...body } = newDef;
    const levelGroup: LevelGroup = {
      id: newDef.id,
      object: groupObj,
      def: body,
      children: [],
      generated: false,
    };
    const subtree: RuntimeSubtree = {
      root: levelGroup,
      placement: { parent: { type: 'root' }, index: this.ctx.rootNodes.length },
      transform: snapshotTransform(groupObj),
      leaves: [],
    };
    attachSubtree(this.ctx, subtree);
    return levelGroup;
  }

  /**
   * Capture a node as a clipboard entry: its def, its current parent, and its
   * world-space transform at copy-time. The world transform lets paste produce
   * sensible placement even if the original parent has been deleted by then.
   */
  captureClipboardEntry(node: LevelSceneNode, worldTransform: TransformSnapshot): ClipboardEntry {
    const parent = capturePlacement(this.ctx, node).parent;
    if (isCompositionNode(node)) {
      return {
        kind: 'composition',
        assetId: node.compositionDef.asset ?? '',
        def: JSON.parse(JSON.stringify(node.compositionDef)),
        parent,
        worldTransform,
      };
    }
    if (isLevelGroup(node)) {
      return {
        kind: 'group',
        def: JSON.parse(JSON.stringify(serializeGroup(node))),
        parent,
        worldTransform,
      };
    }
    return {
      kind: 'object',
      assetId: node.assetId,
      def: JSON.parse(JSON.stringify(node.def)),
      parent,
      worldTransform,
    };
  }

  /**
   * Each entry is placed as a sibling of its original source (or at root, if
   * the source's parent is gone) with a small local-Y offset to avoid overlap.
   * All clones are bundled into a single structural undo entry. Returns the
   * new root nodes in paste order.
   */
  async pasteEntries(entries: ClipboardEntry[]): Promise<LevelSceneNode[]> {
    const newSubtrees: RuntimeSubtree[] = [];
    const newNodes: LevelSceneNode[] = [];

    for (const entry of entries) {
      const result = await this._pasteOne(entry);
      if (!result) continue;
      newSubtrees.push(result.subtree);
      newNodes.push(result.subtree.root);
    }

    if (newSubtrees.length > 0) {
      this.undoPush({
        type: 'structural',
        undoOps: newSubtrees.map(s => ({ type: 'detach_subtree' as const, subtree: s })).reverse(),
        redoOps: newSubtrees.map(s => ({ type: 'attach_subtree' as const, subtree: s })),
      });
    }

    return newNodes;
  }

  private async _pasteOne(entry: ClipboardEntry): Promise<{ subtree: RuntimeSubtree } | null> {
    if (entry.kind === 'object') {
      if (!this.ctx.prototypes.has(entry.assetId)) {
        console.warn(
          `[EditorMutationController] No prototype for asset "${entry.assetId}" — asset may not be loaded yet`
        );
        return null;
      }
    } else if (entry.kind === 'composition') {
      if (!this.ctx.compositionBaked?.has(entry.assetId)) {
        console.warn(`[EditorMutationController] No baked meshes for composition asset "${entry.assetId}"`);
        return null;
      }
    } else {
      for (const leaf of flattenLeaves([entry.def])) {
        if (!hasAsset(leaf)) continue; // dash-token marker — no prototype needed
        const known =
          this.ctx.prototypes.has(leaf.asset) || (this.ctx.compositionBaked?.has(leaf.asset) ?? false);
        if (!known) {
          console.warn(`[EditorMutationController] No prototype for asset "${leaf.asset}" in pasted group`);
          return null;
        }
      }
    }

    // Fall back to root if the source parent is gone or has become generated (read-only).
    let parentRef: ParentRef = entry.parent;
    let targetParentObj: THREE.Object3D = this.ctx.viz.scene;
    let targetChildren: LevelSceneNode[] = this.ctx.rootNodes;
    if (parentRef.type === 'group') {
      const parentNode = this.ctx.nodeById.get(parentRef.groupId);
      if (parentNode && isLevelGroup(parentNode) && isEditable(parentNode)) {
        targetParentObj = parentNode.object;
        targetChildren = parentNode.children;
      } else {
        parentRef = { type: 'root' };
      }
    }

    const local = worldToLocalSnapshot(entry.worldTransform, targetParentObj);
    local.position = [local.position[0], local.position[1] + PASTE_Y_OFFSET, local.position[2]];
    const position = local.position.map(round) as [number, number, number];
    const rotation = local.rotation.map(round) as [number, number, number];
    const scale = local.scale.map(round) as [number, number, number];

    const parentId = parentRef.type === 'group' ? parentRef.groupId : undefined;
    const index = targetChildren.length;

    // Send the full clipboard def (fresh transform patched in) through the paste endpoint so every
    // field — behaviors/parkour/userData/flags — round-trips; the server assigns fresh ids.
    const patchedDef = {
      ...JSON.parse(JSON.stringify(entry.def)),
      position,
      rotation,
      scale,
    } as ObjectDef | ObjectGroupDef;
    const newDef = await this.api.sendPaste(patchedDef, parentId, index);
    if (!newDef) return null;

    let root: LevelSceneNode | null;
    if (entry.kind === 'composition') {
      root = buildCompositionGroupFromCtx(this.ctx, newDef as ObjectDef);
    } else if (entry.kind === 'group') {
      root = buildGroupSubtree(this.ctx, newDef as ObjectGroupDef);
    } else {
      root = buildLeafNode(this.ctx, entry.assetId, newDef as ObjectDef);
    }
    if (!root) return null;

    const leaves = collectSubtreeLeaves(root);
    const subtree: RuntimeSubtree = {
      root,
      placement: { parent: parentRef, index },
      transform: snapshotTransform(root.object),
      leaves,
    };
    attachSubtree(this.ctx, subtree);
    return { subtree };
  }

  /** Caller is responsible for deselecting before this runs. */
  deleteNodes(nodes: LevelSceneNode[]): void {
    const undoOps: StructuralOp[] = [];
    const redoOps: StructuralOp[] = [];
    const deletedNodes = new Set<LevelSceneNode>();
    const deletedIds: string[] = [];

    for (const node of nodes) {
      if (!isEditable(node)) continue;
      const subtree = detachSubtree(this.ctx, node);
      undoOps.push({ type: 'attach_subtree', subtree });
      redoOps.push({ type: 'detach_subtree', subtree });
      deletedNodes.add(node);
      deletedIds.push(node.id);
    }

    if (undoOps.length === 0) return;

    this.undoPurge(
      (e: any) => e.type === 'transform' && e.entries?.some((te: any) => deletedNodes.has(te.node))
    );
    this.undoPush({ type: 'structural', undoOps: [...undoOps].reverse(), redoOps });

    void this.enqueueStructuralSync(async () => {
      for (const id of deletedIds) {
        await this.api.sendDelete(id);
      }
    });
  }

  /** Caller is responsible for deselecting before this runs. */
  async groupNodes(editableNodes: LevelSceneNode[]): Promise<LevelGroup | null> {
    // Capture placements before any mutation so indices remain valid.
    const placements = editableNodes.map(n => ({ node: n, placement: capturePlacement(this.ctx, n) }));
    const insertIndex = Math.min(...placements.map(p => p.placement.index));
    const sharedParent = placements[0].placement.parent;

    const centroid = new THREE.Vector3();
    for (const node of editableNodes) {
      centroid.add(node.object.getWorldPosition(new THREE.Vector3()));
    }
    centroid.divideScalar(editableNodes.length);

    const groupParentObj: THREE.Object3D | null =
      sharedParent.type === 'root'
        ? this.ctx.viz.scene
        : ((this.ctx.nodeById.get(sharedParent.groupId) as LevelGroup | undefined)?.object ?? null);
    if (!groupParentObj) return null;

    groupParentObj.updateMatrixWorld(true);
    const localCentroid = groupParentObj.worldToLocal(centroid.clone());
    const requestedPosition: [number, number, number] = [
      round(localCentroid.x),
      round(localCentroid.y),
      round(localCentroid.z),
    ];

    const newDef = await this.api.groupNodes(
      editableNodes.map(n => n.id),
      requestedPosition
    );
    if (!newDef) return null;

    const groupOrigin = new THREE.Vector3().fromArray(newDef.position ?? requestedPosition);

    const detachedSubtrees: RuntimeSubtree[] = [];
    for (const node of editableNodes) {
      detachedSubtrees.push(detachSubtree(this.ctx, node));
    }

    // Subtract the new group origin so each child's world position is preserved
    // when it's re-parented under the group.
    for (const sub of detachedSubtrees) {
      sub.root.object.position.sub(groupOrigin);
      sub.root.object.position.set(
        round(sub.root.object.position.x),
        round(sub.root.object.position.y),
        round(sub.root.object.position.z)
      );
      sub.root.def.position = sub.root.object.position.toArray() as [number, number, number];
    }

    const groupObj = new THREE.Group();
    groupObj.position.fromArray(newDef.position ?? [0, 0, 0]);

    for (const sub of detachedSubtrees) {
      groupObj.add(sub.root.object);
    }

    // Strip `children` from the runtime def — the hierarchy lives only on `LevelGroup.children`.
    const { children: _omitChildren, ...body } = newDef;
    const levelGroup: LevelGroup = {
      id: newDef.id,
      object: groupObj,
      def: body,
      children: detachedSubtrees.map(s => s.root),
      generated: false,
    };

    const allLeaves = detachedSubtrees.flatMap(s => s.leaves);
    const groupSubtree: RuntimeSubtree = {
      root: levelGroup,
      placement: { parent: sharedParent, index: insertIndex },
      transform: snapshotTransform(groupObj),
      leaves: allLeaves,
    };
    attachSubtree(this.ctx, groupSubtree);

    // Undo: detach group, re-attach each child at its original placement (reversed).
    // Redo: detach children, attach group.
    const undoOps: StructuralOp[] = [
      { type: 'detach_subtree', subtree: groupSubtree },
      ...detachedSubtrees.map(s => ({ type: 'attach_subtree' as const, subtree: s })).reverse(),
    ];
    const redoOps: StructuralOp[] = [
      ...detachedSubtrees.map(s => ({ type: 'detach_subtree' as const, subtree: s })),
      { type: 'attach_subtree', subtree: groupSubtree },
    ];
    this.undoPush({ type: 'structural', undoOps, redoOps });

    return levelGroup;
  }

  /**
   * Reparent nodes to `targetParentId` (or to root if null), preserving world-space
   * transforms. Caller validates that the nodes are non-generated, non-circular, and
   * that the target is a valid group.
   */
  async reparentNodes(validNodes: LevelSceneNode[], targetParentId: string | null): Promise<void> {
    const targetParent = targetParentId ? (this.ctx.nodeById.get(targetParentId) as LevelGroup) : null;

    const worldTransforms = validNodes.map(node => {
      const worldPos = new THREE.Vector3();
      const worldQuat = new THREE.Quaternion();
      const worldScale = new THREE.Vector3();
      node.object.getWorldPosition(worldPos);
      node.object.getWorldQuaternion(worldQuat);
      node.object.getWorldScale(worldScale);
      return { worldPos, worldQuat, worldScale };
    });

    const detachedSubtrees: RuntimeSubtree[] = [];
    for (const node of validNodes) {
      detachedSubtrees.push(detachSubtree(this.ctx, node));
    }

    const targetChildren: LevelSceneNode[] = targetParentId ? targetParent!.children : this.ctx.rootNodes;
    const insertionIndex = targetChildren.length;

    for (let i = 0; i < detachedSubtrees.length; i++) {
      const sub = detachedSubtrees[i];
      const { worldPos, worldQuat, worldScale } = worldTransforms[i];
      const node = sub.root;

      const newParentRef: ParentRef = targetParentId
        ? { type: 'group', groupId: targetParentId }
        : { type: 'root' };
      sub.placement = { parent: newParentRef, index: insertionIndex + i };

      attachSubtree(this.ctx, sub);

      // Restore the world-space transform, re-expressed in the new parent's local frame.
      const parent = node.object.parent ?? this.ctx.viz.scene;
      parent.updateMatrixWorld();
      const parentInv = parent.matrixWorld.clone().invert();

      node.object.position.copy(worldPos).applyMatrix4(parentInv);

      const parentWorldQuat = new THREE.Quaternion();
      parent.getWorldQuaternion(parentWorldQuat);
      node.object.quaternion.copy(parentWorldQuat.invert()).multiply(worldQuat);

      const parentWorldScale = new THREE.Vector3();
      parent.getWorldScale(parentWorldScale);
      node.object.scale.set(
        worldScale.x / parentWorldScale.x,
        worldScale.y / parentWorldScale.y,
        worldScale.z / parentWorldScale.z
      );

      node.def.position = node.object.position.toArray().map(round) as [number, number, number];
      node.def.rotation = [
        round(node.object.rotation.x),
        round(node.object.rotation.y),
        round(node.object.rotation.z),
      ];
      node.def.scale = node.object.scale.toArray().map(round) as [number, number, number];

      node.object.updateMatrixWorld(true);

      for (const leaf of collectSubtreeLeaves(node)) {
        this.ctx.syncPhysics(leaf);
      }
    }

    // Snapshot the new placements for undo/redo by detach-then-reattach.
    const newSubtrees = validNodes.map(node => {
      const subtree = detachSubtree(this.ctx, node);
      attachSubtree(this.ctx, subtree);
      return subtree;
    });

    const undoOps: StructuralOp[] = [
      ...newSubtrees.map(s => ({ type: 'detach_subtree' as const, subtree: s })).reverse(),
      ...detachedSubtrees.map(s => ({ type: 'attach_subtree' as const, subtree: s })).reverse(),
    ];
    const redoOps: StructuralOp[] = [
      ...detachedSubtrees.map(s => ({ type: 'detach_subtree' as const, subtree: s })),
      ...newSubtrees.map(s => ({ type: 'attach_subtree' as const, subtree: s })),
    ];
    this.undoPush({ type: 'structural', undoOps, redoOps });

    void this.enqueueStructuralSync(() =>
      this.api
        .reparentNodes(
          validNodes.map(node => ({ id: node.id, transform: snapshotTransform(node.object) })),
          targetParentId ?? undefined,
          insertionIndex
        )
        .then(() => undefined)
    );
  }

  private _finalizeSpawnNode(root: LevelSceneNode): LevelSceneNode {
    const subtree: RuntimeSubtree = {
      root,
      placement: { parent: { type: 'root' }, index: this.ctx.rootNodes.length },
      transform: snapshotTransform(root.object),
      leaves: collectSubtreeLeaves(root),
    };
    attachSubtree(this.ctx, subtree);
    this.undoPush({
      type: 'structural',
      undoOps: [{ type: 'detach_subtree', subtree }],
      redoOps: [{ type: 'attach_subtree', subtree }],
    });
    return root;
  }
}
