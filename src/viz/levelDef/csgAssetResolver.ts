import * as THREE from 'three';

import { runGeoscript } from 'src/geoscript/runner/geoscriptRunner';
import type { CsgAssetDef } from './types';
import type { LevelObject } from './loadLevelDef';
import { LEVEL_PLACEHOLDER_MAT, instantiateLevelObject } from './levelObjectUtils';
import { replaceLeafInstance } from './editorStructuralOps';
import { generateCsgCode } from './csgCodeGen';
import type { LevelEditor } from './LevelEditor.svelte';
import type { CsgResolveRuntime } from './csgResolveRuntime';

/**
 * Handles full CSG asset re-resolution: running geoscript on the complete CSG
 * tree, building a new prototype, and swapping all live level objects that use
 * the asset to the new geometry.
 *
 * Multiple rapid requests are coalesced — only the latest queued request
 * actually performs a full resolve.
 */
export class CsgAssetResolver {
  private assetResolveRequestId = 0;
  private assetResolveLatestQueuedRequestId = 0;
  private assetResolveQueuedAssetId: string | null = null;
  private assetResolveDrainPromise: Promise<void> | null = null;

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
   * Multiple rapid calls coalesce: only the most recent resolve actually runs.
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
    this.assetResolveQueuedAssetId = assetId;
    this.editingLevelObj = editingLevelObj;
    this.selectedNodePath = selectedNodePath;

    const requestId = ++this.assetResolveRequestId;
    this.assetResolveLatestQueuedRequestId = requestId;

    if (!this.assetResolveDrainPromise) {
      this.assetResolveDrainPromise = this.drainQueue();
    }

    await this.assetResolveDrainPromise;
  }

  private async drainQueue(): Promise<void> {
    while (this.assetResolveQueuedAssetId) {
      const assetId = this.assetResolveQueuedAssetId;
      const requestId = this.assetResolveLatestQueuedRequestId;
      this.assetResolveQueuedAssetId = null;
      await this.performResolve(assetId, requestId);
    }
    this.assetResolveDrainPromise = null;
  }

  private async performResolve(assetId: string, requestId: number): Promise<void> {
    const csgDef = this.editor.levelDef.assets[assetId] as CsgAssetDef;
    const { modules: csgModules, code: csgCode } = generateCsgCode(csgDef, this.editor.levelDef.assets);
    const modules = { ...csgModules, code: csgCode };
    const renderWrapper = 'import { mesh } from "code"\nmesh | render';

    let result;
    try {
      const { repl, ctxPtrPromise } = this.runtime.getAssetRuntime();
      const ctxPtr = await ctxPtrPromise;
      result = await runGeoscript({
        code: renderWrapper,
        ctxPtr,
        repl,
        includePrelude: false,
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

    // Bail if a newer request has been queued since we started.
    if (requestId !== this.assetResolveLatestQueuedRequestId) return;

    const meshes: THREE.Mesh[] = [];
    for (const obj of result.objects) {
      if (obj.type !== 'mesh') continue;
      const mesh = new THREE.Mesh(obj.geometry, LEVEL_PLACEHOLDER_MAT);
      mesh.applyMatrix4(obj.transform);
      meshes.push(mesh);
    }
    if (meshes.length === 0) {
      console.warn(`[CsgAssetResolver] CSG asset "${assetId}" produced no meshes`);
      return;
    }

    if (meshes.length > 1) {
      throw new Error(
        `[CsgAssetResolver] CSG asset "${assetId}" produced ${meshes.length} meshes; leaf objects must resolve to a single mesh`
      );
    }
    const newPrototype: THREE.Mesh = meshes[0];

    // Adopt the new prototype + (re)compute its collision hull if the asset's
    // `colliderShape` requires one.  Awaiting before triggering the physics rebuild
    // ensures syncPhysics uses the new hull rather than a stale one.
    if (this.editor.resolveAssetPrototype) {
      await this.editor.resolveAssetPrototype(assetId, newPrototype);
      // Re-check supersession after the await — a newer resolve may have queued.
      if (requestId !== this.assetResolveLatestQueuedRequestId) return;
    } else {
      this.editor.prototypes.set(assetId, newPrototype);
    }

    // Snapshot the CSG edit context at the time this resolve was latest.
    const editingLevelObj = this.editingLevelObj;
    const selectedNodePath = this.selectedNodePath;

    for (const levelObj of this.editor.allLevelObjects) {
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
