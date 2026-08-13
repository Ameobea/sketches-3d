// The composition's trees, each with a *live* `TreeState`. Keeping every tree
// instantiated (rather than serializing on switch) is what preserves per-tab undo,
// selection and dirty state — and it's why `currentDoc()` must fold all of them.
//
// "tab" names the UI affordance, "tree" names the stored entity; the wire format keeps
// `CompositionDoc.trees`.

import {
  buildEmptyTree,
  type CompositionDoc,
  type TreeEntry,
  type TreeKind,
} from 'src/geoscript/geotoyAPIClient';
import { TreeState } from './treeState.svelte';
import { NAME_RE } from './treeOps';

const DEFAULT_BASE: Record<TreeKind, string> = { mesh: 'scene', texture: 'texture' };

export interface GeotoyTab {
  readonly id: string;
  readonly kind: TreeKind;
  name: string;
  readonly treeState: TreeState;
}

/**
 * Container-level shape: everything about the tab set that isn't tree *content*.
 * Add/delete/rename/reorder move no tree's dirty flag, so this is what makes them
 * register as unsaved changes.
 */
export const tabShapeKey = (entries: readonly { id: string; kind: string; name: string }[]): string =>
  entries.map(e => `${e.id}\u0000${e.kind}\u0000${e.name}`).join('\u0001');

interface GeotoyTabsOpts {
  doc: CompositionDoc;
  /** Server doc when one exists: per-tab dirty baselines come from it, so a draft restored
   *  from localStorage compares against the upstream version rather than against itself. */
  serverDoc: CompositionDoc | null;
  getActiveId: () => string;
}

const buildTabs = (doc: CompositionDoc, serverDoc: CompositionDoc | null): GeotoyTab[] =>
  doc.trees.map(entry => {
    const treeState = new TreeState({
      initial: entry.tree,
      savedBaseline: serverDoc?.trees.find(t => t.id === entry.id)?.tree ?? entry.tree,
    });
    treeState.setSelected(entry.tree.rootId);
    return { id: entry.id, kind: entry.kind, name: entry.name, treeState };
  });

export class GeotoyTabs {
  tabs = $state.raw<GeotoyTab[]>([]);
  private readonly getActiveId: () => string;

  constructor(opts: GeotoyTabsOpts) {
    this.getActiveId = opts.getActiveId;
    this.tabs = buildTabs(opts.doc, opts.serverDoc);
  }

  /** Falls back to the first tab so a stale/missing active id can never leave this null. */
  readonly active = $derived(this.tabs.find(t => t.id === this.getActiveId()) ?? this.tabs[0]);

  serialize = (): TreeEntry[] =>
    this.tabs.map(t => ({ id: t.id, kind: t.kind, name: t.name, tree: t.treeState.serialize() }));

  get shapeKey(): string {
    return tabShapeKey(this.tabs);
  }

  get anyDirty(): boolean {
    return this.tabs.some(t => t.treeState.treeDirty);
  }

  markAllSaved(): void {
    for (const t of this.tabs) t.treeState.markSaved();
  }

  /**
   * Appends a tab and returns its id. The id is a slug derived from the initial name and is
   * **stable across renames** — changing it would churn every module key and invalidate the
   * interpreter's per-module cache on a purely cosmetic edit.
   */
  create(kind: TreeKind): string {
    const base = DEFAULT_BASE[kind];
    const taken = new Set(this.tabs.map(t => t.id));
    let id = base;
    for (let n = 2; taken.has(id); n += 1) id = `${base}_${n}`;
    if (!NAME_RE.test(id)) throw new Error(`generated tab id is not a valid module prefix: ${id}`);

    const tree = buildEmptyTree();
    const treeState = new TreeState({ initial: tree });
    treeState.setSelected(tree.rootId);
    this.tabs = [...this.tabs, { id, kind, name: id, treeState }];
    return id;
  }

  /** Refuses the last tab — a composition always has ≥1 tree. */
  canDelete(id: string): boolean {
    return this.tabs.length > 1 && this.tabs.some(t => t.id === id);
  }

  /** Returns the id to activate next when the deleted tab was active. */
  remove(id: string): string {
    const ix = this.tabs.findIndex(t => t.id === id);
    if (ix < 0 || !this.canDelete(id)) return this.getActiveId();
    this.tabs = this.tabs.filter(t => t.id !== id);
    return this.tabs[Math.min(ix, this.tabs.length - 1)].id;
  }

  /** Display name only; the id (and therefore the module namespace) is untouched. */
  rename(id: string, name: string): void {
    const trimmed = name.trim();
    if (!trimmed) return;
    this.tabs = this.tabs.map(t => (t.id === id ? { ...t, name: trimmed } : t));
  }

  /**
   * Discard the entire local changeset and rebuild from `doc`. Rebuilds the tab *set*, not
   * just each tree's content — locally-created tabs disappear and locally-deleted ones come
   * back, which patching by id could never do. Undo stacks go with them, which is correct
   * for a revert.
   */
  resetFromDoc(doc: CompositionDoc): void {
    this.tabs = buildTabs(doc, doc);
  }
}
