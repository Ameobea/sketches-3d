import * as THREE from 'three';
import type { RunGeoscriptOptions, GeoscriptRunResult, RunStats, GeneratedObject, MatEntry } from './types';
import { buildLight } from 'src/viz/scenes/geoscriptPlayground/lights';
import { getUVUnwrapWorker } from '../uvUnwrapWorker';
import { FallbackMat, HiddenMat, LineMat, NormalMat, WireframeMat } from '../materials';
import type { RenderedObject } from './types';
import type { GeoscriptAsyncDeps } from '../geoscriptWorker.worker';

const buildEmptyRunStats = (startTime: number): RunStats => ({
  runtimeMs: performance.now() - startTime,
  renderedMeshCount: 0,
  renderedPathCount: 0,
  renderedLightCount: 0,
  totalVtxCount: 0,
  totalFaceCount: 0,
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

export const runGeoscript = async ({
  code,
  ctxPtr,
  repl,
  materials,
  includePrelude,
  materialOverride,
  renderMode = false,
}: RunGeoscriptOptions): Promise<GeoscriptRunResult> => {
  await repl.reset(ctxPtr);

  const startTime = performance.now();
  try {
    await repl.eval(ctxPtr, code, includePrelude);
  } catch (evalErr) {
    const errorMessage = `Error evaluating code: ${evalErr}`;
    console.error(errorMessage, evalErr);
    return {
      objects: [],
      stats: buildEmptyRunStats(startTime),
      error: errorMessage,
    };
  }

  const err = (await repl.getErr(ctxPtr)) || null;
  if (err) {
    // Check if it's a special error indicating that an async dep needs to be loaded
    if (err.includes('__GEOTOY_UNINITIALIZED_MODULE__:')) {
      const depName = /__GEOTOY_UNINITIALIZED_MODULE__:(\w+)/.exec(err)?.[1];
      if (!depName) {
        console.error('Unrecognized error format:', err);
        return {
          objects: [],
          stats: buildEmptyRunStats(startTime),
          error: err,
        };
      }

      const argsByKey: Partial<Record<keyof GeoscriptAsyncDeps, string[]>> = {};

      const deps: GeoscriptAsyncDeps = {};
      deps[depName as keyof GeoscriptAsyncDeps] = true;
      const hasArgs = err.includes('||__||');
      if (hasArgs) {
        const args = err.split('||__||').slice(1);
        argsByKey[depName as keyof GeoscriptAsyncDeps] = args;
      }

      await repl.initAsyncDeps(deps, argsByKey);
      return runGeoscript({
        code,
        ctxPtr,
        repl,
        materials,
        includePrelude,
        materialOverride,
        renderMode,
      });
    }

    return {
      objects: [],
      stats: buildEmptyRunStats(startTime),
      error: err,
    };
  }

  const stats: RunStats = buildEmptyRunStats(startTime);
  const renderedObjects: GeneratedObject[] = [];

  const overrideMat = getOverrideMat(materialOverride);

  stats.renderedMeshCount = await repl.getRenderedMeshCount(ctxPtr);
  for (let i = 0; i < stats.renderedMeshCount; i += 1) {
    const {
      transform,
      verts: initialVerts,
      indices: initialIndices,
      normals,
      material: materialName,
    } = await repl.getRenderedMesh(ctxPtr, i);

    let verts = initialVerts;
    let indices = initialIndices;
    let uvs: Float32Array | null = null;
    const { def: matDef, mat: mat } = materials[materialName];

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
      } catch (unwrapErr) {
        const errorMessage = `Error unwrapping UVs: ${unwrapErr}`;
        return {
          objects: [],
          stats,
          error: errorMessage,
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
    });
  }

  stats.renderedPathCount = await repl.getRenderedPathCount(ctxPtr);
  for (let i = 0; i < stats.renderedPathCount; i += 1) {
    const pathVerts: Float32Array = await repl.getRenderedPathVerts(ctxPtr, i);
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
    });
  }

  stats.renderedLightCount = await repl.getRenderedLightCount(ctxPtr);
  for (let i = 0; i < stats.renderedLightCount; i += 1) {
    const light = await repl.getRenderedLight(ctxPtr, i);
    const builtLight = buildLight(light, renderMode);
    renderedObjects.push({
      type: 'light',
      light: builtLight,
    });
  }

  const result: GeoscriptRunResult = {
    objects: renderedObjects,
    stats,
    error: null,
  };

  return result;
};

export const populateScene = (scene: THREE.Scene, geoscriptOutput: GeoscriptRunResult) => {
  const newRenderedObjects: RenderedObject[] = [];

  for (const obj of geoscriptOutput.objects) {
    if (obj.type === 'mesh') {
      const mesh = new THREE.Mesh(obj.geometry, obj.material);
      mesh.userData.materialName = obj.materialName;

      if (obj.materialPromise) {
        obj.materialPromise.then(mat => {
          mesh.material = mat;
        });
      }

      mesh.applyMatrix4(obj.transform);
      mesh.castShadow = obj.castShadow;
      mesh.receiveShadow = obj.receiveShadow;
      scene.add(mesh);
      newRenderedObjects.push(mesh);
    } else if (obj.type === 'path') {
      const line = new THREE.Line(obj.geometry, obj.material);
      line.castShadow = obj.castShadow;
      line.receiveShadow = obj.receiveShadow;
      scene.add(line);
      newRenderedObjects.push(line);
    } else if (obj.type === 'light') {
      scene.add(obj.light);
      newRenderedObjects.push(obj.light);
    } else {
      obj satisfies never;
      console.error('Unhandled rendered object type', obj);
    }
  }

  return newRenderedObjects;
};
