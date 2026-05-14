// Svelte-reactive wrapper around a `TreeDef` and its mutation surface. Logic
// lives in `treeOps.ts` (pure, plain-node testable); this file only adds the
// `$state` reactivity, selection/solo tracking, and dirty-vs-saved bookkeeping.
//
// Persistence: the saved-tree baseline used for dirty detection comes in via the
// constructor (e.g. from the server-provided composition version), and is updated
// via `markSaved()` after a successful save. `serialize()` returns a structural
// snapshot suitable for sending to the API or writing to localStorage.

import type { Transform3, TreeDef } from 'src/geoscript/geotoyAPIClient';

/**
 * Sentinel selection id for the tree's `globalsSource` editor scope. Picked to
 * intentionally collide with the reserved module name `_globals` so a single
 * `selectedId` field can represent both per-node and "edit globals" selection
 * without an extra kind discriminator.
 */
export const GLOBALS_SELECTION_ID = '_globals';

import {
  createNode as opsCreateNode,
  deleteNode as opsDeleteNode,
  emptyTree,
  renameNode as opsRenameNode,
  reparent as opsReparent,
  setDisabled as opsSetDisabled,
  setGlobalsSource as opsSetGlobalsSource,
  setSource as opsSetSource,
  setTransform as opsSetTransform,
  type CreateNodeOpts,
} from './treeOps';
import { TreeUndoSystem, type TreeSnapshot } from './treeUndoSystem';

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

  /** Tree-snapshot undo/redo stack. See `treeUndoSystem.ts` for semantics. */
  readonly undoSystem = new TreeUndoSystem();

  constructor(opts: TreeStateOpts) {
    this.state.tree = structuredClone(opts.initial);
    this.savedSnapshotJson = JSON.stringify(opts.savedBaseline ?? opts.initial);
  }

  private currentSnapshot(): TreeSnapshot {
    return {
      tree: structuredClone($state.snapshot(this.state.tree)) as TreeDef,
      selectedId: this.state.selectedId,
      soloId: this.state.soloId,
    };
  }

  /** Run `op` and snapshot one undo entry. Pass `null` for atomic edits; pass a
   *  stable key (e.g. `transform:<id>`) to coalesce rapid bursts.
   *
   *  Hot path: a gizmo drag fires this ~60×/sec. Snapshotting `before` is a
   *  full-tree clone — skipped when we know we'll coalesce, since the existing
   *  entry's `before` is the one we want to keep. */
  applyEdit(coalesceKey: string | null, op: () => void): void {
    const before = this.undoSystem.wouldCoalesce(coalesceKey) ? null : this.currentSnapshot();
    op();
    const after = this.currentSnapshot();
    this.undoSystem.push(coalesceKey, before, after);
  }

  undo(): boolean {
    const snap = this.undoSystem.undo();
    if (!snap) return false;
    this.restoreSnapshot(snap);
    return true;
  }

  redo(): boolean {
    const snap = this.undoSystem.redo();
    if (!snap) return false;
    this.restoreSnapshot(snap);
    return true;
  }

  private restoreSnapshot(snap: TreeSnapshot): void {
    this.state.tree = structuredClone(snap.tree);
    // Defensive: ids should still exist (snapshot was taken with this tree).
    if (snap.selectedId === null || snap.selectedId === GLOBALS_SELECTION_ID) {
      this.state.selectedId = snap.selectedId;
    } else {
      this.state.selectedId = this.state.tree.nodes[snap.selectedId]
        ? snap.selectedId
        : this.state.tree.rootId;
    }
    this.state.soloId = snap.soloId !== null && this.state.tree.nodes[snap.soloId] ? snap.soloId : null;
  }

  /** Plain-object snapshot of the current tree (Svelte $state.snapshot, deeply). */
  serialize(): TreeDef {
    return structuredClone($state.snapshot(this.state.tree)) as TreeDef;
  }

  isDirty(): boolean {
    return JSON.stringify($state.snapshot(this.state.tree)) !== this.savedSnapshotJson;
  }

  /** Record the current tree as the saved baseline. Call after a successful save. */
  markSaved(): void {
    this.savedSnapshotJson = JSON.stringify($state.snapshot(this.state.tree));
  }

  /** Replace the entire tree (e.g. on "clear local changes" or fork). Clears
   *  selection/solo and resets the dirty baseline to the new tree. Discards
   *  undo history — entries from the previous tree would be incoherent. */
  replaceTree(tree: TreeDef): void {
    this.state.tree = structuredClone(tree);
    this.state.selectedId = null;
    this.state.soloId = null;
    this.savedSnapshotJson = JSON.stringify(tree);
    this.undoSystem.clear();
  }

  createNode(opts: CreateNodeOpts = {}): string {
    return opsCreateNode(this.state.tree, opts);
  }

  /** Refuses to delete `_root`. Every tree has exactly one root, always present. */
  canDelete(id: string): boolean {
    const tree = this.state.tree;
    if (!tree.nodes[id]) return false;
    return id !== tree.rootId;
  }

  deleteNode(id: string): void {
    if (!this.canDelete(id)) return;
    opsDeleteNode(this.state.tree, id);
    const sel = this.state.selectedId;
    if (sel !== null && sel !== GLOBALS_SELECTION_ID) {
      if (sel === id || !this.state.tree.nodes[sel]) {
        this.state.selectedId = this.state.tree.rootId;
      }
    }
    if (this.state.soloId === id || (this.state.soloId && !this.state.tree.nodes[this.state.soloId])) {
      this.state.soloId = null;
    }
  }

  reparent(id: string, newParentId: string | null, index?: number): void {
    opsReparent(this.state.tree, id, newParentId, index);
  }

  rename(id: string, newName: string): void {
    opsRenameNode(this.state.tree, id, newName);
  }

  setTransform(id: string, transform: Transform3): void {
    opsSetTransform(this.state.tree, id, transform);
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
    if (id === null) {
      this.state.selectedId = null;
      return;
    }
    if (id === GLOBALS_SELECTION_ID) {
      this.state.selectedId = GLOBALS_SELECTION_ID;
      return;
    }
    this.state.selectedId = this.state.tree.nodes[id] ? id : null;
  }

  get isGlobalsSelected(): boolean {
    return this.state.selectedId === GLOBALS_SELECTION_ID;
  }

  setSolo(id: string | null): void {
    if (id === this.state.tree.rootId) {
      // Soloing _root is equivalent to no solo at all.
      this.state.soloId = null;
      return;
    }
    this.state.soloId = id !== null && this.state.tree.nodes[id] ? id : null;
  }
}
