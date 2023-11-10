import * as Comlink from 'comlink';

import type { TerrainGenWorker } from './terrain/TerrainGenWorker/TerrainGenWorker.worker';
import { clamp, hasWasmSIMDSupport } from './util';

class WorkerPoolManager<T> {
  private allWorkers: Comlink.Remote<T>[];
  private idleWorkers: Comlink.Remote<T>[];

  /**
   * Populated when there are no idle workers.
   */
  private workQueue: [work: (worker: Comlink.Remote<T>) => any | Promise<any>, cb: (ret: any) => void][] = [];

  constructor(workers: Comlink.Remote<T>[]) {
    console.log(workers);
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
    console.log(this.allWorkers);
    return Promise.all(this.allWorkers.map(work));
  };
}

let didInitNormalGenWasm = false;
let didInitTextureCrossfadeWasm = false;
let threadPoolWorkers: Promise<WorkerPoolManager<any>> | WorkerPoolManager<any> | null = null;
let didInitTerrainGenWasm = false;
let terrainGenWorker: Comlink.Remote<TerrainGenWorker> | null = null;

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
  const url = '/normal_map_gen.wasm';
  const wasmBytesAB = await fetch(url).then(r => r.arrayBuffer());
  return new Uint8Array(wasmBytesAB);
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

    console.log('setting normal gen wasm');
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
    const wasmBytesAB = await fetch('/texture_crossfade.wasm').then(r => r.arrayBuffer());
    const wasmBytes = new Uint8Array(wasmBytesAB);
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
    const wasmBytesAB = await fetch('/terrain.wasm').then(r => r.arrayBuffer());
    const wasmBytes = new Uint8Array(wasmBytesAB);
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
