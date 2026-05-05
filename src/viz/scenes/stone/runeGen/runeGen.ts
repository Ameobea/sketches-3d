import * as Comlink from 'comlink';
import * as THREE from 'three';

import type { RuneGenCtx } from './RuneGenCtx';
import { WASM_ASSET_URLS } from 'src/viz/wasmComp/wasmAssetURLs';

let RuneGenWorker: Promise<Comlink.Remote<RuneGenCtx>> | null = null;

class RuneGenerator {
  public worker: Comlink.Remote<RuneGenCtx>;

  constructor(worker: Comlink.Remote<RuneGenCtx>) {
    this.worker = worker;
  }

  public generateMesh = async (
    targetMesh: THREE.Mesh,
    material: THREE.Material | Promise<THREE.Material>
  ) => {
    await this.worker.awaitInit();

    if (!targetMesh.geometry.index) {
      throw new Error('Mesh must have vertex indices');
    }
    const targetMeshIndices = targetMesh.geometry.index.array as Uint16Array;
    const targetMeshVertices = targetMesh.geometry.attributes.position.array as Float32Array;

    const { indices, vertices, vertexNormals } = await this.worker.generate({
      indices: targetMeshIndices,
      vertices: targetMeshVertices,
    });
    if (indices.length % 3 !== 0) {
      throw new Error('indices.length % 3 !== 0');
    }

    const geometry = new THREE.BufferGeometry();
    geometry.setIndex(new THREE.BufferAttribute(indices, 1));
    geometry.setAttribute('position', new THREE.BufferAttribute(vertices, 3));
    geometry.setAttribute('normal', new THREE.BufferAttribute(vertexNormals, 3));

    const realMat =
      material instanceof Promise ? new THREE.MeshBasicMaterial({ transparent: true, opacity: 0 }) : material;
    const mesh = new THREE.Mesh(geometry, realMat);
    if (material instanceof Promise) {
      material.then(mat => {
        mesh.material = mat;
      });
    }
    return mesh;
  };
}

export const getRuneGenerator = async () => {
  if (!RuneGenWorker) {
    RuneGenWorker = (async () => {
      const workerMod = await import('./runeGenWorker.worker.js?worker');
      const worker = new workerMod.default();
      const ctx = Comlink.wrap<RuneGenCtx>(worker);
      await ctx.init(WASM_ASSET_URLS.geodesics);
      return ctx;
    })();
  }

  return new RuneGenerator(await RuneGenWorker);
};
