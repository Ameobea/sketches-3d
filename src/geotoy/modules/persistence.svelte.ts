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
  getIsDirty,
  getServerState,
  getView,
  loadState,
  saveNewVersion,
  saveState,
  setLastRunWasSuccessful,
  type CompositionMeta,
  type PlaygroundState,
} from 'src/viz/scenes/geoscriptPlayground/persistence';

interface PersistenceOpts {
  viz: Viz;
  /** Read live — forking swaps the userData (and with it the draft-key suffix). */
  getUserData: () => GeoscriptPlaygroundUserData | undefined;
  /** Serializes the active tree's live editing state (tree content lives in TreeState). */
  serializeActiveTree: () => TreeDef;
}

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
  isDirty = $state(false);

  constructor(opts: PersistenceOpts) {
    this.opts = opts;
    this.initial = loadState(opts.getUserData());
    this.doc = this.initial.doc;
    this.activeTreeId = this.initial.activeTreeId;
    this.materialDefinitions = this.initial.materials;
    this.preludeEjected = this.initial.preludeEjected;
    this.environment = this.initial.environment;
    this.isDirty = getIsDirty(opts.getUserData());
  }

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
    this.isDirty = false;
    return server;
  };
}
