import * as Comlink from 'comlink';
import * as THREE from 'three';

import type { RuneGenCtx } from './runeGenWorker.worker';

let RuneGenWorker: Promise<Comlink.Remote<RuneGenCtx>> | null = null;

class RuneGenerator {
  public worker: Comlink.Remote<RuneGenCtx>;

  constructor(worker: Comlink.Remote<RuneGenCtx>) {
    this.worker = worker;
  }

  public generateMesh = async (targetMesh: THREE.Mesh, material: THREE.Material) => {
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
    // geometry.computeVertexNormals();
    geometry.setAttribute('normal', new THREE.BufferAttribute(vertexNormals, 3));

    return new THREE.Mesh(geometry, material);
  };
}

export const getRuneGenerator = async () => {
  if (!RuneGenWorker) {
    RuneGenWorker = new Promise(async resolve => {
      const workerMod = await import('./runeGenWorker.worker?worker');
      const worker = new workerMod.default();
      resolve(Comlink.wrap<RuneGenCtx>(worker));
    });
  }

  return new RuneGenerator(await RuneGenWorker);
};
