import * as Comlink from 'comlink';

import normalMapGenWasmURL from 'src/viz/wasmComp/normal_map_gen.wasm?url';
import terrainWasmURL from 'src/viz/wasmComp/terrain.wasm?url';
import textureCrossfadeWasmURL from 'src/viz/wasmComp/texture_crossfade.wasm?url';
import type { TerrainGenWorker } from './terrain/TerrainGenWorker/TerrainGenWorker.worker';
import { clamp, hasWasmSIMDSupport } from './util/util';

class WorkerPoolManager<T> {
  private allWorkers: Comlink.Remote<T>[];
  private idleWorkers: Comlink.Remote<T>[];

  /**
   * Populated when there are no idle workers.
   */
  private workQueue: [work: (worker: Comlink.Remote<T>) => any | Promise<any>, cb: (ret: any) => void][] = [];

  constructor(workers: Comlink.Remote<T>[]) {
    this.allWorkers = [...workers];
    this.idleWorkers = [...workers];
  }

  public submitWork = async <R>(work: (worker: Comlink.Remote<T>) => R | Promise<R>): Promise<R> => {
    if (this.idleWorkers.length === 0) {
      return new Promise<R>(resolve => {
        this.workQueue.push([work, resolve]);
      });
    }

    const worker = this.idleWorkers.pop()!;

    let ret: { type: 'ok'; val: R } | { type: 'err'; err: any };

    try {
      const out = await work(worker);
      ret = { type: 'ok', val: out };
    } catch (err) {
      console.error('Error in worker', err);
      ret = { type: 'err', err };
    } finally {
      this.idleWorkers.push(worker);
    }

    if (this.workQueue.length > 0) {
      const [nextWork, cb] = this.workQueue.shift()!;
      this.submitWork(nextWork).then(cb);
    }

    if (ret.type === 'err') {
      throw ret.err;
    }
    return ret.val;
  };

  public submitWorkToAll = async <R>(work: (worker: Comlink.Remote<T>) => R | Promise<R>): Promise<R[]> => {
    return Promise.all(this.allWorkers.map(work));
  };
}

let didInitNormalGenWasm = false;
let didInitTextureCrossfadeWasm = false;
let threadPoolWorkers: Promise<WorkerPoolManager<any>> | WorkerPoolManager<any> | null = null;
let didInitTerrainGenWasm = false;
let terrainGenWorker: Comlink.Remote<TerrainGenWorker> | null = null;

const fetchWasmBytes = async (url: string): Promise<Uint8Array> => {
  const wasmBytesAB = await fetch(url).then(r => r.arrayBuffer());
  return new Uint8Array(wasmBytesAB);
};

const buildThreadPoolWorkers = (onInit?: (wrapped: Comlink.Remote<any>) => void | Promise<void>) =>
  new Promise<WorkerPoolManager<any>>(async resolve => {
    const workerMod = await import('./threadpoolWorker.worker?worker');

    const numWorkers = clamp((navigator.hardwareConcurrency || 4) - 2, 1, 8);
    const workers = await Promise.all(
      Array.from({ length: numWorkers }, async () => {
        const worker = new workerMod.default();
        const wrapped = Comlink.wrap<any>(worker);
        await onInit?.(wrapped);
        return wrapped;
      })
    );
    resolve(new WorkerPoolManager(workers));
  });

const loadNormalGenWasm = async () => {
  const simdSupported = await hasWasmSIMDSupport();
  if (!simdSupported) {
    throw new Error('WASM SIMD not supported');
  }
  return fetchWasmBytes(normalMapGenWasmURL);
};

export const getNormalGenWorkers = async () => {
  if (!threadPoolWorkers) {
    threadPoolWorkers = buildThreadPoolWorkers();
  }
  if (threadPoolWorkers instanceof Promise) {
    threadPoolWorkers = await threadPoolWorkers;
  }

  if (!didInitNormalGenWasm) {
    const wasmBytes = await loadNormalGenWasm();
    didInitNormalGenWasm = true;

    await threadPoolWorkers.submitWorkToAll(worker => worker.setNormalGenWasmBytes(wasmBytes));
    return threadPoolWorkers;
  }

  return threadPoolWorkers;
};

export const getTextureCrossfadeWorkers = async () => {
  if (!threadPoolWorkers) {
    threadPoolWorkers = buildThreadPoolWorkers();
  }
  if (threadPoolWorkers instanceof Promise) {
    threadPoolWorkers = await threadPoolWorkers;
  }

  if (!didInitTextureCrossfadeWasm) {
    const wasmBytes = await fetchWasmBytes(textureCrossfadeWasmURL);
    didInitTextureCrossfadeWasm = true;

    await threadPoolWorkers.submitWorkToAll(worker => worker.setTextureCrossfadeWasmBytes(wasmBytes));
    return threadPoolWorkers;
  }

  return threadPoolWorkers;
};

export const getTerrainGenWorker = async () => {
  if (terrainGenWorker) {
    return terrainGenWorker;
  }

  if (!didInitTerrainGenWasm) {
    const wasmBytes = await fetchWasmBytes(terrainWasmURL);
    didInitTerrainGenWasm = true;

    const workerMod = await import('./terrain/TerrainGenWorker/TerrainGenWorker.worker?worker');
    const worker = new workerMod.default();
    const wrapped = Comlink.wrap<TerrainGenWorker>(worker);
    await wrapped.setTerrainGenWasmBytes(wasmBytes);
    terrainGenWorker = wrapped;
    return wrapped;
  }

  throw new Error('Terrain gen worker not initialized, but didInitTerrainGenWasm is true');
};
