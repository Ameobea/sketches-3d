import type { Viz } from 'src/viz';
import {
  withTree,
  type Composition,
  type CompositionDoc,
  type EnvironmentConfig,
  type TreeDef,
} from 'src/geoscript/geotoyAPIClient';
import type { MaterialDefinitions } from 'src/geoscript/materials';
import type { GeoscriptPlaygroundUserData } from 'src/viz/scenes/geoscriptPlayground/geoscriptPlayground.svelte';
import {
  clearSavedState,
  getServerState,
  getView,
  loadState,
  saveNewVersion,
  saveState,
  setLastRunWasSuccessful,
  type CompositionMeta,
  type PlaygroundState,
} from 'src/geotoy/modules/compositionStorage';

interface PersistenceOpts {
  viz: Viz;
  /** Read live — forking swaps the userData (and with it the draft-key suffix). */
  getUserData: () => GeoscriptPlaygroundUserData | undefined;
  /** Serializes the active tree's live editing state (tree content lives in TreeState). */
  serializeActiveTree: () => TreeDef;
  /** Reactive read of the active tree's dirty flag (latched + recomputed by TreeState). */
  isTreeDirty: () => boolean;
}

/** Sorted-key stringify so server-parsed vs live objects compare by content, not key order. */
const stableJson = (v: unknown): string =>
  JSON.stringify(v, (_k, val) =>
    val && typeof val === 'object' && !Array.isArray(val)
      ? Object.fromEntries(Object.entries(val).sort(([a], [b]) => (a < b ? -1 : 1)))
      : val
  );

/**
 * Owns the draftable composition state (container doc, materials, prelude flag,
 * environment) + the dirty flag, and is the single assembler of draft/version
 * snapshots.
 */
export class GeotoyPersistence {
  private readonly opts: PersistenceOpts;
  /** Draft-over-server merge captured once at construction. */
  readonly initial: PlaygroundState;

  doc = $state() as CompositionDoc;
  activeTreeId = $state('');
  materialDefinitions = $state() as MaterialDefinitions;
  preludeEjected = $state(false);
  environment: EnvironmentConfig | undefined = $state(undefined);

  /** Serialized server-version meta state; dirty = live content vs this. */
  private metaBaselines = $state.raw({ mats: '', env: '', prelude: false });
  /** Camera/view changes aren't content-baselined; explicit flag, cleared by markClean. */
  viewDirty = $state(false);
  private readonly metaDirty = $derived(
    stableJson($state.snapshot(this.materialDefinitions)) !== this.metaBaselines.mats ||
      stableJson(($state.snapshot(this.environment) as unknown) ?? null) !== this.metaBaselines.env ||
      this.preludeEjected !== this.metaBaselines.prelude
  );
  readonly isDirty = $derived.by(() => this.metaDirty || this.viewDirty || this.opts.isTreeDirty());

  constructor(opts: PersistenceOpts) {
    this.opts = opts;
    this.initial = loadState(opts.getUserData());
    this.doc = this.initial.doc;
    this.activeTreeId = this.initial.activeTreeId;
    this.materialDefinitions = this.initial.materials;
    this.preludeEjected = this.initial.preludeEjected;
    this.environment = this.initial.environment;
    const server = getServerState(opts.getUserData());
    this.metaBaselines = {
      mats: stableJson(server.materials),
      env: stableJson(server.environment ?? null),
      prelude: server.preludeEjected,
    };
  }

  /** Re-baseline meta dirt to the current live values (after save/fork/revert). */
  markClean = () => {
    this.metaBaselines = {
      mats: stableJson($state.snapshot(this.materialDefinitions)),
      env: stableJson(($state.snapshot(this.environment) as unknown) ?? null),
      prelude: this.preludeEjected,
    };
    this.viewDirty = false;
  };

  private get userData() {
    return this.opts.getUserData();
  }

  currentDoc = (): CompositionDoc => withTree(this.doc, this.activeTreeId, this.opts.serializeActiveTree());

  saveDraft = () =>
    saveState(
      {
        doc: this.currentDoc(),
        activeTreeId: this.activeTreeId,
        materials: this.materialDefinitions,
        view: getView(this.opts.viz),
        preludeEjected: this.preludeEjected,
        environment: this.environment,
      },
      this.userData
    );

  saveVersion = (comp: Composition, meta: CompositionMeta, userData = this.userData) =>
    saveNewVersion(
      comp,
      this.currentDoc(),
      this.activeTreeId,
      this.opts.viz,
      this.materialDefinitions,
      this.preludeEjected,
      this.environment,
      meta,
      userData
    );

  setLastRunWasSuccessful = (wasSuccessful: boolean) => setLastRunWasSuccessful(wasSuccessful, this.userData);

  /**
   * Drop drafts, reset owned state to the server version, and re-seed the draft from
   * it. The caller re-swaps tree content / textures / camera from the returned state.
   */
  revertToServer = (): PlaygroundState => {
    clearSavedState(this.userData);
    const server = getServerState(this.userData);
    this.doc = server.doc;
    this.activeTreeId = server.activeTreeId;
    this.materialDefinitions = server.materials;
    this.preludeEjected = server.preludeEjected;
    this.environment = server.environment;
    saveState(
      {
        doc: server.doc,
        activeTreeId: server.activeTreeId,
        materials: server.materials,
        view: server.view,
        preludeEjected: server.preludeEjected,
        environment: server.environment,
      },
      this.userData
    );
    this.markClean();
    return server;
  };
}
