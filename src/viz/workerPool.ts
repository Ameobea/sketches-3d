import * as Comlink from 'comlink';

import { clamp } from './util';

class WorkerPoolManager<T> {
  private allWorkers: Comlink.Remote<T>[] = [];
  private idleWorkers: Comlink.Remote<T>[];

  /**
   * Populated when there are no idle workers.
   */
  private workQueue: [work: (worker: Comlink.Remote<T>) => any | Promise<any>, cb: (ret: any) => void][] = [];

  constructor(workers: Comlink.Remote<T>[]) {
    this.allWorkers = workers;
    this.idleWorkers = workers;
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
let threadPoolWorkers: Promise<WorkerPoolManager<any>> | null = null;

const buildThreadPoolWorkers = (onInit?: (wrapped: Comlink.Remote<any>) => void) =>
  new Promise<WorkerPoolManager<any>>(async resolve => {
    const workerMod = await import('./threadpoolWorker.worker?worker');

    const numWorkers = clamp((navigator.hardwareConcurrency || 4) - 2, 1, 8);
    const workers = Array.from({ length: numWorkers }, () => {
      const worker = new workerMod.default();
      const wrapped = Comlink.wrap<any>(worker);
      onInit?.(wrapped);
      return wrapped;
    });
    resolve(new WorkerPoolManager(workers));
  });

export const getNormalGenWorkers = async () => {
  if (threadPoolWorkers) {
    if (!didInitNormalGenWasm) {
      const wasmBytesAB = await fetch('/normal_map_gen.wasm').then(r => r.arrayBuffer());
      const wasmBytes = new Uint8Array(wasmBytesAB);
      threadPoolWorkers.then(pool => pool.submitWorkToAll(worker => worker.setNormalGenWasmBytes(wasmBytes)));
    }
    didInitNormalGenWasm = true;

    return threadPoolWorkers;
  }

  let onInit: ((wrapped: Comlink.Remote<any>) => void) | undefined;
  if (!didInitNormalGenWasm) {
    const wasmBytesAB = await fetch('/normal_map_gen.wasm').then(r => r.arrayBuffer());
    const wasmBytes = new Uint8Array(wasmBytesAB);
    onInit = wrapped => wrapped.setNormalGenWasmBytes(wasmBytes);
    didInitNormalGenWasm = true;
  }

  threadPoolWorkers = buildThreadPoolWorkers(onInit);
  return threadPoolWorkers;
};

export const getTextureCrossfadeWorkers = async () => {
  if (threadPoolWorkers) {
    if (!didInitTextureCrossfadeWasm) {
      const wasmBytesAB = await fetch('/texture_crossfade.wasm').then(r => r.arrayBuffer());
      const wasmBytes = new Uint8Array(wasmBytesAB);
      threadPoolWorkers.then(pool =>
        pool.submitWorkToAll(worker => worker.setTextureCrossfadeWasmBytes(wasmBytes))
      );
    }
    didInitTextureCrossfadeWasm = true;

    return threadPoolWorkers;
  }

  let onInit: ((wrapped: Comlink.Remote<any>) => void) | undefined;
  if (!didInitTextureCrossfadeWasm) {
    const wasmBytesAB = await fetch('/texture_crossfade.wasm').then(r => r.arrayBuffer());
    const wasmBytes = new Uint8Array(wasmBytesAB);
    onInit = wrapped => wrapped.setTextureCrossfadeWasmBytes(wasmBytes);
    didInitTextureCrossfadeWasm = true;
  }

  threadPoolWorkers = buildThreadPoolWorkers(onInit);
  return threadPoolWorkers;
};
