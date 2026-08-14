import {
  type Composition,
  type CompositionDoc,
  type TabMetadata,
  type TreeEntry,
} from 'src/geoscript/geotoyAPIClient';
import type { MaterialDefinitions } from 'src/geoscript/materials';
import type { GeoscriptPlaygroundUserData } from 'src/viz/scenes/geoscriptPlayground/geoscriptPlayground.svelte';
import {
  clearSavedState,
  getServerState,
  loadState,
  saveNewVersion,
  saveState,
  setLastRunWasSuccessful,
  type CompositionMeta,
  type PlaygroundState,
} from 'src/geotoy/modules/compositionStorage';
import { tabShapeKey } from 'src/geotoy/modules/tabs.svelte';

interface PersistenceOpts {
  /** Read live — forking swaps the userData (and with it the draft-key suffix). */
  getUserData: () => GeoscriptPlaygroundUserData | undefined;
  /** Serializes *every* live tab. A non-active tab's edits exist only in its `TreeState`,
   *  so folding just the active tree would silently drop them on save. */
  serializeTabs: () => TreeEntry[];
  /** Reactive read of "any tab dirty" (each latched + recomputed by its TreeState). */
  isTreeDirty: () => boolean;
  /** Cheap key of the tab set's shape; content-free so it can be read on every dirty check. */
  tabShapeKey: () => string;
  /**
   * Per-tab metadata for the live tab set, with the active tab's view refreshed from its mode
   * first. The single capture point: both draft and version snapshots go through it, so a
   * saved version can't miss poses collected during the session.
   */
  collectTabMeta: () => Record<string, TabMetadata>;
  /** Same record without the live-capture side effect; safe to read from a derived. */
  peekTabMeta: () => Record<string, TabMetadata>;
}

/** Sorted-key stringify so server-parsed vs live objects compare by content, not key order. */
const stableJson = (v: unknown): string =>
  JSON.stringify(v, (_k, val) =>
    val && typeof val === 'object' && !Array.isArray(val)
      ? Object.fromEntries(Object.entries(val).sort(([a], [b]) => (a < b ? -1 : 1)))
      : val
  );

/** Sorted-key stringify of the per-tab record minus camera poses: orbiting must not dirty
 *  the composition, but ejecting the prelude or editing the environment must. */
const tabMetaKey = (meta: Record<string, TabMetadata>): string =>
  stableJson(Object.fromEntries(Object.entries(meta).map(([id, m]) => [id, { ...m, view: undefined }])));

/**
 * Owns the draftable composition state (container doc + materials) and the dirty flag, and is
 * the single assembler of draft/version snapshots. Per-tab state lives on `GeotoyTabs`.
 */
export class GeotoyPersistence {
  private readonly opts: PersistenceOpts;
  /** Draft-over-server merge captured once at construction. */
  readonly initial: PlaygroundState;

  doc = $state() as CompositionDoc;
  activeTreeId = $state('');
  materialDefinitions = $state() as MaterialDefinitions;

  /** Serialized server-version meta state; dirty = live content vs this. */
  private metaBaselines = $state.raw({ mats: '', tabMeta: '', tabShape: '' });
  /** Camera/view changes aren't content-baselined; explicit flag, cleared by markClean. */
  viewDirty = $state(false);
  // `.by` (not bare `$derived`) so the `this.opts` read stays deferred past field init.
  private readonly metaDirty = $derived.by(
    () =>
      stableJson($state.snapshot(this.materialDefinitions)) !== this.metaBaselines.mats ||
      tabMetaKey(this.opts.peekTabMeta()) !== this.metaBaselines.tabMeta ||
      this.opts.tabShapeKey() !== this.metaBaselines.tabShape
  );
  readonly isDirty = $derived.by(() => this.metaDirty || this.viewDirty || this.opts.isTreeDirty());

  constructor(opts: PersistenceOpts) {
    this.opts = opts;
    this.initial = loadState(opts.getUserData());
    this.doc = this.initial.doc;
    this.activeTreeId = this.initial.activeTreeId;
    this.materialDefinitions = this.initial.materials;
    const server = getServerState(opts.getUserData());
    this.metaBaselines = {
      mats: stableJson(server.materials),
      tabMeta: tabMetaKey(server.tabMeta),
      tabShape: tabShapeKey(server.doc.trees),
    };
  }

  /** Re-baseline meta dirt to the current live values (after save/fork/revert). */
  markClean = () => {
    this.metaBaselines = {
      mats: stableJson($state.snapshot(this.materialDefinitions)),
      tabMeta: tabMetaKey(this.opts.peekTabMeta()),
      tabShape: this.opts.tabShapeKey(),
    };
    this.viewDirty = false;
  };

  private get userData() {
    return this.opts.getUserData();
  }

  currentDoc = (): CompositionDoc => ({ ...this.doc, trees: this.opts.serializeTabs() });

  saveDraft = () =>
    saveState(
      {
        doc: this.currentDoc(),
        activeTreeId: this.activeTreeId,
        materials: this.materialDefinitions,
        tabMeta: this.opts.collectTabMeta(),
      },
      this.userData
    );

  saveVersion = (comp: Composition, meta: CompositionMeta, userData = this.userData) =>
    saveNewVersion(
      comp,
      this.currentDoc(),
      this.activeTreeId,
      this.materialDefinitions,
      this.opts.collectTabMeta(),
      meta,
      userData
    );

  setLastRunWasSuccessful = (wasSuccessful: boolean) => setLastRunWasSuccessful(wasSuccessful, this.userData);

  /**
   * Drop drafts, reset owned state to the server version, and re-seed the draft from it.
   * The caller rebuilds the tab set (and with it every tab's metadata) from the return value.
   */
  revertToServer = (): PlaygroundState => {
    clearSavedState(this.userData);
    const server = getServerState(this.userData);
    this.doc = server.doc;
    this.activeTreeId = server.activeTreeId;
    this.materialDefinitions = server.materials;
    saveState(
      {
        doc: server.doc,
        activeTreeId: server.activeTreeId,
        materials: server.materials,
        tabMeta: server.tabMeta,
      },
      this.userData
    );
    this.markClean();
    return server;
  };
}
