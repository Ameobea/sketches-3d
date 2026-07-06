import * as THREE from 'three';

import type { Viz } from 'src/viz';
import type { BakedCompositionMesh } from 'src/geoscript/runner/bakeComposition';
import { resolveCompositionMaterial } from 'src/geoscript/runner/bakeComposition';
import type { LevelDef, ObjectDef, ObjectGroupDef } from './types';
import type { LevelObject, LevelGroup, LevelSceneNode, CompositionNode } from './levelSceneTypes';
import { isLevelGroup, isCompositionNode } from './levelSceneTypes';
import { isObjectGroup, isGeneratedDef, hasAsset } from './levelDefTreeUtils';
import { LEVEL_PLACEHOLDER_MAT, applyTransform, instantiateLevelObject } from './levelObjectUtils';
import { Entity } from '../sceneRuntime/Entity';

/**
 * Minimal context required to build runtime nodes from defs.
 * LevelEditor satisfies this interface structurally.
 */
export interface BuildCtx {
  viz: Viz;
  prototypes: Map<string, THREE.Mesh>;
  builtMaterials: Map<string, THREE.Material>;
  /** Baked meshes per `geotoyComposition` asset (present in the editor; absent in headless callers). */
  compositionBaked?: Map<string, BakedCompositionMesh[]>;
  /** Needed alongside `compositionBaked` to resolve composition material-name mappings. */
  levelDef?: LevelDef;
  /**
   * Resolves a def to the asset id its prototype/baked meshes live under (a param-variant id when
   * the def carries `inputs`, else the authored id). Absent = authored id.
   */
  effectiveAssetId?: (def: ObjectDef) => string;
}

const effectiveAssetIdOf = (ctx: BuildCtx, def: ObjectDef): string =>
  ctx.effectiveAssetId?.(def) ?? def.asset ?? '';

/**
 * Build a LevelObject from a def and its asset prototype.
 * Does NOT add to the scene, allLevelObjects, nodeById, or register meshes/physics.
 * Call attachSubtree to register the result with the editor.
 */
export function buildLeafNode(ctx: BuildCtx, assetId: string, def: ObjectDef): LevelObject {
  const clone = instantiateLevelObject(ctx.prototypes.get(assetId)!, def, {
    builtMaterials: ctx.builtMaterials,
    fallbackMaterial: LEVEL_PLACEHOLDER_MAT,
  });
  const entity = new Entity(ctx.viz, def.id, clone);
  if (def.nonPermeable !== undefined) {
    entity.nonPermeable = def.nonPermeable;
  }
  return { id: def.id, assetId, object: clone, def, generated: false, entity };
}

/**
 * Recursively build a LevelGroup subtree from a def.
 * Sets up the Three.js parent–child hierarchy within the subtree but does NOT
 * add the root to the scene or register anything with the editor.
 * Call attachSubtree to register the result with the editor.
 */
export function buildGroupSubtree(ctx: BuildCtx, def: ObjectGroupDef): LevelGroup {
  const groupObj = new THREE.Group();
  applyTransform(groupObj, def);

  // Strip `children` from the runtime def: hierarchy lives only in `levelGroup.children`.
  const { children: childDefs, ...body } = def;
  const levelGroup: LevelGroup = {
    id: def.id,
    object: groupObj,
    def: body,
    children: [],
    generated: false,
  };

  for (const childDef of childDefs) {
    if (isObjectGroup(childDef)) {
      const child = buildGroupSubtree(ctx, childDef);
      groupObj.add(child.object);
      levelGroup.children.push(child);
    } else if (!hasAsset(childDef)) {
      // dash-token marker: the clone serializes its def + reload re-materializes it; only live
      // in-editor spawn (token visual + physics ghost, from the parkour subsystem) is skipped.
      continue;
    } else if (ctx.compositionBaked?.has(effectiveAssetIdOf(ctx, childDef))) {
      const child = buildCompositionGroupFromCtx(ctx, childDef);
      if (child) {
        groupObj.add(child.object);
        levelGroup.children.push(child);
      }
    } else {
      const leaf = buildLeafNode(ctx, effectiveAssetIdOf(ctx, childDef), childDef);
      groupObj.add(leaf.object);
      levelGroup.children.push(leaf);
    }
  }

  return levelGroup;
}

/** Just enough context to build composition children — viz (for entities) + built materials. */
export interface CompositionCtx {
  viz: Viz;
  builtMaterials: Map<string, THREE.Material>;
}

/**
 * Build one opaque part (a baked composition mesh) as a `LevelObject`. It is NOT a node — the
 * caller parks it on `owner.opaqueParts` and sets `.owner`; it reuses leaf plumbing for
 * physics/materials. Material is assigned from `builtMaterials` (placeholder until built); callers
 * layer on any post-build material wiring. Does NOT parent the mesh or register it.
 */
export function buildCompositionChild(
  ctx: CompositionCtx,
  objDef: ObjectDef,
  baked: BakedCompositionMesh,
  childIndex: number,
  resolveMaterialName: (geotoyName: string) => string | undefined
): LevelObject {
  const matName = resolveMaterialName(baked.materialName);
  const builtMat = matName ? ctx.builtMaterials.get(matName) : undefined;
  const mesh = new THREE.Mesh(baked.geometry, builtMat ?? LEVEL_PLACEHOLDER_MAT);
  baked.matrix.decompose(mesh.position, mesh.quaternion, mesh.scale);
  mesh.castShadow = objDef.castShadow ?? true;
  mesh.receiveShadow = objDef.receiveShadow ?? true;
  const childId = `${objDef.id}::${childIndex}`;
  mesh.name = childId;

  const childDef: ObjectDef = {
    id: childId,
    asset: objDef.asset,
    material: matName,
    nocollide: objDef.nocollide,
    userData: objDef.userData,
  };
  const entity = new Entity(ctx.viz, childId, mesh);
  if (objDef.nonPermeable !== undefined) entity.nonPermeable = objDef.nonPermeable;
  return { id: childId, assetId: objDef.asset ?? '', object: mesh, def: childDef, generated: false, entity };
}

/**
 * Build a composition placement as a `LevelGroup` (transform container) whose baked meshes are
 * opaque parts (`opaqueParts`), not editable child nodes. Does NOT add to the scene/editor tracking
 * — call `attachSubtree`. The `compositionDef` marker keeps the placement pointer recoverable.
 */
export function buildCompositionGroup(
  ctx: CompositionCtx,
  objDef: ObjectDef,
  baked: BakedCompositionMesh[],
  resolveMaterialName: (geotoyName: string) => string | undefined
): LevelGroup {
  const groupObj = new THREE.Group();
  applyTransform(groupObj, objDef);
  const levelGroup: LevelGroup = {
    id: objDef.id,
    object: groupObj,
    def: {
      id: objDef.id,
      position: objDef.position,
      rotation: objDef.rotation,
      scale: objDef.scale,
      userData: objDef.userData,
    },
    children: [],
    generated: isGeneratedDef(objDef),
    compositionDef: objDef,
  };
  levelGroup.opaqueParts = baked.map((bm, i) => {
    const part = buildCompositionChild(ctx, objDef, bm, i, resolveMaterialName);
    part.owner = levelGroup;
    groupObj.add(part.object);
    return part;
  });
  return levelGroup;
}

/**
 * Build a composition placement group from the editor's `BuildCtx` — resolves the cached baked
 * meshes and the asset's material-name map. Returns null if the asset has no baked meshes.
 */
export function buildCompositionGroupFromCtx(ctx: BuildCtx, objDef: ObjectDef): LevelGroup | null {
  const assetId = objDef.asset;
  if (assetId === undefined) return null;
  // Baked meshes live under the effective (variant) id; the materialMap under the authored id.
  const baked = ctx.compositionBaked?.get(effectiveAssetIdOf(ctx, objDef));
  if (!baked) {
    console.warn(`[editorNodeFactory] No baked meshes for composition asset "${assetId}"`);
    return null;
  }
  const asset = ctx.levelDef?.assets[assetId];
  const materialMap = asset?.type === 'geotoyComposition' ? asset.materialMap : undefined;
  const names = new Set(Object.keys(ctx.levelDef?.materials ?? {}));
  return buildCompositionGroup(
    ctx,
    objDef,
    baked,
    g => resolveCompositionMaterial(names, materialMap, assetId, objDef.material, g).name
  );
}

/**
 * The persisted leaf-pointer `ObjectDef` for a composition node (asset/material/etc.), with the live
 * group transform. The def must only ever round-trip this pointer — never the expanded opaque parts
 * (which would re-expand recursively on reload).
 */
export function compositionPointerDef(group: CompositionNode): ObjectDef {
  const src = group.compositionDef;
  return {
    ...src,
    position: group.def.position ?? src.position,
    rotation: group.def.rotation ?? src.rotation,
    scale: group.def.scale ?? src.scale,
  };
}

/**
 * The persisted def for a node — the only projection ever serialized / cloned / restored:
 * composition → its leaf pointer (expanded content dropped), group → a recursive `ObjectGroupDef`,
 * leaf → a shallow copy of its def. Descendant defs are shallow-cloned (callers needing a fully
 * detached snapshot should JSON-clone the result).
 */
export function nodeToDef(node: LevelSceneNode): ObjectDef | ObjectGroupDef {
  if (isCompositionNode(node)) return compositionPointerDef(node);
  if (isLevelGroup(node)) return serializeGroup(node);
  return { ...node.def };
}

/** A runtime group subtree as a fresh `ObjectGroupDef` (children projected via `nodeToDef`). */
export function serializeGroup(group: LevelGroup): ObjectGroupDef {
  return { ...group.def, children: group.children.map(nodeToDef) };
}
