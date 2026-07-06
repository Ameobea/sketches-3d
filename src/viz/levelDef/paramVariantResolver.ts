import { runGeoscript } from 'src/geoscript/runner/geoscriptRunner';
import type { GeoscriptAsyncDeps } from 'src/geoscript/geoscriptWorker.worker';
import { compileTree, buildInjectedValues } from 'src/geoscript/treeCodegen';
import { bakeCompositionMeshes, type BakedCompositionMesh } from 'src/geoscript/runner/bakeComposition';
import type * as THREE from 'three';

import { BAKED_RENDER_WRAPPER } from './loadLevelDef';
import type { GeoscriptAssetDef, GeotoyCompositionAssetDef, ObjectDef } from './types';
import { injectInputs } from './inputInjection';
import type { InputsJson } from './paramVariants';
import { LEVEL_PLACEHOLDER_MAT, instantiateLevelObject, meshesFromRunObjects } from './levelObjectUtils';
import { replaceLeafInstance } from './editorStructuralOps';
import { buildCompositionChild } from './editorNodeFactory';
import { resolveCompositionMaterial } from 'src/geoscript/runner/bakeComposition';
import { isCompositionNode, type CompositionNode, type LevelObject } from './levelSceneTypes';
import type { LevelSceneNode } from './levelSceneTypes';
import { WorkerSlot } from './csgResolveRuntime';
import type { LevelEditor } from './LevelEditor.svelte';

const REBUILD_DEBOUNCE_MS = 150;

/**
 * Rebuilds a single placement after its per-object `inputs` change: resolves (or reuses) the
 * new param-variant's prototype/baked meshes on a lazily-booted editor worker, then swaps the
 * live instance. Rapid edits per node are debounced + superseded (latest wins).
 */
export class ParamVariantResolver {
  private slot = new WorkerSlot();
  private debounceTimers = new Map<string, number>();
  private seqByNode = new Map<string, number>();

  constructor(private readonly editor: LevelEditor) {}

  destroy() {
    for (const t of this.debounceTimers.values()) clearTimeout(t);
    this.debounceTimers.clear();
    this.slot.terminate();
  }

  queueRebuild(node: LevelSceneNode) {
    clearTimeout(this.debounceTimers.get(node.id));
    this.debounceTimers.set(
      node.id,
      window.setTimeout(() => {
        this.debounceTimers.delete(node.id);
        void this.rebuild(node);
      }, REBUILD_DEBOUNCE_MS)
    );
  }

  private nextSeq(nodeId: string): number {
    const seq = (this.seqByNode.get(nodeId) ?? 0) + 1;
    this.seqByNode.set(nodeId, seq);
    return seq;
  }

  private async rebuild(node: LevelSceneNode) {
    const editor = this.editor;
    const isComp = isCompositionNode(node);
    const def = isComp ? node.compositionDef : (node as LevelObject).def;
    if (!def.asset) return;
    const assetDef = editor.levelDef.assets[def.asset];
    if (!assetDef || (assetDef.type !== 'geoscript' && assetDef.type !== 'geotoyComposition')) return;

    const eid = editor.effectiveAssetId(def);
    const merged: InputsJson = { ...(assetDef.inputs ?? {}), ...(def.inputs ?? {}) };
    const seq = this.nextSeq(node.id);
    const superseded = () => this.seqByNode.get(node.id) !== seq;

    try {
      if (isComp) {
        let baked = editor.compositionBaked.get(eid);
        if (!baked) {
          baked =
            (await this.bakeCompositionVariant(eid, assetDef as GeotoyCompositionAssetDef, merged)) ??
            undefined;
          if (!baked || superseded()) return;
          editor.compositionBaked.set(eid, baked);
        }
        this.swapCompositionParts(node, baked, assetDef as GeotoyCompositionAssetDef);
      } else {
        let proto = editor.prototypes.get(eid);
        if (!proto) {
          proto =
            (await this.resolveGeoscriptVariant(eid, assetDef as GeoscriptAssetDef, merged)) ?? undefined;
          if (!proto || superseded()) return;
          proto.name = eid;
          if (editor.resolveAssetPrototype) {
            await editor.resolveAssetPrototype(eid, proto);
            if (superseded()) return;
          } else {
            editor.prototypes.set(eid, proto);
          }
        }
        const levelObj = node as LevelObject;
        levelObj.assetId = eid;
        const clone = instantiateLevelObject(proto, levelObj.def, {
          builtMaterials: editor.builtMaterials,
          fallbackMaterial: LEVEL_PLACEHOLDER_MAT,
        });
        replaceLeafInstance(editor, levelObj, clone);
        if (editor.selectedObject === levelObj) {
          editor.transformControls?.attach(levelObj.object);
        }
      }
    } catch (err) {
      console.error(`[ParamVariantResolver] rebuild of "${node.id}" failed:`, err);
      this.slot.terminate();
    }
  }

  private async resolveGeoscriptVariant(
    eid: string,
    assetDef: GeoscriptAssetDef,
    merged: InputsJson
  ): Promise<THREE.Mesh | null> {
    const { repl, ctxPtrPromise } = this.slot.get();
    const ctxPtr = await ctxPtrPromise;
    await this.initAsyncDeps(assetDef._meta?.asyncDeps);

    const result = await runGeoscript({
      code: BAKED_RENDER_WRAPPER,
      ctxPtr,
      repl,
      includePrelude: assetDef.includePrelude ?? true,
      modules: { code: assetDef.code },
      gizmoValues: injectInputs({}, merged, ['code']),
    });
    if (result.error) {
      console.error(`[ParamVariantResolver] variant "${eid}" failed:`, result.error);
      return null;
    }
    if (result.controls.length > 0) this.editor.assetControls.set(eid, result.controls);

    const meshes = meshesFromRunObjects(result.objects, LEVEL_PLACEHOLDER_MAT);
    if (meshes.length !== 1) {
      console.warn(`[ParamVariantResolver] variant "${eid}" produced ${meshes.length} meshes`);
      return meshes[0] ?? null;
    }
    return meshes[0];
  }

  private async bakeCompositionVariant(
    eid: string,
    def: GeotoyCompositionAssetDef,
    merged: InputsJson
  ): Promise<BakedCompositionMesh[] | null> {
    const { repl, ctxPtrPromise } = this.slot.get();
    const ctxPtr = await ctxPtrPromise;
    await this.initAsyncDeps(def._meta?.asyncDeps);

    const compiled = compileTree(def.tree);
    const preludeEjected = def.preludeEjected ?? false;
    const ambientSources: string[] = [];
    if (!preludeEjected) ambientSources.push(await repl.getPrelude());
    if (def.tree.globalsSource.trim().length > 0) ambientSources.push(def.tree.globalsSource);
    await repl.setMaterials(ctxPtr, def.defaultMaterialName ?? null, def.materialNames ?? []);

    const result = await runGeoscript({
      code: compiled.rootSource,
      ctxPtr,
      repl,
      includePrelude: !preludeEjected,
      modules: compiled.modules,
      ambientSources,
      gizmoValues: injectInputs(buildInjectedValues(def.tree), merged, [
        ...Object.keys(compiled.modules),
        '_root',
      ]),
    });
    if (result.error) {
      console.error(`[ParamVariantResolver] composition variant "${eid}" failed:`, result.error);
      return null;
    }
    if (result.controls.length > 0) this.editor.assetControls.set(eid, result.controls);
    return bakeCompositionMeshes(def.tree, result.objects);
  }

  private async initAsyncDeps(deps: string[] | undefined) {
    const { repl } = this.slot.get();
    for (const dep of deps ?? []) {
      if (dep === 'text_to_path') continue;
      await repl.initAsyncDep(dep as keyof GeoscriptAsyncDeps);
    }
  }

  /** Tear down + rebuild a composition group's opaque parts from newly-baked meshes. */
  private swapCompositionParts(
    group: CompositionNode,
    baked: BakedCompositionMesh[],
    assetDef: GeotoyCompositionAssetDef
  ) {
    const editor = this.editor;
    const objDef: ObjectDef = group.compositionDef;
    for (const part of group.opaqueParts ?? []) {
      editor.unregisterMeshes(part);
      editor.removePhysics(part);
      group.object.remove(part.object);
      editor.allLevelObjects.delete(part.id);
    }

    const matNames = new Set(Object.keys(editor.levelDef.materials ?? {}));
    const resolveMatName = (geotoyName: string) =>
      resolveCompositionMaterial(matNames, assetDef.materialMap, objDef.asset!, objDef.material, geotoyName)
        .name;

    group.opaqueParts = baked.map((bm, i) => {
      const part = buildCompositionChild(
        { viz: editor.viz, builtMaterials: editor.builtMaterials },
        objDef,
        bm,
        i,
        resolveMatName
      );
      part.owner = group;
      group.object.add(part.object);
      editor.allLevelObjects.set(part.id, part);
      editor.registerMeshes(part);
      editor.syncPhysics(part);
      return part;
    });
  }
}
