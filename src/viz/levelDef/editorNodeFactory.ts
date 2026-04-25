import * as THREE from 'three';

import type { Viz } from 'src/viz';
import type { ObjectDef, ObjectGroupDef } from './types';
import type { LevelObject, LevelGroup } from './levelSceneTypes';
import { isObjectGroup } from './levelDefTreeUtils';
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
}

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
  if (def.colliderShape !== undefined) {
    entity.isConvexHull = def.colliderShape === 'convexHull';
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

  const levelGroup: LevelGroup = {
    id: def.id,
    object: groupObj,
    def,
    children: [],
    generated: false,
  };

  for (const childDef of def.children) {
    if (isObjectGroup(childDef)) {
      const child = buildGroupSubtree(ctx, childDef);
      groupObj.add(child.object);
      levelGroup.children.push(child);
    } else {
      const leaf = buildLeafNode(ctx, childDef.asset, childDef);
      groupObj.add(leaf.object);
      levelGroup.children.push(leaf);
    }
  }

  return levelGroup;
}
