import { WorkerManager } from 'src/geoscript/workerManager';
import { getGeoscriptWorkerWasmURLs } from 'src/viz/wasmComp/wasmAssetURLs';

/** A lazily-created geoscript worker + its initialized context pointer. */
export class WorkerSlot {
  private manager: WorkerManager | null = null;
  private repl: ReturnType<WorkerManager['getWorker']> | null = null;
  private ctxPtrPromise: Promise<number> | null = null;

  get() {
    if (!this.manager || !this.repl || !this.ctxPtrPromise) {
      this.manager = new WorkerManager();
      this.repl = this.manager.getWorker();
      this.ctxPtrPromise = this.repl.init(getGeoscriptWorkerWasmURLs());
    }
    return { repl: this.repl, ctxPtrPromise: this.ctxPtrPromise };
  }

  terminate() {
    this.manager?.terminate();
    this.manager = null;
    this.repl = null;
    this.ctxPtrPromise = null;
  }
}

/**
 * Manages the workers and execution queues used by the CSG editor for preview resolution
 * and full asset re-resolution. Two separate workers keep cheap preview resolves and
 * expensive full-asset resolves from blocking each other.
 */
export class CsgResolveRuntime {
  private readonly previewSlot = new WorkerSlot();
  private readonly assetSlot = new WorkerSlot();
  private previewResolveQueue: Promise<void> = Promise.resolve();

  getPreviewRuntime() {
    return this.previewSlot.get();
  }

  getAssetRuntime() {
    return this.assetSlot.get();
  }

  terminatePreviewWorker() {
    this.previewSlot.terminate();
    this.previewResolveQueue = Promise.resolve();
  }

  terminateAssetWorker() {
    this.assetSlot.terminate();
  }

  /**
   * Enqueue a task on the serialised preview resolve queue.
   * Tasks run one at a time in submission order; a task's rejection is swallowed
   * so that later tasks always run even when an earlier one fails.
   */
  queuePreviewResolve<T>(task: () => Promise<T>): Promise<T> {
    const next = this.previewResolveQueue.then(task, task);
    this.previewResolveQueue = next.then(
      () => undefined,
      () => undefined
    );
    return next;
  }
}
