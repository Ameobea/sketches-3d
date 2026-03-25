import * as THREE from 'three';

import { runGeoscript } from 'src/geoscript/runner/geoscriptRunner';
import { WorkerManager } from 'src/geoscript/workerManager';
import type { Viz } from 'src/viz';
import type { GeoscriptAssetDef, LevelDef, ObjectDef } from './types';
import { buildMaterial } from './buildMaterial';
import { TextureFetchPool } from './texturePool';

export interface LevelObject {
  id: string;
  assetId: string;
  /**
   * The placed Three.js object. Will be a THREE.Mesh for single-mesh assets
   * (gltf or single-output geoscript) or a THREE.Group for multi-mesh geoscript output.
   * Use `.traverse()` to reach individual meshes for material assignment.
   */
  object: THREE.Object3D;
  def: ObjectDef;
}

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
}

// Shared placeholder used until real materials are built.
const PLACEHOLDER_MAT = new THREE.MeshStandardMaterial({ color: 0x888888 });

const buildGeoscriptMaterialsProxy = () =>
  new Proxy(
    {} as Record<string, { def: null; mat: { resolved: THREE.Material; promise: Promise<THREE.Material> } }>,
    {
      get: () => ({
        def: null,
        mat: { resolved: PLACEHOLDER_MAT, promise: Promise.resolve(PLACEHOLDER_MAT) },
      }),
    }
  ) as any;

const applyTransform = (object: THREE.Object3D, def: ObjectDef) => {
  const [px = 0, py = 0, pz = 0] = def.position ?? [];
  const [rx = 0, ry = 0, rz = 0] = def.rotation ?? [];
  const [sx = 1, sy = 1, sz = 1] = def.scale ?? [];
  object.position.set(px, py, pz);
  object.rotation.set(rx, ry, rz, 'YXZ');
  object.scale.set(sx, sy, sz);
};

const applyShadowFlags = (object: THREE.Object3D, def: ObjectDef) => {
  const castShadow = def.castShadow ?? true;
  const receiveShadow = def.receiveShadow ?? true;
  object.traverse(child => {
    if (child instanceof THREE.Mesh) {
      child.castShadow = castShadow;
      child.receiveShadow = receiveShadow;
    }
  });
};

const assignMaterial = (object: THREE.Object3D, mat: THREE.Material) => {
  object.traverse(child => {
    if (child instanceof THREE.Mesh) {
      child.material = mat;
    }
  });
};

/**
 * Run all geoscript assets sequentially in a single worker (sharing context for efficiency).
 * Calls `onResolved` for each asset immediately after it completes.
 *
 * TODO: will probably want to put this in a worker pool at some point
 */
const resolveGeoscriptAssets = async (
  entries: [string, GeoscriptAssetDef][],
  onResolved: (id: string, obj: THREE.Object3D) => void
): Promise<void> => {
  if (entries.length === 0) return;

  const workerManager = new WorkerManager();
  const repl = workerManager.getWorker();
  const ctxPtr = await repl.init();
  const materials = buildGeoscriptMaterialsProxy();

  for (const [id, def] of entries) {
    const runResult = await runGeoscript({
      code: def.code,
      ctxPtr,
      repl,
      materials,
      includePrelude: def.includePrelude ?? true,
    });

    if (runResult.error) {
      console.error(`[levelDef] Geoscript error for asset "${id}":`, runResult.error);
      continue;
    }

    const meshes: THREE.Mesh[] = [];
    for (const obj of runResult.objects) {
      if (obj.type !== 'mesh') continue;
      const mesh = new THREE.Mesh(obj.geometry, PLACEHOLDER_MAT);
      mesh.applyMatrix4(obj.transform);
      meshes.push(mesh);
    }

    if (meshes.length === 0) {
      console.warn(`[levelDef] Geoscript asset "${id}" produced no meshes`);
      continue;
    }

    let prototype: THREE.Object3D;
    if (meshes.length === 1) {
      prototype = meshes[0];
    } else {
      const group = new THREE.Group();
      for (const mesh of meshes) group.add(mesh);
      prototype = group;
    }

    onResolved(id, prototype);
  }

  workerManager.terminate();
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
export const loadLevelDef = (viz: Viz, loadedWorld: THREE.Group, levelDef: LevelDef): LevelLoadHandle => {
  // --- Texture tracking ---

  // For each material, the set of texture keys it still needs before it can be built.
  const matTexPending = new Map<string, Set<string>>();
  // For each material, the textures loaded so far.
  const matTexLoaded = new Map<string, Map<string, THREE.Texture>>();

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
    matTexPending.set(matName, new Set(getTextureRefsForMaterial(matName)));
    matTexLoaded.set(matName, new Map());
  }

  // --- Object tracking ---

  // Map from assetId → list of ObjectDefs that use it
  const assetToObjDefs = new Map<string, ObjectDef[]>();
  for (const objDef of levelDef.objects) {
    const list = assetToObjDefs.get(objDef.asset) ?? [];
    list.push(objDef);
    assetToObjDefs.set(objDef.asset, list);
  }

  // Map from material name → object ids using it
  const matToObjIds = new Map<string, string[]>();
  for (const objDef of levelDef.objects) {
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

  // --- Physics registration ---

  // Track physics: push one callback to collisionWorldLoadedCbs; for objects placed
  // after physics is ready, register directly.
  let physicsReady = false;
  const registerPhysics = (fpCtx: typeof viz.fpCtx, levelObj: LevelObject) => {
    if (levelObj.def.userData?.nocollide) return;
    levelObj.object.traverse(child => {
      if (child instanceof THREE.Mesh) {
        fpCtx!.addTriMesh(child);
      }
    });
  };

  viz.collisionWorldLoadedCbs.push(fpCtx => {
    physicsReady = true;
    for (const levelObj of allLevelObjects) {
      registerPhysics(fpCtx, levelObj);
    }
  });

  // --- Object placement ---

  const placeObject = (assetId: string, prototype: THREE.Object3D, objDef: ObjectDef) => {
    const clone = prototype.clone();
    applyTransform(clone, objDef);
    applyShadowFlags(clone, objDef);
    clone.userData = { ...clone.userData, ...(objDef.userData ?? {}), levelDefId: objDef.id };

    viz.scene.add(clone);

    const levelObj: LevelObject = { id: objDef.id, assetId, object: clone, def: objDef };
    allLevelObjects.push(levelObj);
    placedObjects.set(objDef.id, levelObj);

    // Physics: register immediately if physics is already up, otherwise the batch callback covers it
    if (physicsReady) {
      registerPhysics(viz.fpCtx, levelObj);
    }

    // Material: assign if already built
    if (objDef.material) {
      const mat = builtMaterials.get(objDef.material);
      if (mat) {
        assignMaterial(clone, mat);
      }
    }
  };

  const onAssetResolved = (assetId: string, prototype: THREE.Object3D) => {
    assetPrototypes.set(assetId, prototype);
    for (const objDef of assetToObjDefs.get(assetId) ?? []) {
      placeObject(assetId, prototype, objDef);
    }
  };

  // --- Material building ---

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

  // --- Resolve gltf assets immediately (sync) ---

  for (const [assetId, assetDef] of Object.entries(levelDef.assets)) {
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

  // --- Resolve geoscript assets async (streaming) ---

  const geoscriptEntries = Object.entries(levelDef.assets).filter(
    (e): e is [string, GeoscriptAssetDef] => e[1].type === 'geoscript'
  ) as [string, GeoscriptAssetDef][];

  const geoscriptDone = resolveGeoscriptAssets(geoscriptEntries, onAssetResolved);

  // --- Return handle ---

  const objectsPromise: Promise<LevelObject[]> = geoscriptDone.then(() => allLevelObjects);

  const completePromise: Promise<void> = Promise.all([objectsPromise, ...textureFetchPromises]).then(
    () => void 0
  );

  return { objects: objectsPromise, complete: completePromise, prototypes: assetPrototypes, builtMaterials, loadedTextures };
};
