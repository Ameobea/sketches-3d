import * as THREE from 'three';
import type * as Comlink from 'comlink';
import type {
  RunGeoscriptOptions,
  GeoscriptRunResult,
  RunStats,
  GeneratedObject,
  MatEntry,
  RenderedGizmo,
} from './types';
import { buildLight, fitAutoShadowFrusta } from 'src/viz/scenes/geoscriptPlayground/lights';
import { getUVUnwrapWorker } from '../uvUnwrapWorker';
import { FallbackMat, HiddenMat, LineMat, NormalMat, WireframeMat } from '../materials';
import type { RenderedObject } from './types';
import type { GeoscriptAsyncDeps, GeoscriptWorkerMethods } from '../geoscriptWorker.worker';
import { bitmaskToAsyncDepNames } from '../asyncDepBits';
import type { TreeDef } from '../geotoyAPIClient';
import { ROOT_NODE_NAME } from '../geotoyAPIClient';
import { buildParentMap } from 'src/viz/scenes/geoscriptPlayground/treeOps';
import { buildWorldMatrixCache, instancePathKey, type NodeWorldInstance } from './worldMatrixCache';
export { buildWorldMatrixCache, instancePathKey };
export type { NodeWorldInstance, WorldMatrixCache } from './worldMatrixCache';

const buildEmptyRunStats = (): RunStats => ({
  runtimeMs: 0,
  renderedMeshCount: 0,
  renderedPathCount: 0,
  renderedLightCount: 0,
  totalVtxCount: 0,
  totalFaceCount: 0,
  asyncDeps: [],
});

const getOverrideMat = (materialOverride: 'wireframe' | 'wireframe-xray' | 'normal' | null | undefined) => {
  if (materialOverride === 'wireframe' || materialOverride === 'wireframe-xray') {
    return WireframeMat;
  }
  if (materialOverride === 'normal') {
    return NormalMat;
  }
  return null;
};

/**
 * If `err` is a `__GEOTOY_UNINITIALIZED_MODULE__:<dep>` sentinel, init the dep and
 * return true so the caller can retry. Returns false otherwise.
 */
const tryInitAsyncDepFromErr = async (
  err: string,
  repl: Comlink.Remote<GeoscriptWorkerMethods>
): Promise<boolean> => {
  if (!err.includes('__GEOTOY_UNINITIALIZED_MODULE__:')) return false;
  const depName = /__GEOTOY_UNINITIALIZED_MODULE__:(\w+)/.exec(err)?.[1];
  if (!depName) {
    console.error('Unrecognized __GEOTOY_UNINITIALIZED_MODULE__ format:', err);
    return false;
  }
  const deps: GeoscriptAsyncDeps = {};
  deps[depName as keyof GeoscriptAsyncDeps] = true;
  const argsByKey: Partial<Record<keyof GeoscriptAsyncDeps, string[]>> = {};
  if (err.includes('||__||')) {
    argsByKey[depName as keyof GeoscriptAsyncDeps] = err.split('||__||').slice(1);
  }
  await repl.initAsyncDeps(deps, argsByKey);
  return true;
};

export const runGeoscript = async ({
  code,
  ctxPtr,
  repl,
  materials = {},
  includePrelude,
  materialOverride,
  renderMode = false,
  modules,
  ambientSources,
  gizmoValues,
}: RunGeoscriptOptions): Promise<GeoscriptRunResult> => {
  await repl.reset(ctxPtr);

  if (modules && Object.keys(modules).length > 0) {
    await repl.setModuleSources(ctxPtr, modules);
  }

  if (ambientSources !== undefined) {
    try {
      await repl.setAmbientScope(ctxPtr, ambientSources);
    } catch (err) {
      const errStr = err instanceof Error ? err.message : String(err);
      if (await tryInitAsyncDepFromErr(errStr, repl)) {
        return runGeoscript({
          code,
          ctxPtr,
          repl,
          materials,
          includePrelude,
          materialOverride,
          renderMode,
          modules,
          ambientSources,
          gizmoValues,
        });
      }
      return {
        objects: [],
        stats: buildEmptyRunStats(),
        error: `Error building ambient scope: ${err}`,
        gizmos: [],
      };
    }
  }

  // Always sent (default `{}`) so a previous run's handle values can't leak in.
  await repl.setGizmoValues(ctxPtr, gizmoValues ?? {});

  let evalResult: { durationMs: number; usedDepsBitmask: number } = { durationMs: 0, usedDepsBitmask: 0 };
  try {
    evalResult = await repl.eval(ctxPtr, code, includePrelude);
  } catch (evalErr) {
    const errorMessage = `Error evaluating code: ${evalErr}`;
    console.error(errorMessage, evalErr);
    return {
      objects: [],
      stats: buildEmptyRunStats(),
      error: errorMessage,
      gizmos: [],
    };
  }

  const err = (await repl.getErr(ctxPtr)) || null;
  if (err) {
    // Safety net: if a dep wasn't pre-loaded, load it now and re-run.
    // text_to_path always goes through this path since its args are runtime values.
    if (await tryInitAsyncDepFromErr(err, repl)) {
      return runGeoscript({
        code,
        ctxPtr,
        repl,
        materials,
        includePrelude,
        materialOverride,
        renderMode,
        modules,
        ambientSources,
      });
    }
    return {
      objects: [],
      stats: buildEmptyRunStats(),
      error: err,
      gizmos: [],
    };
  }

  const stats: RunStats = {
    ...buildEmptyRunStats(),
    runtimeMs: evalResult.durationMs,
    asyncDeps: bitmaskToAsyncDepNames(evalResult.usedDepsBitmask),
  };
  const renderedObjects: GeneratedObject[] = [];

  const overrideMat = getOverrideMat(materialOverride);

  stats.renderedMeshCount = await repl.getRenderedMeshCount(ctxPtr);
  for (let i = 0; i < stats.renderedMeshCount; i += 1) {
    const {
      transform,
      verts: initialVerts,
      indices: initialIndices,
      normals,
      uvs: meshUvs,
      tangents: meshTangents,
      material: materialName,
      sourceModule,
      meshId,
    } = await repl.getRenderedMesh(ctxPtr, i);

    let verts = initialVerts;
    let indices = initialIndices;
    let uvs: Float32Array | null = meshUvs ?? null;
    let tangents: Float32Array | null = meshTangents ?? null;
    let didBffUnwrap = false;
    const matLookup = materials[materialName] ?? {
      def: null,
      mat: { resolved: FallbackMat, promise: Promise.resolve(FallbackMat) },
    };
    const { def: matDef, mat: mat } = matLookup;

    const uvUnwrapParams = (() => {
      if (!!overrideMat || matDef?.textureMapping?.type !== 'uv') {
        return null;
      }

      return {
        nCones: matDef.textureMapping.numCones,
        flattenToDisk: matDef.textureMapping.flattenToDisk,
        mapToSphere: matDef.textureMapping.mapToSphere,
        enableUVIslandRotation: matDef.textureMapping.enableUVIslandRotation,
      };
    })();

    // TODO: would be good to parallelize this with other work
    if (uvUnwrapParams) {
      try {
        const uvUnwrapWorker = await getUVUnwrapWorker();
        const unwrapRes = await uvUnwrapWorker.uvUnwrap(verts, indices, uvUnwrapParams);

        if (unwrapRes.type === 'error') {
          throw new Error(unwrapRes.message);
        }

        const { uvs: unwrappedUVs, verts: unwrappedVerts, indices: unwrappedIndices } = unwrapRes.out;
        uvs = unwrappedUVs;
        verts = unwrappedVerts;
        indices = unwrappedIndices;
        tangents = null; // re-mesh invalidates the emitted tangents
        didBffUnwrap = true; // re-mesh invalidates the emitted normals → recompute below
      } catch (unwrapErr) {
        const errorMessage = `Error unwrapping UVs: ${unwrapErr}`;
        return {
          objects: [],
          stats,
          error: errorMessage,
          gizmos: [],
        };
      }
    }

    stats.totalVtxCount += verts.length / 3;
    stats.totalFaceCount += indices.length / 3;

    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.BufferAttribute(verts, 3));
    geometry.setIndex(new THREE.BufferAttribute(indices, 1));

    if (uvs) {
      geometry.setAttribute('uv', new THREE.BufferAttribute(uvs, 2));
    }
    if (tangents) {
      // Named `tangent` so three auto-enables USE_TANGENT for normal-mapped materials → analytic
      // tangent-space normal maps along the sweep. Safe now that the color + depth-prepass shaders
      // pin `invariant gl_Position` (so depth still bit-matches) and the shader guards the
      // degenerate-tangent caps.
      geometry.setAttribute('tangent', new THREE.BufferAttribute(tangents, 4));
    }
    if (didBffUnwrap) {
      geometry.computeVertexNormals();
    } else if (normals) {
      geometry.setAttribute('normal', new THREE.BufferAttribute(normals, 3));
    }

    const matEntry = ((): MatEntry => {
      if (!materialName) {
        return {
          resolved: FallbackMat,
          promise: Promise.resolve(FallbackMat),
        };
      }

      if ('promise' in mat) {
        return mat;
      }

      return { resolved: mat, promise: Promise.resolve(mat) };
    })();

    const material = overrideMat ? overrideMat : (matEntry.resolved ?? HiddenMat);

    renderedObjects.push({
      type: 'mesh',
      geometry,
      material,
      materialName,
      materialPromise: matEntry.promise,
      transform: new THREE.Matrix4().fromArray(transform),
      castShadow: true,
      receiveShadow: true,
      sourceModule: sourceModule ?? '',
      meshId,
    });
  }

  stats.renderedPathCount = await repl.getRenderedPathCount(ctxPtr);
  for (let i = 0; i < stats.renderedPathCount; i += 1) {
    const { verts: pathVerts, pathId } = await repl.getRenderedPath(ctxPtr, i);
    stats.totalVtxCount += pathVerts.length / 3;
    stats.totalFaceCount += pathVerts.length / 3 - 1;

    const pathGeometry = new THREE.BufferGeometry();
    pathGeometry.setAttribute('position', new THREE.BufferAttribute(pathVerts, 3));

    renderedObjects.push({
      type: 'path',
      geometry: pathGeometry,
      material: LineMat,
      castShadow: false,
      receiveShadow: false,
      pathId,
    });
  }

  stats.renderedLightCount = await repl.getRenderedLightCount(ctxPtr);
  for (let i = 0; i < stats.renderedLightCount; i += 1) {
    const { light, lightId } = await repl.getRenderedLight(ctxPtr, i);
    const builtLight = buildLight(light, renderMode);
    renderedObjects.push({
      type: 'light',
      light: builtLight,
      lightId,
    });
  }

  // Gizmos are interactive overlay state, not scene meshes — kept off `objects`.
  const gizmos: RenderedGizmo[] = [];
  const gizmoCount = await repl.getRenderedGizmoCount(ctxPtr);
  for (let i = 0; i < gizmoCount; i += 1) {
    const g = await repl.getRenderedGizmo(ctxPtr, i);
    gizmos.push({
      sourceModule: g.source_module,
      handleId: g.handle_id,
      kind: g.kind,
      origin: g.origin,
      value: g.value,
      absolute: g.absolute,
    });
  }

  const result: GeoscriptRunResult = {
    objects: renderedObjects,
    stats,
    error: null,
    gizmos,
  };

  return result;
};

export interface PopulateSceneOpts {
  /** The tree used to look up ancestor transforms for each rendered mesh. */
  tree?: TreeDef;
  /** Pre-computed `moduleName → nodeId` map. Built by the caller from the tree. */
  moduleNameToNodeId?: Record<string, string>;
  /**
   * Previous-run objects keyed by `reuseKey`. Matches are mutated in place and
   * returned in `reusedKeys`; the caller disposes the rest.
   */
  prev?: Map<string, RenderedObject>;
}

export interface PopulateSceneResult {
  objects: RenderedObject[];
  reusedKeys: Set<string>;
}

const applyLightProps = (target: THREE.Light, source: THREE.Light): void => {
  target.color.copy(source.color);
  target.intensity = source.intensity;
  target.castShadow = source.castShadow;
  target.position.copy(source.position);
  target.quaternion.copy(source.quaternion);
  target.scale.copy(source.scale);
  if (target instanceof THREE.DirectionalLight && source instanceof THREE.DirectionalLight) {
    target.target.position.copy(source.target.position);
    target.userData.autoShadowFrustum = source.userData.autoShadowFrustum;
    // shadow.map is allocated lazily and doesn't auto-resize; force re-alloc
    // when mapSize changes so the new size actually takes effect.
    if (
      target.shadow.map &&
      (target.shadow.mapSize.width !== source.shadow.mapSize.width ||
        target.shadow.mapSize.height !== source.shadow.mapSize.height)
    ) {
      target.shadow.map.dispose();
      target.shadow.map = null as unknown as THREE.WebGLRenderTarget;
    }
    target.shadow.mapSize.copy(source.shadow.mapSize);
    target.shadow.radius = source.shadow.radius;
    target.shadow.blurSamples = source.shadow.blurSamples;
    target.shadow.bias = source.shadow.bias;
    target.shadow.normalBias = source.shadow.normalBias;
    target.shadow.camera.near = source.shadow.camera.near;
    target.shadow.camera.far = source.shadow.camera.far;
    target.shadow.camera.left = source.shadow.camera.left;
    target.shadow.camera.right = source.shadow.camera.right;
    target.shadow.camera.top = source.shadow.camera.top;
    target.shadow.camera.bottom = source.shadow.camera.bottom;
    target.shadow.camera.updateProjectionMatrix();
  }
};

const _identityMatrix = new THREE.Matrix4();
const _scratchFinal = new THREE.Matrix4();

export const populateScene = (
  scene: THREE.Scene,
  geoscriptOutput: GeoscriptRunResult,
  opts: PopulateSceneOpts = {}
): PopulateSceneResult => {
  const newRenderedObjects: RenderedObject[] = [];
  const reusedKeys = new Set<string>();
  const { tree, moduleNameToNodeId, prev } = opts;
  const worldMatrices = tree ? buildWorldMatrixCache(tree, buildParentMap(tree)) : null;

  for (const obj of geoscriptOutput.objects) {
    if (obj.type === 'mesh') {
      const sourceNodeId = tree && moduleNameToNodeId ? moduleNameToNodeId[obj.sourceModule] : undefined;
      if (tree && obj.sourceModule && obj.sourceModule !== ROOT_NODE_NAME && !sourceNodeId) {
        continue;
      }

      const insts =
        (worldMatrices && sourceNodeId ? worldMatrices.get(sourceNodeId) : null) ??
        ([{ world: _identityMatrix, path: [] }] as NodeWorldInstance[]);

      // The first new copy adopts `obj.geometry`; further copies clone so each live
      // mesh owns its geometry and disposes independently. If every copy reused a
      // prior mesh, the freshly-generated geometry is leftover and gets disposed.
      let baseGeomConsumed = false;
      const localInScript = obj.transform.clone();
      const objMeshes: THREE.Mesh[] = [];
      for (const inst of insts) {
        const reuseKey = `${obj.meshId}:${instancePathKey(inst.path)}`;
        _scratchFinal.copy(inst.world).multiply(obj.transform);

        const existing = prev?.get(reuseKey);
        if (existing instanceof THREE.Mesh && !reusedKeys.has(reuseKey)) {
          // Mutate in place to skip the GPU re-upload and scene-graph churn.
          _scratchFinal.decompose(existing.position, existing.quaternion, existing.scale);
          existing.userData.localInScript = localInScript;
          existing.userData.instancePath = inst.path;
          existing.material = obj.material;
          existing.userData.materialName = obj.materialName;
          existing.castShadow = obj.castShadow;
          existing.receiveShadow = obj.receiveShadow;
          if (sourceNodeId) {
            existing.userData.sourceNodeId = sourceNodeId;
          }
          existing.userData.reuseKey = reuseKey;
          reusedKeys.add(reuseKey);
          objMeshes.push(existing);
          newRenderedObjects.push(existing);
          continue;
        }

        const geometry = baseGeomConsumed ? obj.geometry.clone() : ((baseGeomConsumed = true), obj.geometry);
        const mesh = new THREE.Mesh(geometry, obj.material);
        mesh.userData.materialName = obj.materialName;
        mesh.userData.reuseKey = reuseKey;
        mesh.userData.localInScript = localInScript;
        mesh.userData.instancePath = inst.path;
        if (sourceNodeId) {
          mesh.userData.sourceNodeId = sourceNodeId;
        }

        _scratchFinal.decompose(mesh.position, mesh.quaternion, mesh.scale);
        mesh.castShadow = obj.castShadow;
        mesh.receiveShadow = obj.receiveShadow;
        scene.add(mesh);
        objMeshes.push(mesh);
        newRenderedObjects.push(mesh);
      }
      if (obj.materialPromise) {
        obj.materialPromise.then(mat => {
          for (const m of objMeshes) m.material = mat;
        });
      }
      if (!baseGeomConsumed) obj.geometry.dispose();
    } else if (obj.type === 'path') {
      const reuseKey = String(obj.pathId);
      const existing = prev?.get(reuseKey);
      if (existing instanceof THREE.Line && !reusedKeys.has(reuseKey)) {
        obj.geometry.dispose();
        existing.userData.reuseKey = reuseKey;
        reusedKeys.add(reuseKey);
        newRenderedObjects.push(existing);
        continue;
      }
      const line = new THREE.Line(obj.geometry, obj.material);
      line.castShadow = obj.castShadow;
      line.receiveShadow = obj.receiveShadow;
      line.userData.reuseKey = reuseKey;
      scene.add(line);
      newRenderedObjects.push(line);
    } else if (obj.type === 'light') {
      const reuseKey = String(obj.lightId);
      const existing = prev?.get(reuseKey);
      if (
        existing instanceof THREE.Light &&
        !reusedKeys.has(reuseKey) &&
        existing.constructor === obj.light.constructor
      ) {
        applyLightProps(existing, obj.light);
        existing.userData.reuseKey = reuseKey;
        reusedKeys.add(reuseKey);
        newRenderedObjects.push(existing);
        continue;
      }
      if (obj.light instanceof THREE.DirectionalLight || obj.light instanceof THREE.SpotLight) {
        obj.light.userData.geotoyTarget = obj.light.target;
        scene.add(obj.light.target);
      }
      obj.light.userData.reuseKey = reuseKey;
      scene.add(obj.light);
      newRenderedObjects.push(obj.light);
    } else {
      obj satisfies never;
      console.error('Unhandled rendered object type', obj);
    }
  }

  fitAutoShadowFrusta(scene, newRenderedObjects);

  return { objects: newRenderedObjects, reusedKeys };
};
