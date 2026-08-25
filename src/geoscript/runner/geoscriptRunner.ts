import * as THREE from 'three';
import type * as Comlink from 'comlink';
import type {
  RunGeoscriptOptions,
  GeoscriptRunResult,
  RunStats,
  GeneratedObject,
  GeneratedTexture,
  MatEntry,
  RenderedGizmo,
  RenderedControl,
} from './types';
import { buildLight, fitAutoShadowFrusta } from 'src/geotoy/modes/mesh/lights';
import { parseChannelStats } from '../textureStats';
import { FallbackMat, HiddenMat, LineMat, NormalMat, WireframeMat } from '../materials';
import type { RenderedObject } from './types';
import type { GeoscriptAsyncDeps, GeoscriptWorkerMethods } from '../geoscriptWorker.worker';
import { bitmaskToAsyncDepNames } from '../asyncDepBits';
import type { TreeDef } from '../geotoyAPIClient';
import { ROOT_NODE_NAME } from '../geotoyAPIClient';
import { buildParentMap } from 'src/geotoy/modules/treeOps';
import { buildWorldMatrixCache, instancePathKey, type NodeWorldInstance } from './worldMatrixCache';
export { buildWorldMatrixCache, instancePathKey };
export type { NodeWorldInstance, WorldMatrixCache } from './worldMatrixCache';

const buildEmptyRunStats = (): RunStats => ({
  runtimeMs: 0,
  renderedMeshCount: 0,
  renderedPathCount: 0,
  renderedLightCount: 0,
  renderedTextureCount: 0,
  totalVtxCount: 0,
  totalFaceCount: 0,
  asyncDeps: [],
  constEvalCache: { entries: 0, bytes: 0, maxBytes: 0 },
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

export const runGeoscript = async (opts: RunGeoscriptOptions): Promise<GeoscriptRunResult> => {
  const {
    code,
    ctxPtr,
    repl,
    materials = {},
    preludeKind,
    materialOverride,
    renderMode = false,
    textureDetail = 'full',
    modules,
    modulePreludes,
    ambientSources,
    tabAmbients,
    gizmoValues,
    textureParams,
    rootModuleName,
    vectorize = { disabled: false, verify: false, profile: false },
  } = opts;
  await repl.reset(ctxPtr);
  await repl.setVectorizeFlags(ctxPtr, vectorize);

  // Sent even when empty: `set_module_sources` is the only thing that clears the ctx's
  // registered sources, so skipping it would leave a previous run's modules resolvable.
  if (modules) {
    await repl.setModuleSources(ctxPtr, modules, modulePreludes);
  }

  if (tabAmbients !== undefined) {
    try {
      await repl.setTabAmbientScopes(
        ctxPtr,
        tabAmbients.map(t => t.tabId),
        tabAmbients.map(t => t.preludeKind),
        tabAmbients.map(t => t.globalsSource)
      );
    } catch (err) {
      const errStr = err instanceof Error ? err.message : String(err);
      if (await tryInitAsyncDepFromErr(errStr, repl)) {
        return runGeoscript(opts);
      }
      return {
        objects: [],
        stats: buildEmptyRunStats(),
        error: `Error building ambient scope: ${err}`,
        gizmos: [],
        controls: [],
        vectorizeReports: [],
      };
    }
  } else if (ambientSources !== undefined) {
    try {
      await repl.setAmbientScope(ctxPtr, ambientSources, rootModuleName);
    } catch (err) {
      const errStr = err instanceof Error ? err.message : String(err);
      if (await tryInitAsyncDepFromErr(errStr, repl)) {
        return runGeoscript(opts);
      }
      return {
        objects: [],
        stats: buildEmptyRunStats(),
        error: `Error building ambient scope: ${err}`,
        gizmos: [],
        controls: [],
        vectorizeReports: [],
      };
    }
  }

  // Always sent (default `{}`/`[]`) so a previous run's handle values can't leak in.
  await repl.setGizmoValues(ctxPtr, gizmoValues ?? {});
  await repl.setTextureParams(ctxPtr, textureParams ?? []);

  let evalResult: { durationMs: number; usedDepsBitmask: number } = { durationMs: 0, usedDepsBitmask: 0 };
  try {
    evalResult = await repl.eval(ctxPtr, code, preludeKind, rootModuleName);
  } catch (evalErr) {
    const errorMessage = `Error evaluating code: ${evalErr}`;
    console.error(errorMessage, evalErr);
    return {
      objects: [],
      stats: buildEmptyRunStats(),
      error: errorMessage,
      gizmos: [],
      controls: [],
      vectorizeReports: [],
    };
  }

  const err = (await repl.getErr(ctxPtr)) || null;
  if (err) {
    // Safety net: if a dep wasn't pre-loaded, load it now and re-run.
    // text_to_path always goes through this path since its args are runtime values.
    if (await tryInitAsyncDepFromErr(err, repl)) {
      return runGeoscript(opts);
    }
    return {
      objects: [],
      stats: buildEmptyRunStats(),
      error: err,
      gizmos: [],
      controls: [],
      vectorizeReports: [],
    };
  }

  const stats: RunStats = {
    ...buildEmptyRunStats(),
    runtimeMs: evalResult.durationMs,
    asyncDeps: bitmaskToAsyncDepNames(evalResult.usedDepsBitmask),
    constEvalCache: await repl.getConstEvalCacheStats(ctxPtr),
  };
  const vectorizeReports = await repl.getVectorizeReports(ctxPtr);
  const renderedObjects: GeneratedObject[] = [];

  const overrideMat = getOverrideMat(materialOverride);

  stats.renderedMeshCount = await repl.getRenderedMeshCount(ctxPtr);
  for (let i = 0; i < stats.renderedMeshCount; i += 1) {
    const {
      transform,
      verts,
      indices,
      normals,
      uvs,
      tangents,
      material: materialName,
      sourceModule,
      meshId,
    } = await repl.getRenderedMesh(ctxPtr, i);

    const matLookup = materials[materialName] ?? {
      def: null,
      mat: { resolved: FallbackMat, promise: Promise.resolve(FallbackMat) },
    };
    const { mat } = matLookup;

    stats.totalVtxCount += verts.length / 3;
    stats.totalFaceCount += indices.length / 3;

    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.BufferAttribute(verts, 3));
    geometry.setIndex(new THREE.BufferAttribute(indices, 1));

    if (uvs) {
      geometry.setAttribute('uv', new THREE.BufferAttribute(uvs, 2));
    }
    if (normals) {
      geometry.setAttribute('normal', new THREE.BufferAttribute(normals, 3));
    }
    if (tangents) {
      // Named `tangent` so three auto-enables USE_TANGENT for normal-mapped materials → analytic
      // tangent-space normal maps. Safe now that the color + depth-prepass shaders pin
      // `invariant gl_Position` (so depth still bit-matches) and the shader guards degenerate caps.
      geometry.setAttribute('tangent', new THREE.BufferAttribute(tangents, 4));
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
      // Plain-material entries (geotoy's MaterialRuntime) are assigned reactively by
      // the shell; only MatEntry callers rely on populateScene's async swap.
      materialPromise: 'promise' in mat ? matEntry.promise : null,
      transform: new THREE.Matrix4().fromArray(transform),
      castShadow: true,
      receiveShadow: true,
      sourceModule: sourceModule ?? '',
      meshId,
    });
  }

  stats.renderedPathCount = await repl.getRenderedPathCount(ctxPtr);
  for (let i = 0; i < stats.renderedPathCount; i += 1) {
    const { verts: pathVerts, pathId, sourceModule } = await repl.getRenderedPath(ctxPtr, i);
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
      sourceModule,
    });
  }

  stats.renderedLightCount = await repl.getRenderedLightCount(ctxPtr);
  for (let i = 0; i < stats.renderedLightCount; i += 1) {
    const { light, lightId, sourceModule } = await repl.getRenderedLight(ctxPtr, i);
    renderedObjects.push({
      type: 'light',
      light: buildLight(light, renderMode),
      lightId,
      sourceModule,
    });
  }

  stats.renderedTextureCount = await repl.getRenderedTextureCount(ctxPtr);
  for (let i = 0; i < stats.renderedTextureCount; i += 1) {
    const t = await repl.getRenderedTexture(ctxPtr, i, textureDetail);
    renderedObjects.push({
      type: 'texture',
      name: t.name,
      usage: (t.usage || null) as GeneratedTexture['usage'],
      wrap: t.wrap as GeneratedTexture['wrap'],
      width: t.width,
      height: t.height,
      channels: t.channels,
      layers: t.layers,
      data: t.pixels,
      encoded: t.encoded,
      encodedFormat: t.encodedFormat,
      rgba: t.rgba,
      sourceModule: t.sourceModule,
      textureId: t.textureId,
      minFilter: t.minFilter || null,
      magFilter: t.magFilter || null,
      format: t.format || null,
      stats: parseChannelStats(t.stats),
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
      axes: g.axes,
      ghost: g.ghost,
    });
  }

  const controls: RenderedControl[] = [];
  const controlCount = await repl.getRenderedControlCount(ctxPtr);
  for (let i = 0; i < controlCount; i += 1) {
    const c = await repl.getRenderedControl(ctxPtr, i);
    controls.push({
      sourceModule: c.source_module,
      handleId: c.handle_id,
      kind: c.kind,
      label: c.label,
      value: c.value,
      str_value: c.str_value,
      min: c.min,
      max: c.max,
      step: c.step,
      style: c.style,
      options: c.options,
      stats: c.stats ? parseChannelStats(c.stats) : null,
      hasOverride: c.has_override,
    });
  }

  const result: GeoscriptRunResult = {
    objects: renderedObjects,
    stats,
    error: null,
    gizmos,
    controls,
    vectorizeReports,
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

/** Release a run's geometry when nothing will adopt it — `populateScene` never ran, or ran
 *  for a target that has since gone away. */
export const disposeRunObjects = (result: GeoscriptRunResult) => {
  for (const obj of result.objects) {
    if ('geometry' in obj && obj.geometry) obj.geometry.dispose();
  }
};

export const populateScene = (
  scene: THREE.Object3D,
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
      const sourceNodeId =
        tree && moduleNameToNodeId && obj.sourceModule ? moduleNameToNodeId[obj.sourceModule] : undefined;
      const existing = prev?.get(reuseKey);
      if (existing instanceof THREE.Line && !reusedKeys.has(reuseKey)) {
        obj.geometry.dispose();
        existing.userData.reuseKey = reuseKey;
        existing.userData.sourceNodeId = sourceNodeId;
        reusedKeys.add(reuseKey);
        newRenderedObjects.push(existing);
        continue;
      }
      const line = new THREE.Line(obj.geometry, obj.material);
      line.castShadow = obj.castShadow;
      line.receiveShadow = obj.receiveShadow;
      line.userData.reuseKey = reuseKey;
      line.userData.sourceNodeId = sourceNodeId;
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
    } else if (obj.type === 'texture') {
      // Not a scene object; consumed by texture mode and material texture uploads.
    } else {
      obj satisfies never;
      console.error('Unhandled rendered object type', obj);
    }
  }

  fitAutoShadowFrusta(scene, newRenderedObjects);

  // debugging handle for inspecting live scene meshes (attributes, UVs) from devtools/automation
  (globalThis as any).__geotoyRenderedObjects = newRenderedObjects;

  return { objects: newRenderedObjects, reusedKeys };
};
