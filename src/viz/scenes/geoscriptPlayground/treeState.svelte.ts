// Svelte-reactive wrapper around a `TreeDef` and its mutation surface. Logic
// lives in `treeOps.ts` (pure, plain-node testable); this file adds the
// `$state` reactivity, selection/solo tracking, dirty-vs-saved bookkeeping,
// and action-based undo/redo.
//
// Undo policy: structural ops (createNode, deleteNode, reparent) push entries
// themselves; `setInstanceTransform` is bare and the gizmo/inspector calls
// `recordInstanceTransformChange` once per gesture. Source edits, globals
// source, disable, and renames are not tracked here.

import type { ControlValue, GizmoValue, Instance, Transform3, TreeDef } from 'src/geoscript/geotoyAPIClient';
import { cloneTransform3 } from 'src/geoscript/geotoyAPIClient';

/**
 * Sentinel selection id for the tree's `globalsSource` editor scope. Picked to
 * intentionally collide with the reserved module name `_globals` so a single
 * `selectedId` field can represent both per-node and "edit globals" selection
 * without an extra kind discriminator.
 */
export const GLOBALS_SELECTION_ID = '_globals';

import {
  addInstance as opsAddInstance,
  createNode as opsCreateNode,
  deleteNode as opsDeleteNode,
  emptyTree,
  findParentId,
  deleteHandle as opsDeleteHandle,
  pruneHandles as opsPruneHandles,
  setControl as opsSetControl,
  deleteControl as opsDeleteControl,
  pruneControls as opsPruneControls,
  removeInstance as opsRemoveInstance,
  renameNode as opsRenameNode,
  reparent as opsReparent,
  setDisabled as opsSetDisabled,
  setHandle as opsSetHandle,
  setGlobalsSource as opsSetGlobalsSource,
  setSource as opsSetSource,
  setInstanceTransform as opsSetInstanceTransform,
  type CreateNodeOpts,
} from './treeOps';
import {
  applyGeotoyUndoEntry,
  buildGeotoyUndoSystem,
  captureSubtreeNodes,
  type GeotoyUndoSystem,
} from './treeUndoSystem';

export interface TreeStateOpts {
  /** Initial tree (e.g. from server load or migrated single-node tree). Cloned. */
  initial: TreeDef;
  /**
   * Baseline used for dirty tracking. Defaults to `initial`. Pass the server-side
   * tree when the in-memory tree is being restored from localStorage so dirty
   * compares correctly against the upstream version, not the local draft.
   */
  savedBaseline?: TreeDef;
}

const transformsEqual = (a: Transform3, b: Transform3): boolean =>
  a.pos[0] === b.pos[0] &&
  a.pos[1] === b.pos[1] &&
  a.pos[2] === b.pos[2] &&
  a.rot[0] === b.rot[0] &&
  a.rot[1] === b.rot[1] &&
  a.rot[2] === b.rot[2] &&
  a.scale[0] === b.scale[0] &&
  a.scale[1] === b.scale[1] &&
  a.scale[2] === b.scale[2];

// Sort `nodes` keys so dirty-detection survives delete+restore undo, which
// reshuffles insertion order but produces an otherwise-identical tree.
const stableSerializeTree = (tree: TreeDef): string => {
  const nodes: Record<string, unknown> = {};
  for (const id of Object.keys(tree.nodes).sort()) nodes[id] = tree.nodes[id];
  return JSON.stringify({
    version: tree.version,
    rootId: tree.rootId,
    globalsSource: tree.globalsSource,
    nodes,
  });
};

export class TreeState {
  /** Reactive container — mutations to `state.tree`, `state.selectedId`, etc.
   *  propagate through Svelte 5's deep `$state` proxy. */
  readonly state: { tree: TreeDef; selectedId: string | null; soloId: string | null } = $state({
    tree: emptyTree(),
    selectedId: null,
    soloId: null,
  });

  /** JSON snapshot of the tree at the last "saved" point. Compared against the
   *  live tree on `isDirty()`. */
  private savedSnapshotJson: string;

  readonly undoSystem: GeotoyUndoSystem = buildGeotoyUndoSystem();

  constructor(opts: TreeStateOpts) {
    this.state.tree = $state.snapshot(opts.initial) as TreeDef;
    this.savedSnapshotJson = stableSerializeTree(
      $state.snapshot(opts.savedBaseline ?? opts.initial) as TreeDef
    );
  }

  private applyUndoEntry = (
    entry: Parameters<typeof applyGeotoyUndoEntry>[1],
    direction: 'undo' | 'redo'
  ) => {
    const res = applyGeotoyUndoEntry(this.state.tree, entry, direction);
    this.applySelectAfter(res.selectAfter);
  };

  undo(): boolean {
    return this.undoSystem.undo(this.applyUndoEntry);
  }

  redo(): boolean {
    return this.undoSystem.redo(this.applyUndoEntry);
  }

  private applySelectAfter(selectAfter: string | undefined): void {
    if (selectAfter === undefined) return;
    if (this.state.tree.nodes[selectAfter]) {
      this.state.selectedId = selectAfter;
    } else {
      this.state.selectedId = this.state.tree.rootId;
    }
    if (this.state.soloId !== null && !this.state.tree.nodes[this.state.soloId]) {
      this.state.soloId = null;
    }
  }

  /** Plain-object snapshot of the current tree (Svelte $state.snapshot, deeply). */
  serialize(): TreeDef {
    return $state.snapshot(this.state.tree) as TreeDef;
  }

  isDirty(): boolean {
    return stableSerializeTree($state.snapshot(this.state.tree) as TreeDef) !== this.savedSnapshotJson;
  }

  /** Record the current tree as the saved baseline. Call after a successful save. */
  markSaved(): void {
    this.savedSnapshotJson = stableSerializeTree($state.snapshot(this.state.tree) as TreeDef);
  }

  /** Replace the entire tree (e.g. on "clear local changes" or fork). Clears
   *  selection/solo and resets the dirty baseline to the new tree. Discards
   *  undo history — entries from the previous tree would be incoherent. */
  replaceTree(tree: TreeDef): void {
    const snap = $state.snapshot(tree) as TreeDef;
    this.state.tree = snap;
    this.state.selectedId = null;
    this.state.soloId = null;
    this.savedSnapshotJson = stableSerializeTree(snap);
    this.undoSystem.clear();
  }

  createNode(opts: CreateNodeOpts = {}): string {
    const id = opsCreateNode(this.state.tree, opts);
    const parentId = opts.parentId ?? this.state.tree.rootId;
    const parent = this.state.tree.nodes[parentId];
    const index = parent.children.indexOf(id);
    const nodeDef = $state.snapshot(this.state.tree.nodes[id]);
    this.undoSystem.push({ type: 'createNode', nodeDef, parentId, index });
    return id;
  }

  /** Refuses to delete `_root`. Every tree has exactly one root, always present. */
  canDelete(id: string): boolean {
    const tree = this.state.tree;
    if (!tree.nodes[id]) return false;
    return id !== tree.rootId;
  }

  deleteNode(id: string): void {
    if (!this.canDelete(id)) return;
    const tree = this.state.tree;
    const parentId = findParentId(tree, id);
    if (!parentId) return;
    const parent = tree.nodes[parentId];
    const index = parent.children.indexOf(id);
    const nodes = captureSubtreeNodes($state.snapshot(tree) as TreeDef, id);

    opsDeleteNode(tree, id);

    this.undoSystem.push({ type: 'deleteSubtree', rootId: id, nodes, parentId, index });

    const sel = this.state.selectedId;
    if (sel !== null && sel !== GLOBALS_SELECTION_ID) {
      if (sel === id || !tree.nodes[sel]) this.state.selectedId = tree.rootId;
    }
    if (this.state.soloId === id || (this.state.soloId && !tree.nodes[this.state.soloId])) {
      this.state.soloId = null;
    }
  }

  reparent(id: string, newParentId: string | null, index?: number): void {
    const tree = this.state.tree;
    const oldParentId = findParentId(tree, id);
    if (oldParentId === null) return;
    const oldIndex = tree.nodes[oldParentId].children.indexOf(id);

    opsReparent(tree, id, newParentId, index);

    const effectiveNewParentId = newParentId ?? tree.rootId;
    const newIndex = tree.nodes[effectiveNewParentId].children.indexOf(id);
    if (oldParentId === effectiveNewParentId && oldIndex === newIndex) return;

    this.undoSystem.push({
      type: 'reparent',
      id,
      oldParentId,
      oldIndex,
      newParentId: effectiveNewParentId,
      newIndex,
    });
  }

  rename(id: string, newName: string): void {
    opsRenameNode(this.state.tree, id, newName);
  }

  /** Does NOT push undo. Pair with `recordInstanceTransformChange` on gesture commit. */
  setInstanceTransform(nodeId: string, instanceId: string, transform: Transform3): void {
    opsSetInstanceTransform(this.state.tree, nodeId, instanceId, transform);
  }

  /** Plain-value snapshot of one instance's transform, for capturing the `before`
   *  of a drag/edit gesture before subsequent `setInstanceTransform` mutations. */
  captureInstanceTransform(nodeId: string, instanceId: string): Transform3 | null {
    const t = this.state.tree.nodes[nodeId]?.instances.find(i => i.id === instanceId);
    return t ? cloneTransform3(t) : null;
  }

  recordInstanceTransformChange(
    nodeId: string,
    instanceId: string,
    before: Transform3,
    after: Transform3
  ): void {
    if (!this.state.tree.nodes[nodeId]?.instances.some(i => i.id === instanceId)) return;
    if (transformsEqual(before, after)) return;
    this.undoSystem.push({
      type: 'transform',
      id: nodeId,
      instanceId,
      before: cloneTransform3(before),
      after: cloneTransform3(after),
    });
  }

  /** Appends a placement (undo-tracked); returns its id, or null for `_root`/missing. */
  addInstance(nodeId: string, seed?: Transform3): string | null {
    const newId = opsAddInstance(this.state.tree, nodeId, seed);
    if (newId === null) return null;
    const inst = this.state.tree.nodes[nodeId].instances.find(i => i.id === newId)!;
    this.undoSystem.push({ type: 'addInstance', nodeId, instance: $state.snapshot(inst) as Instance });
    return newId;
  }

  /** Removes a placement by id (undo-tracked). Refuses the last instance / `_root`. */
  removeInstance(nodeId: string, instanceId: string): void {
    const node = this.state.tree.nodes[nodeId];
    if (!node || nodeId === this.state.tree.rootId || node.instances.length <= 1) return;
    const index = node.instances.findIndex(i => i.id === instanceId);
    if (index < 0) return;
    const instance = $state.snapshot(node.instances[index]) as Instance;
    opsRemoveInstance(this.state.tree, nodeId, instanceId);
    this.undoSystem.push({ type: 'removeInstance', nodeId, instance, index });
  }

  /** Does NOT push undo. Pair with `recordHandleChange` on gesture commit. */
  setHandle(nodeId: string, handleId: string, value: GizmoValue): void {
    opsSetHandle(this.state.tree, nodeId, handleId, value);
  }

  /** Plain-value snapshot of one handle's stored value (or null), for capturing a gesture's `before`. */
  captureHandle(nodeId: string, handleId: string): GizmoValue | null {
    const v = this.state.tree.nodes[nodeId]?.handles?.[handleId];
    return v ? (structuredClone($state.snapshot(v)) as GizmoValue) : null;
  }

  recordHandleChange(
    nodeId: string,
    handleId: string,
    before: GizmoValue | null,
    after: GizmoValue | null
  ): void {
    if (!this.state.tree.nodes[nodeId]) return;
    if (JSON.stringify(before) === JSON.stringify(after)) return; // skip no-op (e.g. click without drag)
    this.undoSystem.push({ type: 'setHandle', nodeId, handleId, before, after });
  }

  /** GC orphaned handle values (no undo entry — automatic cleanup on run/save). */
  pruneHandles(nodeId: string, liveHandleIds: ReadonlySet<string>): void {
    opsPruneHandles(this.state.tree, nodeId, liveHandleIds);
  }

  deleteHandle(nodeId: string, handleId: string): void {
    opsDeleteHandle(this.state.tree, nodeId, handleId);
  }

  /** Does NOT push undo. Pair with `recordControlChange` to commit an undo entry. */
  setControl(nodeId: string, handleId: string, value: ControlValue): void {
    opsSetControl(this.state.tree, nodeId, handleId, value);
  }

  captureControl(nodeId: string, handleId: string): ControlValue | null {
    const v = this.state.tree.nodes[nodeId]?.controls?.[handleId];
    return v ? (structuredClone($state.snapshot(v)) as ControlValue) : null;
  }

  recordControlChange(
    nodeId: string,
    handleId: string,
    before: ControlValue | null,
    after: ControlValue | null
  ): void {
    if (!this.state.tree.nodes[nodeId]) return;
    if (JSON.stringify(before) === JSON.stringify(after)) return;
    this.undoSystem.push({ type: 'setControl', nodeId, handleId, before, after });
  }

  /** GC orphaned control values (no undo entry — automatic cleanup on run/save). */
  pruneControls(nodeId: string, liveHandleIds: ReadonlySet<string>): void {
    opsPruneControls(this.state.tree, nodeId, liveHandleIds);
  }

  deleteControl(nodeId: string, handleId: string): void {
    opsDeleteControl(this.state.tree, nodeId, handleId);
  }

  setSource(id: string, source: string): void {
    opsSetSource(this.state.tree, id, source);
  }

  setDisabled(id: string, disabled: boolean): void {
    opsSetDisabled(this.state.tree, id, disabled);
  }

  setGlobalsSource(source: string): void {
    opsSetGlobalsSource(this.state.tree, source);
  }

  setSelected(id: string | null): void {
    const next =
      id === null || id === GLOBALS_SELECTION_ID
        ? id
        : this.state.tree.nodes[id]
          ? id
          : this.state.tree.rootId;
    if (this.state.selectedId !== next) this.state.selectedId = next;
  }

  get isGlobalsSelected(): boolean {
    return this.state.selectedId === GLOBALS_SELECTION_ID;
  }

  setSolo(id: string | null): void {
    // Soloing _root is equivalent to no solo at all.
    const next = id === null || id === this.state.tree.rootId || !this.state.tree.nodes[id] ? null : id;
    if (this.state.soloId !== next) this.state.soloId = next;
  }
}
