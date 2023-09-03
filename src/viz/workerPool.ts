import * as Comlink from 'comlink';

import { clamp, hasWasmSIMDSupport } from './util';

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
let threadPoolWorkers: Promise<WorkerPoolManager<any>> | WorkerPoolManager<any> | null = null;

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
  if (threadPoolWorkers) {
    if (threadPoolWorkers instanceof Promise) {
      threadPoolWorkers = await threadPoolWorkers;
    }

    if (!didInitNormalGenWasm) {
      const wasmBytes = await loadNormalGenWasm();
      didInitNormalGenWasm = true;
      console.log('Initializing normal map gen workers');

      await threadPoolWorkers.submitWorkToAll(worker => worker.setNormalGenWasmBytes(wasmBytes));
      return threadPoolWorkers;
    }

    return threadPoolWorkers;
  }

  if (!didInitNormalGenWasm) {
    threadPoolWorkers = loadNormalGenWasm().then(wasmBytes => {
      const onInit = (wrapped: Comlink.Remote<any>) => wrapped.setNormalGenWasmBytes(wasmBytes);
      return buildThreadPoolWorkers(onInit);
    });
    didInitNormalGenWasm = true;
    return threadPoolWorkers;
  }

  throw new Error('Normal map gen workers not initialized, but didInitNormalGenWasm is true');
};

export const getTextureCrossfadeWorkers = async () => {
  if (threadPoolWorkers) {
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
