import * as THREE from 'three';
import type * as Comlink from 'comlink';

import type { GeoscriptWorkerMethods } from 'src/geoscript/geoscriptWorker.worker';
import type { RenderedObject } from 'src/geoscript/runner/types';
import type { TreeDef } from 'src/geoscript/geotoyAPIClient';
import { exportObjectsToData, type MeshExportFormat } from './export';
import type { RunStats } from './types';

type RMesh = THREE.Mesh<THREE.BufferGeometry, THREE.Material>;
type RLine = THREE.Line<THREE.BufferGeometry, THREE.Material>;

export type MeshOutputFormat = 'summary' | MeshExportFormat | 'json';

export interface EvalRequest {
  /** Optional geoscript expression, evaluated against the composition's root scope. */
  expr?: string;
  /** t∈[0,1] sample count for callable values (0 = don't sample). */
  samples?: number;
  /** How much mesh geometry to include. `summary` = counts + bbox only. */
  meshes?: MeshOutputFormat;
}

const uint8ToBase64 = (bytes: Uint8Array): string => {
  let bin = '';
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    bin += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(bin);
};

const worldBbox = (obj: THREE.Object3D) => {
  const b = new THREE.Box3().setFromObject(obj);
  return b.isEmpty() ? null : { min: b.min.toArray(), max: b.max.toArray() };
};

const moduleOf = (obj: THREE.Object3D, tree: TreeDef): string | null => {
  const nid = obj.userData.sourceNodeId as string | undefined;
  return nid ? (tree.nodes[nid]?.name ?? null) : null;
};

const attrArray = (g: THREE.BufferGeometry, name: string): number[] | null => {
  const a = g.getAttribute(name);
  return a ? Array.from(a.array as Float32Array) : null;
};

export const buildEvalResultJson = async (params: {
  repl: Comlink.Remote<GeoscriptWorkerMethods>;
  ctxPtr: number;
  renderedObjects: RenderedObject[];
  tree: TreeDef;
  stats: RunStats | null;
  req: EvalRequest;
}): Promise<string> => {
  const { repl, ctxPtr, renderedObjects, tree, stats, req } = params;
  const samples = req.samples ?? 0;

  const meshObjs = renderedObjects.filter((o): o is RMesh => o instanceof THREE.Mesh);
  const lineObjs = renderedObjects.filter((o): o is RLine => o instanceof THREE.Line);
  const lightObjs = renderedObjects.filter((o): o is THREE.Light => o instanceof THREE.Light);

  const exports = JSON.parse(await repl.getExportsJson(ctxPtr, samples));
  const prints = await repl.takePrints(ctxPtr);
  // `--expr` is appended to the root source (see the render route), so its value is the run's
  // last top-level statement — fully resolved/optimized because it ran as part of the program.
  const expr = req.expr ? JSON.parse(await repl.getLastValueJson(ctxPtr, samples)) : undefined;

  const meshes = meshObjs.map(m => {
    const pos = m.geometry.getAttribute('position');
    const idx = m.geometry.getIndex();
    return {
      id: (m.userData.reuseKey as string) ?? null,
      sourceModule: moduleOf(m, tree),
      material: (m.userData.materialName as string) ?? null,
      vertices: pos ? pos.count : 0,
      faces: idx ? idx.count / 3 : pos ? pos.count / 3 : 0,
      bbox: worldBbox(m),
    };
  });

  const paths = lineObjs.map(l => {
    l.updateMatrixWorld(true);
    const pos = l.geometry.getAttribute('position');
    const pts: number[][] = [];
    if (pos) {
      const v = new THREE.Vector3();
      for (let i = 0; i < pos.count; i++) {
        v.fromBufferAttribute(pos, i).applyMatrix4(l.matrixWorld);
        pts.push([v.x, v.y, v.z]);
      }
    }
    return { id: (l.userData.reuseKey as string) ?? null, sourceModule: moduleOf(l, tree), points: pts };
  });

  const lights = lightObjs.map(li => ({
    type: li.type,
    color: li.color.toArray(),
    intensity: li.intensity,
    position: li.position.toArray(),
  }));

  const result: Record<string, unknown> = {
    ok: true,
    error: null,
    stats: stats && {
      meshes: stats.renderedMeshCount,
      paths: stats.renderedPathCount,
      lights: stats.renderedLightCount,
      vertices: stats.totalVtxCount,
      faces: stats.totalFaceCount,
      runtimeMs: stats.runtimeMs,
    },
    exports,
    prints,
    meshes,
    paths,
    lights,
  };
  if (expr !== undefined) result.expr = expr;

  const fmt = req.meshes ?? 'summary';
  if (fmt === 'json') {
    result.meshData = {
      format: 'json',
      meshes: meshObjs.map(m => {
        m.updateMatrixWorld(true);
        const idx = m.geometry.getIndex();
        return {
          id: (m.userData.reuseKey as string) ?? null,
          sourceModule: moduleOf(m, tree),
          positions: attrArray(m.geometry, 'position'),
          normals: attrArray(m.geometry, 'normal'),
          uvs: attrArray(m.geometry, 'uv'),
          indices: idx ? Array.from(idx.array as Uint32Array) : null,
          matrixWorld: m.matrixWorld.elements.slice(),
        };
      }),
    };
  } else if (fmt === 'glb' || fmt === 'gltf' || fmt === 'obj') {
    const hasExportable = meshObjs.length + lightObjs.length > 0;
    const data = hasExportable ? await exportObjectsToData(renderedObjects, fmt) : { text: '' };
    result.meshData =
      'binary' in data
        ? { format: fmt, encoding: 'base64', data: uint8ToBase64(data.binary) }
        : { format: fmt, encoding: 'utf8', data: data.text };
  }

  return JSON.stringify(result);
};
