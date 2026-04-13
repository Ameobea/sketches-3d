import * as THREE from 'three';

import type { LevelObject, LevelGroup, LevelSceneNode } from './levelSceneTypes';
import type { ObjectDef, ObjectGroupDef } from './types';
import type { StructuralCtx, RuntimeSubtree, StructuralOp, ParentRef } from './editorStructuralTypes';
import type { BuildCtx } from './editorNodeFactory';
import type { LevelEditorApi } from './levelEditorApi';
import { buildLeafNode, buildGroupSubtree } from './editorNodeFactory';
import {
  attachSubtree,
  detachSubtree,
  capturePlacement,
  collectSubtreeLeaves,
  applyStructuralOp,
} from './editorStructuralOps';
import { snapshotTransform } from './TransformHandler';
import { flattenLeaves } from './levelDefTreeUtils';
import { round } from './mathUtils';

export type StructuralUndoEntry = {
  type: 'structural';
  undoOps: StructuralOp[];
  redoOps: StructuralOp[];
};

type MutationCtx = StructuralCtx & BuildCtx;

/**
 * Coordinator for all structural mutation workflows in the level editor.
 *
 * Sits above `editorStructuralOps.ts` and owns:
 * - applying structural ops locally
 * - deriving persistence actions from structural ops
 * - sequencing structural server sync
 * - building compound structural undo entries for higher-level operations
 *
 * LevelEditor delegates structural operations here and handles selection / UI
 * state updates based on the returned results.
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

  // ---------------------------------------------------------------------------
  // Persistence queueing
  // ---------------------------------------------------------------------------

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

  // ---------------------------------------------------------------------------
  // Undo application
  // ---------------------------------------------------------------------------

  /**
   * Apply the ops from a structural undo/redo entry and enqueue persistence.
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

  // ---------------------------------------------------------------------------
  // Spawn / paste operations
  // ---------------------------------------------------------------------------

  /**
   * Spawn a new leaf object at the given position.
   * Handles the server call, local attach, and undo registration.
   * Returns the new node or null on server failure.
   */
  async spawnLeaf(
    assetId: string,
    materialId: string | undefined,
    position: [number, number, number]
  ): Promise<LevelObject | null> {
    if (!this.ctx.prototypes.has(assetId)) {
      console.warn(`[EditorMutationController] No prototype for asset "${assetId}"`);
      return null;
    }
    const newDef = await this.api.sendAdd({ asset: assetId, material: materialId, position });
    if (!newDef) return null;
    return this._finalizeSpawnLeaf(assetId, newDef);
  }

  /**
   * Spawn a new empty group at the given position.
   * Handles the server call and local attach. No undo entry (matches original behavior).
   * Returns the new group or null on server failure.
   */
  async spawnGroup(position: [number, number, number]): Promise<LevelGroup | null> {
    const newDef = await this.api.sendAddGroup({ position });
    if (!newDef) return null;

    const groupObj = new THREE.Group();
    groupObj.position.fromArray(newDef.position ?? [0, 0, 0]);

    const levelGroup: LevelGroup = {
      id: newDef.id,
      object: groupObj,
      def: newDef as ObjectGroupDef,
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
   * Paste a leaf object from clipboard data.
   * Offsets the Y position slightly to avoid Z-fighting on the source.
   * Returns the new node or null on failure.
   */
  async pasteLeaf(assetId: string, def: ObjectDef): Promise<LevelObject | null> {
    if (!this.ctx.prototypes.has(assetId)) {
      console.warn(
        `[EditorMutationController] No prototype for asset "${assetId}" — asset may not be loaded yet`
      );
      return null;
    }
    const srcPos = def.position ?? [0, 0, 0];
    const position: [number, number, number] = [srcPos[0], srcPos[1] + 0.5, srcPos[2]];
    const newDef = await this.api.sendAdd({
      asset: assetId,
      material: def.material,
      position,
      rotation: def.rotation,
      scale: def.scale,
    });
    if (!newDef) return null;
    return this._finalizeSpawnLeaf(assetId, newDef);
  }

  /**
   * Paste a group subtree from clipboard data.
   * Offsets the Y position slightly and validates that all leaf prototypes are loaded.
   * Returns the new root group or null on failure.
   */
  async pasteGroup(def: ObjectGroupDef, worldPosition: [number, number, number]): Promise<LevelGroup | null> {
    const patchedDef: ObjectGroupDef = {
      ...JSON.parse(JSON.stringify(def)),
      position: [worldPosition[0], worldPosition[1] + 0.5, worldPosition[2]] as [number, number, number],
    };

    for (const leaf of flattenLeaves([patchedDef])) {
      if (!this.ctx.prototypes.has(leaf.asset)) {
        console.warn(`[EditorMutationController] No prototype for asset "${leaf.asset}" in pasted group`);
        return null;
      }
    }

    const newDef = await this.api.sendPasteGroup(patchedDef);
    if (!newDef) return null;

    const root = buildGroupSubtree(this.ctx, newDef);
    const leaves = collectSubtreeLeaves(root);
    const subtree: RuntimeSubtree = {
      root,
      placement: { parent: { type: 'root' }, index: this.ctx.rootNodes.length },
      transform: snapshotTransform(root.object),
      leaves,
    };
    attachSubtree(this.ctx, subtree);
    this.undoPush({
      type: 'structural',
      undoOps: [{ type: 'detach_subtree', subtree }],
      redoOps: [{ type: 'attach_subtree', subtree }],
    });
    return root;
  }

  // ---------------------------------------------------------------------------
  // Delete
  // ---------------------------------------------------------------------------

  /**
   * Detach and delete the given nodes. Purges stale transform undo entries.
   * The caller is responsible for deselecting before calling this.
   */
  deleteNodes(nodes: LevelSceneNode[]): void {
    const undoOps: StructuralOp[] = [];
    const redoOps: StructuralOp[] = [];
    const deletedNodes = new Set<LevelSceneNode>();
    const deletedIds: string[] = [];

    for (const node of nodes) {
      if (node.generated) continue;
      const subtree = detachSubtree(this.ctx, node);
      undoOps.push({ type: 'attach_subtree', subtree });
      redoOps.push({ type: 'detach_subtree', subtree });
      deletedNodes.add(node);
      deletedIds.push(node.id);
    }

    if (undoOps.length === 0) return;

    // Remove stale transform entries for deleted nodes before pushing the structural entry.
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

  // ---------------------------------------------------------------------------
  // Group / reparent
  // ---------------------------------------------------------------------------

  /**
   * Group a set of sibling nodes into a new parent group.
   * The caller is responsible for deselecting before calling this.
   * Returns the new group or null on server failure.
   */
  async groupNodes(editableNodes: LevelSceneNode[]): Promise<LevelGroup | null> {
    // Capture placements before any mutation so indices remain valid.
    const placements = editableNodes.map(n => ({ node: n, placement: capturePlacement(this.ctx, n) }));
    const insertIndex = Math.min(...placements.map(p => p.placement.index));
    const sharedParent = placements[0].placement.parent;

    // Compute the group origin in the shared parent's local space.
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

    // Detach each selected node. Collect subtrees in order for correct undo reversal.
    const detachedSubtrees: RuntimeSubtree[] = [];
    for (const node of editableNodes) {
      detachedSubtrees.push(detachSubtree(this.ctx, node));
    }

    // Adjust each node's local position relative to the new group origin so world
    // placement is preserved when they are re-parented under the group.
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

    const levelGroup: LevelGroup = {
      id: newDef.id,
      object: groupObj,
      def: newDef as ObjectGroupDef,
      children: detachedSubtrees.map(s => s.root),
      generated: false,
    };
    // Rebind def.children to live child def references so subsequent mutations
    // (transform, material) are reflected when the group is serialized.
    levelGroup.def.children = detachedSubtrees.map(s => s.root.def);

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
   * Reparent nodes to a new parent (or to root if targetParentId is null).
   * Preserves world-space transforms. The caller is responsible for pre-validating
   * that validNodes are non-generated, non-circular, and that the target is a valid group.
   */
  async reparentNodes(validNodes: LevelSceneNode[], targetParentId: string | null): Promise<void> {
    const targetParent = targetParentId ? (this.ctx.nodeById.get(targetParentId) as LevelGroup) : null;

    // Capture world transforms BEFORE detachment.
    const worldTransforms = validNodes.map(node => {
      const worldPos = new THREE.Vector3();
      const worldQuat = new THREE.Quaternion();
      const worldScale = new THREE.Vector3();
      node.object.getWorldPosition(worldPos);
      node.object.getWorldQuaternion(worldQuat);
      node.object.getWorldScale(worldScale);
      return { worldPos, worldQuat, worldScale };
    });

    // Detach all nodes.
    const detachedSubtrees: RuntimeSubtree[] = [];
    for (const node of validNodes) {
      detachedSubtrees.push(detachSubtree(this.ctx, node));
    }

    // Determine insertion index after detach (append to the target's current children).
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

      // Restore world-space transform expressed in the new parent's local space.
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

      // Re-sync physics for all leaves in the subtree (world transform changed).
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

  // ---------------------------------------------------------------------------
  // Internal helpers
  // ---------------------------------------------------------------------------

  private _finalizeSpawnLeaf(assetId: string, newDef: ObjectDef): LevelObject {
    const leaf = buildLeafNode(this.ctx, assetId, newDef);
    const subtree: RuntimeSubtree = {
      root: leaf,
      placement: { parent: { type: 'root' }, index: this.ctx.rootNodes.length },
      transform: snapshotTransform(leaf.object),
      leaves: [leaf],
    };
    attachSubtree(this.ctx, subtree);
    this.undoPush({
      type: 'structural',
      undoOps: [{ type: 'detach_subtree', subtree }],
      redoOps: [{ type: 'attach_subtree', subtree }],
    });
    return leaf;
  }
}
