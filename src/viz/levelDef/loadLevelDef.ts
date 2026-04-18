import * as THREE from 'three';

import { dev } from '$app/environment';
import { GeoscriptExecutor } from 'src/geoscript/geoscriptExecutor';
import type { GeoscriptJob } from 'src/geoscript/geoscriptExecutor';
import type { Viz } from 'src/viz';
import { withWorldSpaceTransform } from 'src/viz/util/three';
import {
  LEVEL_PLACEHOLDER_MAT,
  applyTransform,
  assignMaterial,
  forEachMesh,
  instantiateLevelObject,
} from './levelObjectUtils';
import type {
  AssetDef,
  BehaviorSpec,
  CsgAssetDef,
  CsgTreeNode,
  GeoscriptAssetMeta,
  LevelDef,
  ObjectDef,
  ObjectGroupDef,
} from './types';
import type { GraphicsQuality } from 'src/viz/conf';
import type { SceneRuntime } from '../sceneRuntime';
import type { BehaviorFn } from '../sceneRuntime/types';
import { isObjectGroup, flattenLeaves, isGeneratedDef } from './levelDefTreeUtils';
import { type LevelObject, type LevelGroup, type LevelSceneNode, type LevelLight } from './levelSceneTypes';
import { replaceLeafInstance } from './editorStructuralOps';
import { addLevelLightToScene, createLevelLight } from './levelLightUtils';
export type { LevelObject, LevelGroup, LevelSceneNode, LevelLight } from './levelSceneTypes';
export { isLevelGroup } from './levelSceneTypes';
import { buildMaterial } from './buildMaterial';
import { TextureFetchPool } from './texturePool';
import { generateCsgCode } from './csgCodeGen';
import type { BtRigidBody } from 'src/ammojs/ammoTypes';

type PhysicsContext = NonNullable<Viz['fpCtx']>;

/** @deprecated Use LevelLoadHandle instead */
export interface LoadedLevel {
  objects: LevelObject[];
}

export interface LevelLoadHandle {
  /**
   * Resolves with all placed LevelObjects once every asset (gltf + geoscript) has been resolved
   * and every object is in the scene. Materials may still be streaming in at this point.
   */
  objects: Promise<LevelObject[]>;
  /**
   * Resolves when all texture fetches are done and all materials have been assigned.
   * Gate player input on this to avoid unplayable low FPS during texture uploads.
   */
  complete: Promise<void>;
  /**
   * Map from assetId → uncloned prototype Object3D. Populated incrementally as assets resolve.
   * Safe to read (with full coverage) once `objects` has resolved.
   * Intended for the level editor's "add object" flow.
   */
  prototypes: Map<string, THREE.Object3D>;
  /**
   * Map from material name → built THREE.Material. Populated incrementally as textures load.
   * Safe to read (with full coverage) once `complete` has resolved.
   * Intended for the level editor's "add object" flow.
   */
  builtMaterials: Map<string, THREE.Material>;
  /**
   * Flat map of texKey → THREE.Texture for all textures that have successfully loaded.
   * Populated incrementally; complete once `complete` resolves.
   * Intended for the level editor's live material rebuild path.
   */
  loadedTextures: Map<string, THREE.Texture>;
  /**
   * The top-level scene nodes (LevelObjects and LevelGroups) in the order they appear
   * in the level def. Available once `objects` has resolved.
   */
  rootNodes: LevelSceneNode[];
  /**
   * Fast lookup from any node id (group or object) to its LevelSceneNode.
   * Populated incrementally — safe to read once `objects` has resolved.
   */
  nodeById: Map<string, LevelSceneNode>;
  /**
   * Resolves with all LevelObjects that carry `parkour` metadata in their def.
   * Available once `objects` has resolved.
   */
  parkourObjects: Promise<LevelObject[]>;
  /**
   * Resolves (after `complete`) with every THREE.Mesh whose assigned material def
   * has `emissiveBypass: true`. These are automatically added to
   * `viz.postprocessingController.emissiveBypassPass` if one is present.
   */
  emissiveBypassMeshes: Promise<THREE.Mesh[]>;
  /**
   * Register factory functions for `type: "generated"` materials.  Call this synchronously
   * inside `processLoadedScene` so factories are available before async asset resolution
   * finishes and objects start being placed.
   *
   * Each key must match a material name in the level def whose `type` is `"generated"`.
   * The factory receives `viz` and returns a `MaterialFactoryResult` — either a plain
   * `THREE.Material`, or `{ material, onAssigned }` where `onAssigned` is called for every
   * mesh the material is applied to, after the mesh is placed with its final transform.
   */
  setMaterialFactories(factories: Record<string, (viz: Viz) => MaterialFactoryResult>): void;
  /**
   * All lights instantiated from the level def. Available immediately (synchronous).
   */
  lights: LevelLight[];
  /**
   * Connect a SceneRuntime to the level def system.  Objects with `behaviors` or `spawner`
   * fields in the level def will have entities created and behaviors attached automatically
   * once objects are placed and physics is ready.
   *
   * @param sceneName — used to resolve level-local behaviors (e.g. `holes__myBehavior`)
   */
  setSceneRuntime(runtime: SceneRuntime, sceneName: string): void;
}

/**
 * Return type for generated material factories.  Return a plain `THREE.Material` when no
 * post-assignment setup is needed, or `{ material, onAssigned }` to receive a callback for
 * each mesh the material is applied to (useful e.g. for bbox-dependent uniform initialization).
 */
export type MaterialFactoryResult =
  | THREE.Material
  | { material: THREE.Material; onAssigned: (mesh: THREE.Mesh) => void };

/**
 * Builds a reverse-dependency map: for each asset id, the set of CSG asset ids
 * that directly or transitively depend on it.  Used for hot-reload to find all
 * assets that need re-running when a geo file changes.
 */
const buildReverseDeps = (assets: Record<string, AssetDef>): Map<string, Set<string>> => {
  const rev = new Map<string, Set<string>>();
  for (const id of Object.keys(assets)) rev.set(id, new Set());

  for (const [id, asset] of Object.entries(assets)) {
    if (asset.type !== 'csg') continue;
    const visitNode = (node: CsgTreeNode) => {
      if ('asset' in node) {
        rev.get(node.asset)?.add(id);
      } else {
        for (const child of node.children) visitNode(child);
      }
    };
    visitNode(asset.tree);
  }
  return rev;
};

/** Returns the set of asset ids that transitively depend on `changedId` (inclusive). */
const getDownstreamAssets = (changedId: string, reverseDeps: Map<string, Set<string>>): Set<string> => {
  const affected = new Set<string>();
  const visit = (id: string) => {
    if (affected.has(id)) return;
    affected.add(id);
    for (const dep of reverseDeps.get(id) ?? []) visit(dep);
  };
  visit(changedId);
  return affected;
};

/**
 * Collects the set of assets transitively reachable from a seed set of directly-referenced
 * asset IDs (i.e. the assets used by level objects). CSG sub-assets are followed recursively.
 * Any asset not in the returned set is orphaned and can be skipped.
 */
const computeReachableAssets = (
  assets: Record<string, AssetDef>,
  directRefs: Iterable<string>
): Set<string> => {
  const reachable = new Set<string>();

  const visit = (id: string) => {
    if (reachable.has(id)) return;
    reachable.add(id);
    const asset = assets[id];
    if (asset?.type === 'csg') {
      const visitNode = (node: CsgTreeNode) => {
        if ('asset' in node) {
          visit(node.asset);
        } else {
          for (const child of node.children) visitNode(child);
        }
      };
      visitNode(asset.tree);
    }
  };

  for (const id of directRefs) visit(id);
  return reachable;
};

/**
 * Topological sort of assets: CSG assets are ordered after their dependencies.
 */
const topoSortAssets = (assets: Record<string, AssetDef>): string[] => {
  const visited = new Set<string>();
  const visiting = new Set<string>(); // cycle detection
  const order: string[] = [];

  const visit = (id: string) => {
    if (visited.has(id)) {
      return;
    }
    if (visiting.has(id)) throw new Error(`Circular asset dependency involving "${id}"`);
    visiting.add(id);

    const asset = assets[id];
    if (asset?.type === 'csg') {
      const visitNode = (node: CsgTreeNode) => {
        if ('asset' in node) {
          visit(node.asset);
        } else {
          for (const child of node.children) {
            visitNode(child);
          }
        }
      };
      visitNode(asset.tree);
    }

    visiting.delete(id);
    visited.add(id);
    order.push(id);
  };

  for (const id of Object.keys(assets)) {
    visit(id);
  }
  return order;
};

/**
 * Extract a prototype Object3D from a geoscript run result.
 * Returns null if no meshes were produced.
 */
const extractPrototype = (
  objects: { type: string; geometry?: THREE.BufferGeometry; transform?: THREE.Matrix4 }[]
): THREE.Object3D | null => {
  const meshes: THREE.Mesh[] = [];
  for (const obj of objects) {
    if (obj.type !== 'mesh') {
      continue;
    }

    const mesh = new THREE.Mesh(obj.geometry!, LEVEL_PLACEHOLDER_MAT);
    mesh.applyMatrix4(obj.transform!);
    meshes.push(mesh);
  }

  if (meshes.length === 0) {
    return null;
  } else if (meshes.length === 1) {
    return meshes[0];
  }

  const group = new THREE.Group();
  for (const mesh of meshes) {
    group.add(mesh);
  }
  return group;
};

/** The render wrapper imports the asset's exported mesh and pipes it through `render`. */
const RENDER_WRAPPER = 'import { mesh } from "code"\nmesh | apply_transforms | render';

/**
 * Runs a single geoscript code string through the geoscript worker and returns the
 * resulting prototype Object3D, or null if no meshes were produced or an error occurred.
 *
 * Used by the level editor to resolve newly-registered asset library entries without
 * requiring a full page reload.
 */
export const resolveGeoscriptAsset = async (code: string): Promise<THREE.Object3D | null> => {
  const executor = new GeoscriptExecutor();
  const promises = executor.submit([
    {
      id: '__resolveGeoscriptAsset__',
      modules: { code },
      code: RENDER_WRAPPER,
      includePrelude: false,
      asyncDeps: [],
      deps: [],
      collectMetadata: false,
    },
  ]);
  const result = await promises.get('__resolveGeoscriptAsset__')!;
  executor.terminate();

  if (result.error) {
    console.error('[levelDef] resolveGeoscriptAsset error:', result.error);
    return null;
  }

  return extractPrototype(result.objects as any);
};

/**
 * Build the modules map for running a single asset through the render wrapper.
 * Geoscript assets become a single "code" module. CSG assets generate their own
 * module tree via `generateCsgCode` and the CSG program itself becomes the "code" module.
 */
const buildAssetModules = (
  id: string,
  def: AssetDef,
  allAssets: Record<string, AssetDef>
): Record<string, string> => {
  if (def.type === 'geoscript') {
    return { code: def.code };
  }

  // CSG asset — generate the CSG program and collect its transitive modules
  const { modules, code } = generateCsgCode(def as CsgAssetDef, allAssets);
  return { ...modules, code };
};

/** djb2 hash over a string, returned as a hex string. */
const djb2Hash = (s: string): string => {
  let h = 5381;
  for (let i = 0; i < s.length; i++) {
    h = ((h << 5) + h + s.charCodeAt(i)) | 0;
  }
  return (h >>> 0).toString(16);
};

const computeCodeHash = (includePrelude: boolean, modules: Record<string, string>): string => {
  const content =
    (includePrelude ? '1' : '0') +
    Object.entries(modules)
      .sort(([a], [b]) => (a < b ? -1 : 1))
      .map(([k, v]) => `${k}:${v}`)
      .join('\0');
  return djb2Hash(content);
};

const shouldCollectMeta = (def: AssetDef, modules: Record<string, string>, includePrelude: boolean): boolean => {
  const meta = (def as Record<string, unknown>)._meta as GeoscriptAssetMeta | undefined;
  if (!meta) {
    return true;
  }
  if (meta.codeHash !== computeCodeHash(includePrelude, modules)) {
    return true;
  }
  return meta.count < 5;
};

const computeUpdatedMeta = (
  existing: GeoscriptAssetMeta | undefined,
  sampled: { runtimeMs: number; asyncDeps: string[] },
  modules: Record<string, string>,
  includePrelude: boolean
): GeoscriptAssetMeta => {
  const codeHash = computeCodeHash(includePrelude, modules);
  const hashChanged = !existing || existing.codeHash !== codeHash;
  const baseCount = hashChanged ? 0 : existing!.count;
  const baseRuntimeMs = hashChanged ? 0 : existing!.runtimeMs;

  const newCount = Math.min(baseCount + 1, 5);
  const newRuntimeMs = (baseRuntimeMs * baseCount + sampled.runtimeMs) / (baseCount + 1);

  const result: GeoscriptAssetMeta = { runtimeMs: newRuntimeMs, count: newCount, codeHash };
  if (sampled.asyncDeps.length > 0) {
    result.asyncDeps = sampled.asyncDeps;
  }
  return result;
};

/**
 * Run all geoscript and CSG assets via the executor, firing onResolved incrementally.
 * In dev mode, collects per-asset runtime metadata and POSTs updates to disk.
 */
const resolveScriptAssets = async (
  sortedIds: string[],
  assets: Record<string, AssetDef>,
  onResolved: (id: string, obj: THREE.Object3D) => void,
  sceneName: string
): Promise<void> => {
  const scriptIds = sortedIds.filter(id => assets[id].type === 'geoscript' || assets[id].type === 'csg');
  if (scriptIds.length === 0) {
    return;
  }

  const executor = new GeoscriptExecutor();

  const jobs: GeoscriptJob[] = scriptIds.map(id => {
    const def = assets[id];
    const modules = buildAssetModules(id, def, assets);
    const includePrelude = def.type === 'geoscript' ? (def.includePrelude ?? true) : false;
    const asyncDeps: string[] = ((def as Record<string, unknown>)._meta as GeoscriptAssetMeta | undefined)?.asyncDeps?.filter(d => d !== 'text_to_path') ?? [];
    const collectMetadata = dev && shouldCollectMeta(def, modules, includePrelude);
    return { id, modules, code: RENDER_WRAPPER, includePrelude, asyncDeps, deps: [], collectMetadata };
  });

  const promises = executor.submit(jobs);
  const metaUpdates: Record<string, GeoscriptAssetMeta> = {};

  for (const id of scriptIds) {
    promises.get(id)!.then(res => {
      if (res.error) {
        console.error(`[levelDef] ${assets[id].type} error for asset "${id}":`, res.error);
        return;
      }
      const prototype = extractPrototype(res.objects as any);
      if (!prototype) {
        console.warn(`[levelDef] Asset "${id}" produced no meshes`);
        return;
      }
      prototype.name = id;
      onResolved(id, prototype);

      if (dev && res.meta) {
        const def = assets[id];
        const modules = buildAssetModules(id, def, assets);
        const includePrelude = def.type === 'geoscript' ? (def.includePrelude ?? true) : false;
        const existing = (def as Record<string, unknown>)._meta as GeoscriptAssetMeta | undefined;
        const updated = computeUpdatedMeta(existing, res.meta, modules, includePrelude);
        if (JSON.stringify(updated) !== JSON.stringify(existing)) {
          metaUpdates[id] = updated;
        }
      }
    });
  }

  await Promise.all(promises.values());
  executor.terminate();

  if (dev && Object.keys(metaUpdates).length > 0) {
    fetch(`/level_editor/${sceneName}/asset-metadata`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(metaUpdates),
    }).catch(err => console.error('[levelDef] Failed to save asset metadata:', err));
  }
};

/**
 * Start loading a level definition. Returns a handle with:
 * - `objects`: resolves when all geometry is placed in the scene
 * - `complete`: resolves when all textures are done and materials assigned
 *
 * Objects are added to the scene incrementally as assets resolve, spreading GPU upload cost
 * across multiple render frames. Call this from `processLoadedScene` WITHOUT awaiting it so
 * the render loop can start immediately and textures upload concurrently.
 */
export const loadLevelDef = (
  viz: Viz,
  loadedWorld: THREE.Group,
  levelDef: LevelDef,
  quality: GraphicsQuality
): LevelLoadHandle => {
  // For each material, the set of texture keys it still needs before it can be built.
  const matTexPending = new Map<string, Set<string>>();
  // For each material, the textures loaded so far.
  const matTexLoaded = new Map<string, Map<string, THREE.Texture>>();
  // Generated material names still waiting for a factory to be registered.
  const pendingGeneratedMats = new Set<string>();
  const matAssignedCbs = new Map<string, (mesh: THREE.Mesh) => void>();
  const getTextureRefsForMaterial = (matName: string): string[] => {
    const def = levelDef.materials?.[matName];
    if (!def || def.type !== 'customShader' || !def.props) return [];
    const p = def.props;
    return [
      p.map,
      p.normalMap,
      p.roughnessMap,
      p.metalnessMap,
      p.lightMap,
      p.transmissionMap,
      p.clearcoatNormalMap,
    ].filter((x): x is string => typeof x === 'string');
  };

  for (const matName of Object.keys(levelDef.materials ?? {})) {
    const def = levelDef.materials![matName];
    if (def.type === 'generated') {
      // Generated materials are built only when setMaterialFactories() is called.
      pendingGeneratedMats.add(matName);
    } else {
      matTexPending.set(matName, new Set(getTextureRefsForMaterial(matName)));
      matTexLoaded.set(matName, new Map());
    }
  }

  // Flatten all leaf ObjectDefs for asset/material bookkeeping.
  const allLeafDefs = flattenLeaves(levelDef.objects);

  // Map from assetId → list of ObjectDefs that use it
  const assetToObjDefs = new Map<string, ObjectDef[]>();
  for (const objDef of allLeafDefs) {
    const list = assetToObjDefs.get(objDef.asset) ?? [];
    list.push(objDef);
    assetToObjDefs.set(objDef.asset, list);
  }

  // Warn about orphaned assets — assets that are defined but never referenced by any object
  // (directly or transitively through CSG). In dev, still resolve them so the level editor
  // can spawn new objects that reference assets not yet used in the scene.
  const reachableAssets = computeReachableAssets(levelDef.assets, assetToObjDefs.keys());
  if (dev) {
    for (const id of Object.keys(levelDef.assets)) {
      if (!reachableAssets.has(id)) {
        console.warn(`[levelDef] Asset "${id}" is defined but not referenced by any object`);
      }
    }
  }

  // Map from material name → object ids using it
  const matToObjIds = new Map<string, string[]>();
  for (const objDef of allLeafDefs) {
    if (objDef.material) {
      const list = matToObjIds.get(objDef.material) ?? [];
      list.push(objDef.id);
      matToObjIds.set(objDef.material, list);
    }
  }

  const placedObjects = new Map<string, LevelObject>();
  const builtMaterials = new Map<string, THREE.Material>();
  const loadedTextures = new Map<string, THREE.Texture>();
  const assetPrototypes = new Map<string, THREE.Object3D>();
  const allLevelObjects: LevelObject[] = [];
  const registeredPhysicsObjects = new Set<string>();

  // Pre-create the group hierarchy synchronously before async asset resolution.
  // Each leaf def gets mapped to its parent Object3D so placeObject can add to the right parent.
  const parentMap = new Map<string, THREE.Object3D>(); // objectId → parent container
  const parentGroupForLeaf = new Map<string, { group: LevelGroup; index: number }>();
  const nodeById = new Map<string, LevelSceneNode>();
  const rootNodes: LevelSceneNode[] = [];

  const preCreateGroups = (
    nodes: (ObjectDef | ObjectGroupDef)[],
    parent: THREE.Object3D,
    parentGroup: LevelGroup | null = null
  ): void => {
    for (let childIx = 0; childIx < nodes.length; childIx += 1) {
      const node = nodes[childIx];
      if (isObjectGroup(node)) {
        const group = new THREE.Group();
        applyTransform(group, node);
        parent.add(group);
        const levelGroup: LevelGroup = {
          id: node.id,
          object: group,
          def: node,
          children: [],
          generated: isGeneratedDef(node),
        };
        nodeById.set(node.id, levelGroup);
        if (parent === viz.scene) rootNodes.push(levelGroup);
        preCreateGroups(node.children, group, levelGroup);
        // Populate children after recursion so they're in the nodeById map
        for (const child of node.children) {
          const childNode = nodeById.get(child.id);
          if (childNode) levelGroup.children.push(childNode);
        }
      } else {
        parentMap.set(node.id, parent);
        if (parentGroup) {
          parentGroupForLeaf.set(node.id, { group: parentGroup, index: childIx });
        }
        // Leaf nodes are added to rootNodes/nodeById when placed (onAssetResolved)
      }
    }
  };

  preCreateGroups(levelDef.objects, viz.scene);

  // --- Instantiate lights ---
  const levelLights: LevelLight[] = [];
  for (const lightDef of levelDef.lights ?? []) {
    const levelLight = createLevelLight(lightDef, quality);
    addLevelLightToScene(viz.scene, levelLight);
    levelLights.push(levelLight);
  }

  // Track physics: push one callback to collisionWorldLoadedCbs; for objects placed
  // after physics is ready, register directly.
  let physicsReady = false;
  let resolvePhysicsWorldReady!: (fpCtx: PhysicsContext) => void;
  const physicsWorldReady = new Promise<PhysicsContext>(resolve => {
    resolvePhysicsWorldReady = resolve;
  });

  const maybeRegisterPhysics = (fpCtx: PhysicsContext, levelObj: LevelObject) => {
    if (
      registeredPhysicsObjects.has(levelObj.id) ||
      levelObj.def.nocollide ||
      levelObj.def.userData?.nocollide
    ) {
      return;
    }

    levelObj.object.traverse(child => {
      if (!(child instanceof THREE.Mesh)) {
        return;
      }

      // addTriMesh -> addCollisionObject reads mesh.position/quaternion/scale directly,
      // so temporarily expose the mesh's world-space transform during registration.
      withWorldSpaceTransform(child, mesh => fpCtx.addTriMesh(mesh));
    });
    registeredPhysicsObjects.add(levelObj.id);
  };

  viz.collisionWorldLoadedCbs.push(fpCtx => {
    physicsReady = true;
    resolvePhysicsWorldReady(fpCtx);
    for (const levelObj of allLevelObjects) {
      maybeRegisterPhysics(fpCtx, levelObj);
    }
  });

  const placeObject = (assetId: string, prototype: THREE.Object3D, objDef: ObjectDef) => {
    const clone = instantiateLevelObject(prototype, objDef, {
      builtMaterials,
      fallbackMaterial: LEVEL_PLACEHOLDER_MAT,
    });

    const parent = parentMap.get(objDef.id) ?? viz.scene;
    parent.add(clone);

    const levelObj: LevelObject = {
      id: objDef.id,
      assetId,
      object: clone,
      def: objDef,
      generated: isGeneratedDef(objDef),
    };
    allLevelObjects.push(levelObj);
    placedObjects.set(objDef.id, levelObj);
    nodeById.set(objDef.id, levelObj);
    const parentGroupEntry = parentGroupForLeaf.get(objDef.id);
    if (parentGroupEntry) {
      const insertAt = parentGroupEntry.group.children.findIndex(existingChild => {
        const existingIx = parentGroupEntry.group.def.children.findIndex(
          defChild => defChild.id === existingChild.id
        );
        return existingIx > parentGroupEntry.index;
      });
      if (insertAt === -1) {
        parentGroupEntry.group.children.push(levelObj);
      } else {
        parentGroupEntry.group.children.splice(insertAt, 0, levelObj);
      }
    } else if (parent === viz.scene) {
      const levelObjRootIndex = levelDef.objects.findIndex(rootNode => rootNode.id === levelObj.id);
      const insertAt = rootNodes.findIndex(existingRootNode => {
        const existingRootIx = levelDef.objects.findIndex(rootNode => rootNode.id === existingRootNode.id);
        return existingRootIx > levelObjRootIndex;
      });
      if (insertAt === -1) {
        rootNodes.push(levelObj);
      } else {
        rootNodes.splice(insertAt, 0, levelObj);
      }
    }

    // Fire onAssigned callback if the material factory registered one.
    if (objDef.material) {
      const cb = matAssignedCbs.get(objDef.material);
      if (cb) forEachMesh(clone, cb);
    }

    // Physics: register immediately if physics is already up, otherwise the batch callback covers it
    if (physicsReady) {
      maybeRegisterPhysics(viz.fpCtx!, levelObj);
    }
  };

  const onAssetResolved = (assetId: string, prototype: THREE.Object3D) => {
    assetPrototypes.set(assetId, prototype);
    for (const objDef of assetToObjDefs.get(assetId) ?? []) {
      placeObject(assetId, prototype, objDef);
    }
  };

  const tryBuildMaterial = (matName: string) => {
    const pending = matTexPending.get(matName);
    if (!pending || pending.size > 0) return;

    const def = levelDef.materials![matName];
    const texMap = matTexLoaded.get(matName)!;
    const mat = buildMaterial(def, texMap);
    builtMaterials.set(matName, mat);

    // Assign to any already-placed objects
    for (const objId of matToObjIds.get(matName) ?? []) {
      const levelObj = placedObjects.get(objId);
      if (levelObj) {
        assignMaterial(levelObj.object, mat);

        if (mat.userData.nonPermeable && physicsReady) {
          const fpCtx = viz.fpCtx;
          if (fpCtx) {
            levelObj.object.traverse(child => {
              if (!(child instanceof THREE.Mesh)) return;
              // Object-level override already made a decision — respect it.
              if (child.userData.nonPermeable !== undefined) return;
              const rigidBody = child.userData.rigidBody;
              if (rigidBody) {
                fpCtx.markBodyNonPermeable(rigidBody);
              }
            });
          } else {
            console.error(`\`fpCtx\` not ready when trying to mark "${levelObj.id}" as non-permeable`);
          }
        }
      }
    }
  };

  // For materials with no texture dependencies, they can be built immediately.
  for (const matName of Object.keys(levelDef.materials ?? {})) {
    tryBuildMaterial(matName);
  }

  // --- Start texture fetches ---

  const texturePool = new TextureFetchPool(8, 3);
  const textureEntries = Object.entries(levelDef.textures ?? {});

  const textureFetchPromises = textureEntries.map(([texName, texDef]) =>
    texturePool.load(texDef).then(
      tex => {
        // Upload to GPU immediately so cost is spread across the loading window
        // rather than spiking on the first render frame after loadingComplete resolves.
        viz.renderer.initTexture(tex);
        loadedTextures.set(texName, tex);

        // Mark this texture as loaded in every material that references it
        for (const [matName, pending] of matTexPending) {
          if (pending.has(texName)) {
            pending.delete(texName);
            matTexLoaded.get(matName)!.set(texName, tex);
            tryBuildMaterial(matName);
          }
        }
      },
      err => {
        console.error(`[levelDef] Texture "${texName}" failed after all retries:`, err);
        // Still advance: remove from pending sets so materials aren't blocked forever
        for (const [matName, pending] of matTexPending) {
          if (pending.has(texName)) {
            pending.delete(texName);
            tryBuildMaterial(matName);
          }
        }
      }
    )
  );

  // --- Topo-sort assets for dependency ordering ---

  const sortedAssetIds = dev
    ? topoSortAssets(levelDef.assets)
    : topoSortAssets(levelDef.assets).filter(id => reachableAssets.has(id));

  // --- Resolve gltf assets immediately (sync) ---

  for (const assetId of sortedAssetIds) {
    const assetDef = levelDef.assets[assetId];
    if (assetDef.type !== 'gltf') continue;
    const src = loadedWorld.getObjectByName(assetDef.meshName);
    if (!src) {
      console.warn(
        `[levelDef] gltf asset "${assetId}": mesh "${assetDef.meshName}" not found in loadedWorld`
      );
      continue;
    }
    onAssetResolved(assetId, src);
  }

  const geoscriptDone = resolveScriptAssets(sortedAssetIds, levelDef.assets, onAssetResolved, viz.sceneName);

  const objectsPromise: Promise<LevelObject[]> = geoscriptDone.then(() => allLevelObjects);
  const physicsRegistrationComplete = Promise.all([objectsPromise, physicsWorldReady]).then(
    ([levelObjects, fpCtx]) => {
      for (const levelObj of levelObjects) {
        maybeRegisterPhysics(fpCtx, levelObj);
      }
    }
  );
  viz.registerPhysicsStartupBarrier(physicsRegistrationComplete);

  const completePromise: Promise<void> = Promise.all([objectsPromise, ...textureFetchPromises]).then(
    () => void 0
  );

  if (dev) {
    // Mutable shadow of levelDef.assets, kept current as geo files are hot-reloaded.
    const mutableAssets: Record<string, AssetDef> = { ...levelDef.assets };

    objectsPromise.then(objects =>
      import('./LevelEditor.svelte').then(({ initLevelEditor }) => {
        const editor = initLevelEditor(
          viz,
          objects,
          viz.sceneName,
          assetPrototypes,
          builtMaterials,
          loadedTextures,
          levelDef,
          rootNodes,
          nodeById,
          levelLights
        );

        // Subscribe to geo file changes for in-place hot reload.
        const sse = new EventSource(`/level_editor/${viz.sceneName}/geo-watch`);
        sse.addEventListener('geo-change', async (event: Event) => {
          const { assetId, code } = JSON.parse((event as MessageEvent).data) as {
            assetId: string;
            code: string;
          };
          console.log(`[levelDef] Hot reloading geo asset "${assetId}"`);

          // Update the code in our mutable shadow.
          const existing = mutableAssets[assetId];
          if (!existing || existing.type !== 'geoscript') return;
          mutableAssets[assetId] = { type: 'geoscript', code, includePrelude: existing.includePrelude };

          // Re-run the changed asset and everything that transitively depends on it.
          const affected = getDownstreamAssets(assetId, buildReverseDeps(mutableAssets));
          const sortedAffected = topoSortAssets(mutableAssets).filter(
            id =>
              affected.has(id) && (mutableAssets[id].type === 'geoscript' || mutableAssets[id].type === 'csg')
          );
          if (sortedAffected.length === 0) return;

          await resolveScriptAssets(sortedAffected, mutableAssets, (id, newPrototype) => {
            assetPrototypes.set(id, newPrototype);

            // Swap every placed object that uses this asset in-place.
            for (const levelObj of allLevelObjects) {
              if (levelObj.assetId !== id) continue;

              const clone = instantiateLevelObject(newPrototype, levelObj.def, {
                builtMaterials,
                fallbackMaterial: LEVEL_PLACEHOLDER_MAT,
              });
              replaceLeafInstance(editor, levelObj, clone);
            }
          }, viz.sceneName);
        });

        viz.registerDestroyedCb(() => sse.close());
      })
    );
  }

  const parkourObjectsPromise = objectsPromise.then(levelObjects =>
    levelObjects.filter(obj => obj.def.parkour != null)
  );

  const emissiveBypassMeshesPromise = completePromise.then(() => {
    const meshes: THREE.Mesh[] = [];
    for (const levelObj of allLevelObjects) {
      const matName = levelObj.def.material;
      if (!matName) continue;
      const matDef = levelDef.materials?.[matName];
      if (!matDef?.emissiveBypass) continue;
      levelObj.object.traverse(child => {
        if (child instanceof THREE.Mesh) meshes.push(child);
      });
    }
    if (meshes.length > 0) {
      const bypassPass = viz.postprocessingController?.emissiveBypassPass;
      if (bypassPass) {
        for (const mesh of meshes) {
          bypassPass.addBypassMesh(mesh);
        }
      } else {
        console.warn(
          `[loadLevelDef] ${meshes.length} mesh(es) have emissiveBypass=true but no emissive bypass pass is configured. ` +
            `They will render normally without bypass treatment.`
        );
      }
    }
    return meshes;
  });

  const setMaterialFactories = (factories: Record<string, (viz: Viz) => MaterialFactoryResult>) => {
    for (const matName of [...pendingGeneratedMats]) {
      const factory = factories[matName];
      if (!factory) continue;
      pendingGeneratedMats.delete(matName);
      const result = factory(viz);
      const mat = result instanceof THREE.Material ? result : result.material;
      if (!(result instanceof THREE.Material)) {
        matAssignedCbs.set(matName, result.onAssigned);
      }
      builtMaterials.set(matName, mat);
      // Assign to any already-placed objects referencing this material
      for (const objId of matToObjIds.get(matName) ?? []) {
        const levelObj = placedObjects.get(objId);
        if (levelObj) {
          assignMaterial(levelObj.object, mat);
          const cb = matAssignedCbs.get(matName);
          if (cb) forEachMesh(levelObj.object, cb);
        }
      }
    }
  };

  // --- Behavior / entity wiring ---

  const setSceneRuntime = (runtime: SceneRuntime, sceneName: string) => {
    // Collect all leaf ObjectDefs that have behaviors or spawner defined
    const behaviorDefs = allLeafDefs.filter(d => d.behaviors?.length || d.spawner);

    // Lazy-load the virtual behaviors module only when actually needed. If there are no
    // behaviors to wire this turns into a no-op, but we still register the barrier so
    // the "scene runtime is fully wired before physics ticks" invariant holds uniformly.
    const behaviorsModuleP =
      behaviorDefs.length === 0
        ? Promise.resolve({} as Record<string, BehaviorFn>)
        : import('virtual:behaviors').then(m => m.default as Record<string, BehaviorFn>);

    const behaviorWiringComplete = Promise.all([objectsPromise, physicsWorldReady, behaviorsModuleP]).then(
      ([, , behaviorsModule]) => {
        const resolveBehaviorFn = (fnName: string): BehaviorFn | null => {
          // Try level-local first, then shared
          return behaviorsModule[`${sceneName}__${fnName}`] ?? behaviorsModule[fnName] ?? null;
        };

        const resolveBehaviorSpecs = (
          specs: BehaviorSpec[]
        ): { fn: BehaviorFn; params: Record<string, unknown> }[] => {
          const resolved: { fn: BehaviorFn; params: Record<string, unknown> }[] = [];
          for (const spec of specs) {
            const fn = resolveBehaviorFn(spec.fn);
            if (!fn) {
              console.warn(`[loadLevelDef] Unknown behavior "${spec.fn}" — skipping`);
              continue;
            }
            resolved.push({ fn, params: spec.params ?? {} });
          }
          return resolved;
        };

        for (const objDef of behaviorDefs) {
          const levelObj = placedObjects.get(objDef.id);
          if (!levelObj) {
            console.warn(`[loadLevelDef] Object "${objDef.id}" has behaviors but was not placed — skipping`);
            continue;
          }

          if (objDef.spawner) {
            const childBehaviors = resolveBehaviorSpecs(objDef.spawner.behaviors ?? []);
            runtime.registerSpawner(objDef.id, levelObj.object, {
              interval: objDef.spawner.interval,
              initialDelay: objDef.spawner.initialDelay,
              behaviors: childBehaviors,
            });
          } else if (objDef.behaviors) {
            const resolved = resolveBehaviorSpecs(objDef.behaviors);
            if (resolved.length === 0) continue;

            const rigidBody = (() => {
              let rb: BtRigidBody | undefined;
              levelObj.object.traverse(child => {
                if (child instanceof THREE.Mesh && child.userData.rigidBody && !rb) {
                  rb = child.userData.rigidBody;
                }
              });
              return rb;
            })();

            if (rigidBody) {
              rigidBody.setCollisionFlags(2); // CF_KINEMATIC_OBJECT
              rigidBody.setActivationState(4); // DISABLE_DEACTIVATION
            }

            const entity = runtime.createEntity(objDef.id, levelObj.object, rigidBody);

            for (const { fn, params } of resolved) {
              const behavior = fn(params, entity, runtime);
              entity.addBehavior(behavior);
            }
          }
        }
      }
    );

    // Register as a startup barrier so physics doesn't tick until behaviors are wired
    viz.registerPhysicsStartupBarrier(behaviorWiringComplete);
  };

  return {
    objects: objectsPromise,
    complete: completePromise,
    prototypes: assetPrototypes,
    builtMaterials,
    loadedTextures,
    rootNodes,
    nodeById,
    lights: levelLights,
    parkourObjects: parkourObjectsPromise,
    emissiveBypassMeshes: emissiveBypassMeshesPromise,
    setMaterialFactories,
    setSceneRuntime,
  };
};
