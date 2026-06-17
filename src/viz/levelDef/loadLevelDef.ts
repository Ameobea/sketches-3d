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
  GeotoyCompositionAssetDef,
  LevelDef,
  ObjectDef,
  ObjectGroupDef,
} from './types';
import { compileTree, buildGizmoValues } from 'src/geoscript/treeCodegen';
import {
  bakeCompositionMeshes,
  resolveCompositionMaterial,
  type BakedCompositionMesh,
} from 'src/geoscript/runner/bakeComposition';
import type { GraphicsQuality } from 'src/viz/conf';
import type { SceneRuntime } from '../sceneRuntime';
import type { BehaviorFn } from '../sceneRuntime/types';
import { isObjectGroup, flattenLeaves, isGeneratedDef } from './levelDefTreeUtils';
import { type LevelObject, type LevelGroup, type LevelSceneNode, type LevelLight } from './levelSceneTypes';
import { replaceLeafInstance } from './editorStructuralOps';
import { buildCompositionChild } from './editorNodeFactory';
import { addLevelLightToScene, createLevelLight } from './levelLightUtils';
export type { LevelObject, LevelGroup, LevelSceneNode, LevelLight } from './levelSceneTypes';
import { buildMaterial, stampMaterialMetaUserData } from './buildMaterial';
import { CustomShaderMaterial, setSceneEnvironment } from 'src/viz/shaders/customShader';
import { generateGradientEnvironment, loadEnvironment } from 'src/viz/textureLoading';
import { extractHullInputVertices, type CollisionMeshOverride } from '../collisionShapes';
import { Entity } from '../sceneRuntime/Entity';
import { TextureFetchPool } from './texturePool';
import { generateCsgCode } from './csgCodeGen';

type PhysicsContext = NonNullable<Viz['fpCtx']>;

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
  prototypes: Map<string, THREE.Mesh>;
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
): THREE.Mesh | null => {
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
  }
  if (meshes.length > 1) {
    throw new Error(
      `Geoscript asset produced ${meshes.length} meshes; leaf objects must resolve to a single mesh`
    );
  }
  return meshes[0];
};

/**
 * Bakes the asset's root transform into vertices before rendering, so `obj.transform`
 * comes back as identity. Use for any path that hands the mesh to `instantiateLevelObject`,
 * which overwrites the prototype's Object3D transform with the level def's.
 */
export const BAKED_RENDER_WRAPPER = 'import { mesh } from "code"\nmesh | apply_transforms | render';

/** Leaves the root transform on `obj.transform`. Use when the consumer wants it separable (e.g. editor gizmo). */
export const UNBAKED_RENDER_WRAPPER = 'import { mesh } from "code"\nmesh | render';

/**
 * Runs a single geoscript code string through the geoscript worker and returns the
 * resulting prototype Object3D, or null if no meshes were produced or an error occurred.
 *
 * Used by the level editor to resolve newly-registered asset library entries without
 * requiring a full page reload.
 */
export const resolveGeoscriptAsset = async (code: string): Promise<THREE.Mesh | null> => {
  const executor = new GeoscriptExecutor();
  const promises = executor.submit([
    {
      id: '__resolveGeoscriptAsset__',
      modules: { code },
      code: BAKED_RENDER_WRAPPER,
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

const shouldCollectMeta = (
  def: AssetDef,
  modules: Record<string, string>,
  includePrelude: boolean
): boolean => {
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
  const rawRuntimeMs = (baseRuntimeMs * baseCount + sampled.runtimeMs) / (baseCount + 1);
  const newRuntimeMs = Math.round(rawRuntimeMs * 10) / 10;

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
  onResolved: (id: string, obj: THREE.Mesh) => void,
  sceneName: string,
  providedExecutor?: GeoscriptExecutor
): Promise<void> => {
  const scriptIds = sortedIds.filter(id => assets[id].type === 'geoscript' || assets[id].type === 'csg');
  if (scriptIds.length === 0) {
    return;
  }

  // Reuse a caller-owned executor when provided so the worker boot + wasm fetches
  // can overlap with the rest of page load.  Caller is responsible for terminating it.
  const executor = providedExecutor ?? new GeoscriptExecutor();
  const ownsExecutor = !providedExecutor;

  const jobs: GeoscriptJob[] = scriptIds.map(id => {
    const def = assets[id];
    const modules = buildAssetModules(id, def, assets);
    const includePrelude = def.type === 'geoscript' ? (def.includePrelude ?? true) : false;
    const asyncDeps: string[] =
      ((def as Record<string, unknown>)._meta as GeoscriptAssetMeta | undefined)?.asyncDeps?.filter(
        d => d !== 'text_to_path'
      ) ?? [];
    const collectMetadata = dev && shouldCollectMeta(def, modules, includePrelude);
    return { id, modules, code: BAKED_RENDER_WRAPPER, includePrelude, asyncDeps, deps: [], collectMetadata };
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
  if (ownsExecutor) {
    executor.terminate();
  }

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
  const matTexPending = new Map<string, Set<string>>();
  const matTexLoaded = new Map<string, Map<string, THREE.Texture>>();
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
      p.pomHeightMap,
    ].filter((x): x is string => typeof x === 'string');
  };

  for (const matName of Object.keys(levelDef.materials ?? {})) {
    const def = levelDef.materials![matName];
    if (def.type === 'generated') {
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
  // Baked mesh prototypes per composition asset, retained so the editor can re-expand a
  // placement (add-from-library / clone) without re-running the geoscript worker.
  const compositionBaked = new Map<string, BakedCompositionMesh[]>();
  const builtMaterials = new Map<string, THREE.Material>();
  const loadedTextures = new Map<string, THREE.Texture>();
  const assetPrototypes = new Map<string, THREE.Mesh>();
  const allLevelObjects: LevelObject[] = [];
  const registeredPhysicsObjects = new Set<string>();

  // Per-asset collision-hull mesh.  Populated by `resolveAssetPrototype` for assets with
  // `colliderShape: 'convexHull'` once Manifold has computed the hull; absent for trimesh
  // assets, which derive their collision data from the visual mesh at registration time.
  const assetCollisionMeshes = new Map<string, CollisionMeshOverride>();

  // Per-asset latest resolution promise.  Doubles as a supersession token: when a
  // hot-reload kicks off a new resolution, an older in-flight resolution that finishes
  // afterward sees a different promise here and drops its result.
  const assetResolutions = new Map<string, Promise<void>>();

  // The geoscript worker hosts Manifold (used for convex-hull derivation) alongside the
  // geoscript runtime.  Acquire it lazily — only if some asset actually needs the worker
  // (geoscript/csg assets or assets with `colliderShape: 'convexHull'`).
  const needsExecutor = Object.values(levelDef.assets).some(
    def =>
      def.type === 'geoscript' ||
      def.type === 'csg' ||
      def.type === 'geotoyComposition' ||
      def.colliderShape === 'convexHull'
  );
  const sharedExecutor: GeoscriptExecutor | undefined = needsExecutor
    ? viz.getGeoscriptExecutor()
    : undefined;

  /**
   * Adopt `mesh` as the asset's prototype and (if its `colliderShape` requires it) kick
   * off async computation of its collision hull.  Returns a single promise that resolves
   * when the asset is fully ready — i.e., the collision hull (if any) has landed and
   * physics registration may proceed.
   *
   * Idempotent and supersession-aware: a newer call wins; results from an older in-flight
   * call are dropped if they land after a newer call.  Used by initial load, by hot-reload
   * of geo source files, and by the editor's CSG asset re-resolve path.
   */
  const resolveAssetPrototype = (
    assetId: string,
    mesh: THREE.Mesh,
    assetsRef: Record<string, AssetDef> = levelDef.assets
  ): Promise<void> => {
    assetPrototypes.set(assetId, mesh);
    // Drop any stale hull entry now; will be repopulated below if applicable.
    assetCollisionMeshes.delete(assetId);

    const assetDef = assetsRef[assetId];
    const needsHull = assetDef?.colliderShape === 'convexHull';
    if (!needsHull) {
      const ready = Promise.resolve();
      assetResolutions.set(assetId, ready);
      return ready;
    }
    if (!sharedExecutor) {
      // `needsExecutor` should have flagged this asset, so this is a programming error.
      console.error(`[levelDef] convexHull asset "${assetId}" but no executor available`);
      const ready = Promise.resolve();
      assetResolutions.set(assetId, ready);
      return ready;
    }

    let inputVerts: Float32Array;
    try {
      inputVerts = extractHullInputVertices(mesh.geometry);
    } catch (err) {
      console.warn(`[levelDef] convexHull asset "${assetId}": skipping hull —`, err);
      const ready = Promise.resolve();
      assetResolutions.set(assetId, ready);
      return ready;
    }
    const promise: Promise<void> = sharedExecutor
      .computeConvexHull(inputVerts)
      .then(hull => {
        // Only adopt this result if no newer resolution has superseded it.
        if (assetResolutions.get(assetId) === promise) {
          assetCollisionMeshes.set(assetId, { verts: hull.verts, indices: hull.indices });
        }
      })
      .catch(err => {
        console.error(`[levelDef] Failed to compute convex hull for asset "${assetId}":`, err);
      });
    assetResolutions.set(assetId, promise);
    return promise;
  };

  // Pre-create the group hierarchy synchronously before async asset resolution.
  // Each leaf def gets mapped to its parent Object3D so placeObject can add to the right parent.
  const parentMap = new Map<string, THREE.Object3D>(); // objectId → parent container
  const parentGroupForLeaf = new Map<string, { group: LevelGroup; index: number }>();
  // For every node that's a child of some group in the input def, its sibling index
  // within that group. Lets async leaf placement find the right insertion slot
  // without consulting the runtime def (whose `children` is intentionally omitted).
  const inputChildIndex = new Map<string, number>();
  const nodeById = new Map<string, LevelSceneNode>();
  const rootNodes: LevelSceneNode[] = [];
  // Composition placements: each referencing ObjectDef pre-creates an (initially empty)
  // editable LevelGroup; its read-only children are filled once the tree is baked.
  const compositionGroups = new Map<string, { group: LevelGroup; objDef: ObjectDef }>();
  const isCompositionAsset = (assetKey: string): boolean =>
    levelDef.assets[assetKey]?.type === 'geotoyComposition';

  const preCreateGroups = (
    nodes: (ObjectDef | ObjectGroupDef)[],
    parent: THREE.Object3D,
    parentGroup: LevelGroup | null = null
  ): void => {
    for (let childIx = 0; childIx < nodes.length; childIx += 1) {
      const node = nodes[childIx];
      if (parentGroup) inputChildIndex.set(node.id, childIx);
      if (!isObjectGroup(node) && isCompositionAsset(node.asset)) {
        const group = new THREE.Group();
        applyTransform(group, node);
        parent.add(group);
        const levelGroup: LevelGroup = {
          id: node.id,
          object: group,
          def: {
            id: node.id,
            position: node.position,
            rotation: node.rotation,
            scale: node.scale,
            userData: node.userData,
          },
          children: [],
          generated: isGeneratedDef(node),
          compositionDef: node,
        };
        nodeById.set(node.id, levelGroup);
        if (parent === viz.scene) rootNodes.push(levelGroup);
        compositionGroups.set(node.id, { group: levelGroup, objDef: node });
      } else if (isObjectGroup(node)) {
        const group = new THREE.Group();
        applyTransform(group, node);
        parent.add(group);
        const { children: _omitChildren, ...body } = node;
        const levelGroup: LevelGroup = {
          id: node.id,
          object: group,
          def: body,
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

  const levelLights: LevelLight[] = [];
  for (const lightDef of levelDef.lights ?? []) {
    const levelLight = createLevelLight(lightDef, quality);
    addLevelLightToScene(viz.scene, levelLight);
    levelLights.push(levelLight);
  }

  const envDef = levelDef.environment;
  if (envDef) {
    if (envDef.kind === 'gradient') {
      const { envMap, background } = generateGradientEnvironment(viz.renderer, {
        skyColor: envDef.skyColor ?? 0xffffff,
        horizonColor: envDef.horizonColor ?? 0x888888,
        groundColor: envDef.groundColor ?? 0x444444,
      });
      setSceneEnvironment(viz.scene, { envMap, intensity: envDef.intensity ?? 1 });
      if (envDef.setBackground !== false) viz.scene.background = background;
    } else {
      // Async so the level load isn't blocked on the image fetch.
      const envLoader = new THREE.ImageBitmapLoader();
      void loadEnvironment(viz.renderer, envLoader, envDef.url).then(
        ({ envMap, background }) => {
          setSceneEnvironment(viz.scene, { envMap, intensity: envDef.intensity ?? 1 });
          if (envDef.setBackground !== false) viz.scene.background = background;
        },
        err => console.error(`[levelDef] Failed to load environment "${envDef.url}":`, err)
      );
    }
  }

  if (levelDef.audio) {
    if (levelDef.audio.sfxDefs) {
      viz.sfxManager.registerSfxDefs(levelDef.audio.sfxDefs);
    }
    for (const loop of levelDef.audio.spatialLoops ?? []) {
      viz.sfxManager.playSpatialLoop(loop.sfx, {
        pos: loop.pos,
        gain: loop.gain,
        playbackRate: loop.playbackRate,
        xfade: loop.xfade,
        filter: loop.filter,
        refDistance: loop.refDistance,
        rolloff: loop.rolloff,
        cullThreshold: loop.cullThreshold,
      });
    }
  }

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

    // Placement is gated on `resolveAssetPrototype` resolution, so by the time a
    // `levelObj` exists its asset's collision data (if any) is already in the cache.
    const collisionMeshOverride = assetCollisionMeshes.get(levelObj.assetId);
    withWorldSpaceTransform(levelObj.object, mesh =>
      fpCtx.addTriMesh(mesh, 'static', levelObj.entity, collisionMeshOverride)
    );
    registeredPhysicsObjects.add(levelObj.id);
  };

  viz.collisionWorldLoadedCbs.push(fpCtx => {
    physicsReady = true;
    resolvePhysicsWorldReady(fpCtx);
    for (const levelObj of allLevelObjects) {
      maybeRegisterPhysics(fpCtx, levelObj);
    }
  });

  // Material `userData` channels that propagate to the owning Entity if the entity didn't
  // already get an explicit value from the object def.  Centralized so the three sites that
  // bind a material to an entity (placement w/ already-built mat, post-texture build,
  // generated-mat factory) all stay in sync.  Add new propagated channels here.
  const propagateMatUserDataToEntity = (mat: THREE.Material, entity: Entity) => {
    const ud = mat.userData;
    if (ud.boostSurfaceConfig && !entity.boostSurfaceConfig) {
      entity.setBoostSurfaceConfig(ud.boostSurfaceConfig);
    }
    if (ud.externalVelocityAirDampingFactor && !entity.externalVelocityAirDampingFactor) {
      entity.externalVelocityAirDampingFactor = ud.externalVelocityAirDampingFactor;
    }
    if (ud.externalVelocityGroundDampingFactor && !entity.externalVelocityGroundDampingFactor) {
      entity.externalVelocityGroundDampingFactor = ud.externalVelocityGroundDampingFactor;
    }
  };

  const placeObject = (assetId: string, prototype: THREE.Mesh, objDef: ObjectDef) => {
    const clone = instantiateLevelObject(prototype, objDef, {
      builtMaterials,
      fallbackMaterial: LEVEL_PLACEHOLDER_MAT,
    });

    const parent = parentMap.get(objDef.id) ?? viz.scene;
    parent.add(clone);

    const entity = new Entity(viz, objDef.id, clone);
    if (objDef.nonPermeable !== undefined) {
      entity.nonPermeable = objDef.nonPermeable;
    }
    if (objDef.parkour?.boostSurface) {
      entity.setBoostSurfaceConfig(objDef.parkour.boostSurface);
    }
    if (objDef.externalVelocityAirDampingFactor) {
      entity.externalVelocityAirDampingFactor = objDef.externalVelocityAirDampingFactor;
    }
    if (objDef.externalVelocityGroundDampingFactor) {
      entity.externalVelocityGroundDampingFactor = objDef.externalVelocityGroundDampingFactor;
    }
    // Texture-less materials build before any placement, so tryBuildMaterial's post-build
    // propagation runs against an empty placedObjects map; cover the already-built case here.
    const earlyMat = objDef.material ? builtMaterials.get(objDef.material) : undefined;
    if (earlyMat) {
      propagateMatUserDataToEntity(earlyMat, entity);
    }
    const levelObj: LevelObject = {
      id: objDef.id,
      assetId,
      object: clone,
      def: objDef,
      generated: isGeneratedDef(objDef),
      entity,
    };
    allLevelObjects.push(levelObj);
    placedObjects.set(objDef.id, levelObj);
    nodeById.set(objDef.id, levelObj);
    const parentGroupEntry = parentGroupForLeaf.get(objDef.id);
    if (parentGroupEntry) {
      const insertAt = parentGroupEntry.group.children.findIndex(existingChild => {
        const existingIx = inputChildIndex.get(existingChild.id) ?? -1;
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

  // Tracks every initial-load placement chain so the completion barrier can await both
  // the geoscript run *and* the post-resolution hull/placement steps.
  const initialPlacementPromises: Promise<void>[] = [];

  const onAssetResolved = (assetId: string, prototype: THREE.Mesh) => {
    const promise = resolveAssetPrototype(assetId, prototype).then(() => {
      for (const objDef of assetToObjDefs.get(assetId) ?? []) {
        placeObject(assetId, prototype, objDef);
      }
    });
    initialPlacementPromises.push(promise);
  };

  // --- Composition assets: run the tree headlessly, bake meshes, expand each placement ---

  const warnedUnmappedComposition = new Set<string>();
  const levelMaterialNames = new Set(Object.keys(levelDef.materials ?? {}));
  const resolveChildMaterial = (
    def: GeotoyCompositionAssetDef,
    objDef: ObjectDef,
    geotoyName: string,
    assetId: string
  ): string | undefined => {
    const { name, unmapped } = resolveCompositionMaterial(
      levelMaterialNames,
      def.materialMap,
      objDef.material,
      geotoyName
    );
    if (unmapped) {
      const key = `${assetId}:${geotoyName}`;
      if (!warnedUnmappedComposition.has(key)) {
        warnedUnmappedComposition.add(key);
        console.warn(
          `[levelDef] composition "${assetId}": material "${geotoyName}" is unmapped; falling back`
        );
      }
    }
    return name;
  };

  const placeCompositionChild = (
    group: LevelGroup,
    objDef: ObjectDef,
    assetId: string,
    def: GeotoyCompositionAssetDef,
    baked: BakedCompositionMesh,
    childIndex: number
  ) => {
    const levelObj = buildCompositionChild({ viz, builtMaterials }, objDef, baked, childIndex, g =>
      resolveChildMaterial(def, objDef, g, assetId)
    );
    group.object.add(levelObj.object);

    const matName = levelObj.def.material;
    const builtMat = matName ? builtMaterials.get(matName) : undefined;
    if (builtMat) propagateMatUserDataToEntity(builtMat, levelObj.entity);

    allLevelObjects.push(levelObj);
    placedObjects.set(levelObj.id, levelObj);
    nodeById.set(levelObj.id, levelObj);
    group.children.push(levelObj);

    // Register for the post-texture material build, or fire the assigned-cb if already built.
    if (matName) {
      const list = matToObjIds.get(matName) ?? [];
      list.push(levelObj.id);
      matToObjIds.set(matName, list);
      if (builtMat) {
        const cb = matAssignedCbs.get(matName);
        if (cb) forEachMesh(levelObj.object, cb);
      }
    }

    if (physicsReady) maybeRegisterPhysics(viz.fpCtx!, levelObj);
  };

  const resolveCompositionAssets = async (): Promise<void> => {
    const compIds = sortedAssetIds.filter(id => levelDef.assets[id].type === 'geotoyComposition');
    if (compIds.length === 0) return;
    if (!sharedExecutor) {
      console.error('[levelDef] geotoyComposition assets present but no geoscript executor available');
      return;
    }

    const prelude = await sharedExecutor.getPrelude();

    const jobs: GeoscriptJob[] = compIds.map(id => {
      const def = levelDef.assets[id] as GeotoyCompositionAssetDef;
      if (def.rootNodeName) {
        console.warn(
          `[levelDef] composition "${id}": rootNodeName scoping is not supported in v1; importing the whole tree`
        );
      }
      const compiled = compileTree(def.tree);
      const preludeEjected = def.preludeEjected ?? false;
      const ambientSources: string[] = [];
      if (!preludeEjected) ambientSources.push(prelude);
      if (def.tree.globalsSource.trim().length > 0) ambientSources.push(def.tree.globalsSource);
      const asyncDeps = def._meta?.asyncDeps?.filter(d => d !== 'text_to_path') ?? [];
      return {
        id,
        modules: compiled.modules,
        code: compiled.rootSource,
        includePrelude: !preludeEjected,
        ambientSources,
        gizmoValues: buildGizmoValues(def.tree),
        asyncDeps,
        deps: [],
        collectMetadata: false,
      };
    });

    const promises = sharedExecutor.submit(jobs);
    await Promise.all(
      compIds.map(id =>
        promises.get(id)!.then(res => {
          if (res.error) {
            console.error(`[levelDef] composition asset "${id}" error:`, res.error);
            return;
          }
          const def = levelDef.assets[id] as GeotoyCompositionAssetDef;
          const baked = bakeCompositionMeshes(def.tree, res.objects);
          compositionBaked.set(id, baked);
          if (baked.length === 0) console.warn(`[levelDef] composition asset "${id}" produced no meshes`);
          for (const objDef of assetToObjDefs.get(id) ?? []) {
            const entry = compositionGroups.get(objDef.id);
            if (!entry) continue;
            baked.forEach((bm, i) => placeCompositionChild(entry.group, objDef, id, def, bm, i));
          }
        })
      )
    );
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

        // Upgrade the entity's material class now that the real material has arrived.
        if (mat instanceof CustomShaderMaterial && mat.materialClass !== undefined) {
          levelObj.entity.setMaterialClass(mat.materialClass);
        }

        propagateMatUserDataToEntity(mat, levelObj.entity);

        if (mat.userData.nonPermeable && physicsReady) {
          const fpCtx = viz.fpCtx;
          if (fpCtx) {
            if (levelObj.entity.nonPermeable === undefined && levelObj.entity.body) {
              fpCtx.markBodyNonPermeable(levelObj.entity.body);
            }
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
    if (!(src instanceof THREE.Mesh)) {
      throw new Error(
        `[levelDef] gltf asset "${assetId}": "${assetDef.meshName}" resolved to ${src.type}, expected Mesh`
      );
    }
    onAssetResolved(assetId, src);
  }

  const geoscriptDone = resolveScriptAssets(
    sortedAssetIds,
    levelDef.assets,
    onAssetResolved,
    viz.sceneName,
    sharedExecutor
  );

  // Composition jobs share the worker ctx with script assets, so run them after the script
  // batch settles (concurrent `submit()` calls would interleave resets on the shared ctx).
  const compositionsDone = geoscriptDone.then(() => resolveCompositionAssets());

  // `geoscriptDone` resolves after every `onAssetResolved` has been called (so every
  // `initialPlacementPromises` entry exists); awaiting those + composition expansion then
  // gives us "all assets resolved + all hulls computed + all objects placed".
  const objectsPromise: Promise<LevelObject[]> = Promise.all([geoscriptDone, compositionsDone])
    .then(() => Promise.all(initialPlacementPromises))
    .then(() => allLevelObjects);
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
          levelLights,
          assetCollisionMeshes,
          resolveAssetPrototype,
          compositionBaked
        );

        // Subscribe to geo file changes for in-place hot reload.
        const sse = new EventSource(`/level_editor/${viz.sceneName}/geo-watch`);
        sse.addEventListener('geo-change', async (event: Event) => {
          const { assetId, code } = JSON.parse((event as MessageEvent).data) as {
            assetId: string;
            code: string;
          };
          console.log(`[levelDef] Hot reloading geo asset "${assetId}"`);

          // Update the code in our mutable shadow.  Preserve every field that affects
          // physics/collision derivation (notably `colliderShape`) so a hot-reload of a
          // `convexHull` asset doesn't get demoted back to trimesh after the swap.
          const existing = mutableAssets[assetId];
          if (!existing || existing.type !== 'geoscript') return;
          mutableAssets[assetId] = { ...existing, code };

          // Re-run the changed asset and everything that transitively depends on it.
          const affected = getDownstreamAssets(assetId, buildReverseDeps(mutableAssets));
          const sortedAffected = topoSortAssets(mutableAssets).filter(
            id =>
              affected.has(id) && (mutableAssets[id].type === 'geoscript' || mutableAssets[id].type === 'csg')
          );
          if (sortedAffected.length === 0) return;

          await resolveScriptAssets(
            sortedAffected,
            mutableAssets,
            (id, newPrototype) => {
              // Adopt the new prototype + recompute its hull (if any) before swapping
              // instances so syncPhysics picks up the new hull rather than a stale one.
              resolveAssetPrototype(id, newPrototype, mutableAssets).then(() => {
                for (const levelObj of allLevelObjects) {
                  if (levelObj.assetId !== id) continue;

                  const clone = instantiateLevelObject(newPrototype, levelObj.def, {
                    builtMaterials,
                    fallbackMaterial: LEVEL_PLACEHOLDER_MAT,
                  });
                  replaceLeafInstance(editor, levelObj, clone);
                }
              });
            },
            viz.sceneName,
            sharedExecutor
          );
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
    viz.postprocessingController?.rescanPomMeshes();
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
      const matDef = levelDef.materials![matName];
      if (matDef.type === 'generated') {
        stampMaterialMetaUserData(matDef, mat);
      }
      builtMaterials.set(matName, mat);
      // Assign to any already-placed objects referencing this material
      for (const objId of matToObjIds.get(matName) ?? []) {
        const levelObj = placedObjects.get(objId);
        if (levelObj) {
          assignMaterial(levelObj.object, mat);
          propagateMatUserDataToEntity(mat, levelObj.entity);
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

            // Promote the entity's body to kinematic so behaviors can drive it
            // via setTransform without the physics engine fighting back.
            if (levelObj.entity.body) {
              levelObj.entity.body.setCollisionFlags(2); // CF_KINEMATIC_OBJECT
              levelObj.entity.body.setActivationState(4); // DISABLE_DEACTIVATION
            }

            const entity = runtime.adoptEntity(levelObj.entity);

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
