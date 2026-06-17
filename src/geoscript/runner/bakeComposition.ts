import * as THREE from 'three';

import { ROOT_NODE_NAME, type TreeDef } from '../geotoyAPIClient';
import { buildModuleNameToNodeId } from '../treeCodegen';
import { buildParentMap } from 'src/viz/scenes/geoscriptPlayground/treeOps';
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
  /** geotoyName was non-empty but matched neither a `materialMap` entry nor a same-named level material. */
  unmapped: boolean;
}

/**
 * Resolve a rendered mesh's geotoy material name to a level-def material id:
 * explicit `materialMap` → same-named level material → the referencing object's material →
 * `undefined` (caller falls back to the placeholder). `unmapped` lets the caller warn without
 * re-deriving the decision.
 */
export const resolveCompositionMaterial = (
  levelMaterialNames: ReadonlySet<string>,
  materialMap: Record<string, string> | undefined,
  objectMaterial: string | undefined,
  geotoyName: string
): CompositionMaterialResolution => {
  const mapped = geotoyName ? materialMap?.[geotoyName] : undefined;
  if (mapped && levelMaterialNames.has(mapped)) return { name: mapped, unmapped: false };
  if (geotoyName && levelMaterialNames.has(geotoyName)) return { name: geotoyName, unmapped: false };
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
export const bakeCompositionMeshes = (tree: TreeDef, objects: GeneratedObject[]): BakedCompositionMesh[] => {
  const worldMatrices = buildWorldMatrixCache(tree, buildParentMap(tree));
  const moduleToNode = buildModuleNameToNodeId(tree);
  const out: BakedCompositionMesh[] = [];

  for (const obj of objects) {
    if (obj.type !== 'mesh') continue;
    const namedModule = obj.sourceModule && obj.sourceModule !== ROOT_NODE_NAME;
    const nodeId = namedModule ? moduleToNode[obj.sourceModule] : undefined;
    if (namedModule && !nodeId) continue; // module no longer maps to a live node

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
