// The composition's trees, each with a *live* `TreeState`. Keeping every tree
// instantiated (rather than serializing on switch) is what preserves per-tab undo,
// selection and dirty state — and it's why `currentDoc()` must fold all of them.
//
// "tab" names the UI affordance, "tree" names the stored entity; the wire format keeps
// `CompositionDoc.trees`.

import {
  buildLegacyRootTree,
  defaultTabMetadata,
  NAME_RE,
  type CompositionDoc,
  type EnvironmentConfig,
  type MeshTabView,
  type TabMetadata,
  type TabView,
  type TextureOutputGpuParams,
  type TextureOutputMeta,
  type TextureTabView,
  type TreeEntry,
  type TreeKind,
} from 'src/geoscript/geotoyAPIClient';
import { TreeState } from './treeState.svelte';

const DEFAULT_BASE: Record<TreeKind, string> = { mesh: 'scene', texture: 'texture' };

/** Fresh-tab root source, the texture analog of `DefaultCode`'s start cube. */
const STARTER_SOURCE: Record<TreeKind, string> = {
  mesh: '',
  texture: `n = 256

shade = input_color_ramp("shade", default=[[-1., srgb(0x89847B)], [0.2, srgb(0xB59F82)], [1., srgb(0xE6DED1)]])

diffuse = texture(n, n, |uv| fbm(octaves=5, frequency=3., pos=uv, tileable=true) | shade) | blur(0.8)

diffuse | render_texture(name="diffuse", usage="albedo")
`,
};

export interface GeotoyTab {
  readonly id: string;
  readonly kind: TreeKind;
  name: string;
  readonly treeState: TreeState;
  /** Kind-matched to the tab (mesh camera / texture pan-zoom). Mutated in place rather
   *  than through `patch`: it changes on every switch and nothing renders from it. */
  view: TabView | null;
  readonly preludeEjected: boolean;
  readonly environment?: EnvironmentConfig;
  /** Texture tabs: `render_texture` outputs from the tab's last run (persisted in tab
   *  metadata so a fresh load can offer them before the tab has executed). */
  readonly textureOutputs: readonly TextureOutputMeta[];
  /** Texture tabs: UI-owned per-output GPU params, keyed by output name. User content —
   *  edits dirty the composition and join the eval input hash. */
  readonly textureParams: Readonly<Record<string, TextureOutputGpuParams>>;
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
  /** Per-tab metadata for `doc`, already resolved draft-over-server. */
  tabMeta: Record<string, TabMetadata>;
  /** Server doc when one exists: per-tab dirty baselines come from it, so a draft restored
   *  from localStorage compares against the upstream version rather than against itself. */
  serverDoc: CompositionDoc | null;
  getActiveId: () => string;
}

const buildTabs = (
  doc: CompositionDoc,
  tabMeta: Record<string, TabMetadata>,
  serverDoc: CompositionDoc | null
): GeotoyTab[] =>
  doc.trees.map(entry => {
    const treeState = new TreeState({
      initial: entry.tree,
      savedBaseline: serverDoc?.trees.find(t => t.id === entry.id)?.tree ?? entry.tree,
    });
    treeState.setSelected(entry.tree.rootId);
    const meta = tabMeta[entry.id];
    return {
      id: entry.id,
      kind: entry.kind,
      name: entry.name,
      treeState,
      view: meta.view ?? null,
      preludeEjected: meta.preludeEjected,
      environment: meta.kind === 'mesh' ? meta.environment : undefined,
      textureOutputs: (meta.kind === 'texture' ? meta.textureOutputs : undefined) ?? [],
      textureParams: (meta.kind === 'texture' ? meta.textureParams : undefined) ?? {},
    };
  });

export class GeotoyTabs {
  tabs = $state.raw<GeotoyTab[]>([]);
  private readonly getActiveId: () => string;

  constructor(opts: GeotoyTabsOpts) {
    this.getActiveId = opts.getActiveId;
    this.tabs = buildTabs(opts.doc, opts.tabMeta, opts.serverDoc);
  }

  /** Falls back to the first tab so a stale/missing active id can never leave this null. */
  readonly active = $derived(this.tabs.find(t => t.id === this.getActiveId()) ?? this.tabs[0]);

  serialize = (): TreeEntry[] =>
    this.tabs.map(t => ({ id: t.id, kind: t.kind, name: t.name, tree: t.treeState.serialize() }));

  /**
   * Per-tab metadata for the *live* tab set. Rebuilt on every call rather than merged into a
   * stored record, so a deleted tab's entry can't survive anywhere.
   */
  metaRecord = (): Record<string, TabMetadata> =>
    Object.fromEntries(
      this.tabs.map(t => [
        t.id,
        t.kind === 'mesh'
          ? {
              kind: 'mesh' as const,
              preludeEjected: t.preludeEjected,
              view: (t.view as MeshTabView | null) ?? undefined,
              environment: t.environment,
            }
          : {
              kind: 'texture' as const,
              preludeEjected: t.preludeEjected,
              view: (t.view as TextureTabView | null) ?? undefined,
              textureOutputs: t.textureOutputs.length ? [...t.textureOutputs] : undefined,
              textureParams: Object.keys(t.textureParams).length ? { ...t.textureParams } : undefined,
            },
      ])
    );

  get shapeKey(): string {
    return tabShapeKey(this.tabs);
  }

  get anyDirty(): boolean {
    return this.tabs.some(t => t.treeState.treeDirty);
  }

  markAllSaved(): void {
    for (const t of this.tabs) t.treeState.markSaved();
  }

  /** Reassigns the array so menus, the eval hash and the dirty check see the change. */
  private patch(id: string, fields: Partial<GeotoyTab>): void {
    this.tabs = this.tabs.map(t => (t.id === id ? { ...t, ...fields } : t));
  }

  setPreludeEjected(id: string, ejected: boolean): void {
    this.patch(id, { preludeEjected: ejected });
  }

  setEnvironment(id: string, environment: EnvironmentConfig | undefined): void {
    this.patch(id, { environment });
  }

  /** Sync a texture tab's output index from a completed run; no-op when unchanged so
   *  routine reruns don't churn the tabs array. Also prunes params for vanished outputs. */
  setTextureOutputs(id: string, outputs: TextureOutputMeta[]): void {
    const tab = this.tabs.find(t => t.id === id);
    if (!tab) return;
    const live = new Set(outputs.map(o => o.name));
    const params = Object.fromEntries(Object.entries(tab.textureParams).filter(([name]) => live.has(name)));
    const outputsChanged = JSON.stringify(tab.textureOutputs) !== JSON.stringify(outputs);
    const paramsChanged = Object.keys(params).length !== Object.keys(tab.textureParams).length;
    if (!outputsChanged && !paramsChanged) return;
    this.patch(id, {
      ...(outputsChanged ? { textureOutputs: outputs } : {}),
      ...(paramsChanged ? { textureParams: params } : {}),
    });
  }

  /** Merge a partial params edit for one output; empty-string/undefined fields clear. */
  setTextureParams(id: string, output: string, patch: Partial<TextureOutputGpuParams>): void {
    const tab = this.tabs.find(t => t.id === id);
    if (!tab) return;
    const merged: TextureOutputGpuParams = { ...tab.textureParams[output] };
    for (const [k, v] of Object.entries(patch) as [keyof TextureOutputGpuParams, string | undefined][]) {
      if (v) merged[k] = v;
      else delete merged[k];
    }
    const params = { ...tab.textureParams };
    if (Object.keys(merged).length) params[output] = merged;
    else delete params[output];
    this.patch(id, { textureParams: params });
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

    const tree = buildLegacyRootTree(STARTER_SOURCE[kind]);
    const treeState = new TreeState({ initial: tree });
    treeState.setSelected(tree.rootId);
    this.tabs = [
      ...this.tabs,
      {
        id,
        name: id,
        treeState,
        view: null,
        textureOutputs: [],
        textureParams: {},
        ...defaultTabMetadata(kind),
      },
    ];
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
    this.patch(id, { name: trimmed });
  }

  /**
   * Discard the entire local changeset and rebuild from `doc`. Rebuilds the tab *set*, not
   * just each tree's content — locally-created tabs disappear and locally-deleted ones come
   * back, which patching by id could never do. Undo stacks and per-tab metadata go with them,
   * which is correct for a revert.
   */
  resetFromDoc(doc: CompositionDoc, tabMeta: Record<string, TabMetadata>): void {
    this.tabs = buildTabs(doc, tabMeta, doc);
  }
}
