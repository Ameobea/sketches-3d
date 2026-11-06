import { runGeoscript } from 'src/geoscript/runner/geoscriptRunner';
import type { CsgAssetDef } from './types';
import { BAKED_RENDER_WRAPPER, type LevelObject } from './loadLevelDef';
import { LEVEL_PLACEHOLDER_MAT, instantiateLevelObject, meshesFromRunObjects } from './levelObjectUtils';
import { replaceLeafInstance } from './editorStructuralOps';
import { generateCsgCode } from './csgCodeGen';
import type { LevelEditor } from './LevelEditor.svelte';
import type { CsgResolveRuntime } from './csgResolveRuntime';

/**
 * Handles full CSG asset re-resolution: running geoscript on the complete CSG
 * tree, building a new prototype, and swapping all live level objects that use
 * the asset to the new geometry.
 *
 * Rapid requests for the same asset are coalesced — only its latest queued
 * request actually performs a full resolve.
 */
export class CsgAssetResolver {
  private requestId = 0;
  private latestRequestIdByAsset = new Map<string, number>();
  private queuedAssetIds = new Set<string>();
  private drainPromise: Promise<void> | null = null;

  /**
   * The level object currently being edited in CSG mode (may change between
   * queued calls; the drain loop always reads the latest value).
   */
  private editingLevelObj: LevelObject | null = null;
  /**
   * The selected node path at the time of the latest queued request (used to
   * decide whether to re-attach the transform gizmo after replacement).
   */
  private selectedNodePath: string | null = null;

  constructor(
    private readonly editor: LevelEditor,
    private readonly runtime: CsgResolveRuntime
  ) {}

  /**
   * Queue a full re-resolve of the given CSG asset.
   * Rapid calls for the same asset coalesce: only the most recent resolve actually runs.
   *
   * @param editingLevelObj  The level object currently open in CSG edit mode
   *                         (null when called outside active edit mode).
   * @param selectedNodePath The currently selected CSG node path (for gizmo
   *                         re-attachment after replacement).
   */
  async reResolveCsgAsset(
    assetId: string,
    editingLevelObj: LevelObject | null = null,
    selectedNodePath: string | null = null
  ): Promise<void> {
    this.queuedAssetIds.add(assetId);
    this.latestRequestIdByAsset.set(assetId, ++this.requestId);
    this.editingLevelObj = editingLevelObj;
    this.selectedNodePath = selectedNodePath;
    this.drainPromise ??= this.drainQueue();
    await this.drainPromise;
  }

  private async drainQueue(): Promise<void> {
    try {
      for (const assetId of this.queuedAssetIds) {
        this.queuedAssetIds.delete(assetId);
        await this.performResolve(assetId, this.latestRequestIdByAsset.get(assetId)!);
      }
    } finally {
      this.drainPromise = null;
    }
  }

  private async performResolve(assetId: string, requestId: number): Promise<void> {
    const csgDef = this.editor.levelDef.assets[assetId] as CsgAssetDef;
    const { modules: csgModules, code: csgCode } = generateCsgCode(csgDef, this.editor.levelDef.assets);
    const modules = { ...csgModules, code: csgCode };
    const renderWrapper = BAKED_RENDER_WRAPPER;

    let result;
    try {
      const { repl, ctxPtrPromise } = this.runtime.getAssetRuntime();
      const ctxPtr = await ctxPtrPromise;
      result = await runGeoscript({
        code: renderWrapper,
        ctxPtr,
        repl,
        preludeKind: undefined,
        modules,
      });
    } catch (error) {
      console.error(`[CsgAssetResolver] CSG re-resolve failed:`, error);
      this.runtime.terminateAssetWorker();
      return;
    }

    if (result.error) {
      console.error(`[CsgAssetResolver] CSG re-resolve failed:`, result.error);
      return;
    }

    const superseded = () => requestId !== this.latestRequestIdByAsset.get(assetId);
    if (superseded()) return;

    const meshes = meshesFromRunObjects(result.objects, LEVEL_PLACEHOLDER_MAT);
    if (meshes.length === 0) {
      console.warn(`[CsgAssetResolver] CSG asset "${assetId}" produced no meshes`);
      return;
    }

    if (meshes.length > 1) {
      throw new Error(
        `[CsgAssetResolver] CSG asset "${assetId}" produced ${meshes.length} meshes; leaf objects must resolve to a single mesh`
      );
    }
    const newPrototype = meshes[0];

    // Adopt the new prototype + (re)compute its collision hull if the asset's
    // `colliderShape` requires one.  Awaiting before triggering the physics rebuild
    // ensures syncPhysics uses the new hull rather than a stale one.
    if (this.editor.resolveAssetPrototype) {
      await this.editor.resolveAssetPrototype(assetId, newPrototype);
      if (superseded()) return;
    } else {
      this.editor.prototypes.set(assetId, newPrototype);
    }

    // Snapshot the CSG edit context at the time this resolve was latest.
    const editingLevelObj = this.editingLevelObj;
    const selectedNodePath = this.selectedNodePath;

    for (const levelObj of this.editor.allLevelObjects.values()) {
      if (levelObj.assetId !== assetId) continue;

      const clone = instantiateLevelObject(newPrototype, levelObj.def, {
        builtMaterials: this.editor.builtMaterials,
        fallbackMaterial: LEVEL_PLACEHOLDER_MAT,
      });

      if (levelObj === editingLevelObj) {
        // In CSG edit mode: preserve visibility state and skip mesh re-registration
        // because the preview scene owns raycast registration during active editing.
        const wasVisible = levelObj.object.visible;
        replaceLeafInstance(this.editor, levelObj, clone, {
          visible: wasVisible,
          skipMeshRegistration: true,
        });
        if (selectedNodePath === '') {
          this.editor.transformControls?.attach(levelObj.object);
        }
        continue;
      }

      replaceLeafInstance(this.editor, levelObj, clone);

      if (this.editor.selectedObject === levelObj) {
        this.editor.transformControls?.attach(levelObj.object);
      }
    }
  }
}
