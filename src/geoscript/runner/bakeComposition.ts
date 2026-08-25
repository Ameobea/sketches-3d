import * as THREE from 'three';

import { ROOT_NODE_NAME, type TreeDef } from '../geotoyAPIClient';
import { buildModuleNameToNodeId, qualifyModuleName } from '../treeCodegen';
import { buildParentMap } from 'src/geotoy/modules/treeOps';
import type { GeneratedObject } from './types';
import { buildWorldMatrixCache, type NodeWorldInstance } from './worldMatrixCache';

export interface BakedCompositionMesh {
  geometry: THREE.BufferGeometry;
  /** Composition-space transform: nodeWorld(instance copy) × in-script mesh transform. */
  matrix: THREE.Matrix4;
  /** geotoy material name; the palette default name for meshes that didn't call `set_material` (`''` if the runtime had no default). */
  materialName: string;
}

export interface CompositionMaterialResolution {
  name: string | undefined;
  /** geotoyName was non-empty but resolved to neither `materialMap` nor an auto-imported material. */
  unmapped: boolean;
}

/** Prefix + key for a composition's own palette material, auto-imported as an anonymous level material. */
export const COMP_MATERIAL_PREFIX = '__comp:';
export const compMaterialKey = (assetId: string, geotoyName: string): string =>
  `${COMP_MATERIAL_PREFIX}${assetId}:${geotoyName}`;

/**
 * Resolve a rendered mesh's geotoy material name to a level-def material id:
 * explicit `materialMap` override → the auto-imported composition material (`compMaterialKey`) →
 * the referencing object's material → `undefined` (caller falls back to the placeholder).
 * `unmapped` lets the caller warn without re-deriving the decision.
 */
export const resolveCompositionMaterial = (
  levelMaterialNames: ReadonlySet<string>,
  materialMap: Record<string, string> | undefined,
  assetId: string,
  objectMaterial: string | undefined,
  geotoyName: string
): CompositionMaterialResolution => {
  const mapped = geotoyName ? materialMap?.[geotoyName] : undefined;
  if (mapped && levelMaterialNames.has(mapped)) return { name: mapped, unmapped: false };
  const auto = geotoyName ? compMaterialKey(assetId, geotoyName) : undefined;
  if (auto && levelMaterialNames.has(auto)) return { name: auto, unmapped: false };
  const name = objectMaterial && levelMaterialNames.has(objectMaterial) ? objectMaterial : undefined;
  return { name, unmapped: geotoyName.length > 0 };
};

const IDENTITY_INSTANCE: NodeWorldInstance[] = [{ world: new THREE.Matrix4(), path: [] }];

/**
 * Headless analogue of `populateScene`'s mesh loop: turn a composition tree run into a flat
 * list of baked mesh prototypes (one per rendered mesh × ancestor instance copy), dropping
 * rendered lights/paths. Geometry is shared across instance copies — consumers set per-copy
 * `Object3D` matrices rather than mutating verts.
 */
export const bakeCompositionMeshes = (
  tree: TreeDef,
  objects: GeneratedObject[],
  tabId?: string
): BakedCompositionMesh[] => {
  const worldMatrices = buildWorldMatrixCache(tree, buildParentMap(tree));
  const moduleToNode = buildModuleNameToNodeId(tree, tabId);
  const rootModule = qualifyModuleName(ROOT_NODE_NAME, tabId);
  const out: BakedCompositionMesh[] = [];
  const droppedModules = new Set<string>();

  for (const obj of objects) {
    if (obj.type !== 'mesh') continue;
    const namedModule = obj.sourceModule && obj.sourceModule !== rootModule;
    const nodeId = namedModule ? moduleToNode[obj.sourceModule] : undefined;
    // Skip modules that don't map to a live node in the imported tree — dep-tab renders
    // (no transform context here) and stale module names. Geotoy would show dep-tab
    // renders, so make the divergence loud.
    if (namedModule && !nodeId) {
      if (!droppedModules.has(obj.sourceModule)) {
        droppedModules.add(obj.sourceModule);
        console.warn(
          `[bakeComposition] dropping mesh rendered by module "${obj.sourceModule}" — not part of the imported tab's tree`
        );
      }
      continue;
    }

    const insts = (nodeId ? worldMatrices.get(nodeId) : null) ?? IDENTITY_INSTANCE;
    for (const inst of insts) {
      out.push({
        geometry: obj.geometry,
        matrix: inst.world.clone().multiply(obj.transform),
        materialName: obj.materialName,
      });
    }
  }

  return out;
};
