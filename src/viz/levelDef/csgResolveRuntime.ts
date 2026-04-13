import { WorkerManager } from 'src/geoscript/workerManager';

/**
 * Manages the worker pool and execution queues used by the CSG editor for
 * preview resolution and full asset re-resolution.
 *
 * Two separate workers are maintained so that cheap preview resolves and
 * expensive full-asset resolves do not block each other.
 */
export class CsgResolveRuntime {
  private previewWorkerManager: WorkerManager | null = null;
  private previewRepl: ReturnType<WorkerManager['getWorker']> | null = null;
  private previewCtxPtrPromise: Promise<number> | null = null;
  private previewResolveQueue: Promise<void> = Promise.resolve();

  private assetWorkerManager: WorkerManager | null = null;
  private assetRepl: ReturnType<WorkerManager['getWorker']> | null = null;
  private assetCtxPtrPromise: Promise<number> | null = null;

  /** Return (or lazily create) the preview worker runtime. */
  getPreviewRuntime() {
    if (!this.previewWorkerManager || !this.previewRepl || !this.previewCtxPtrPromise) {
      this.previewWorkerManager = new WorkerManager();
      this.previewRepl = this.previewWorkerManager.getWorker();
      this.previewCtxPtrPromise = this.previewRepl.init();
    }
    return { repl: this.previewRepl, ctxPtrPromise: this.previewCtxPtrPromise };
  }

  /** Return (or lazily create) the asset re-resolution worker runtime. */
  getAssetRuntime() {
    if (!this.assetWorkerManager || !this.assetRepl || !this.assetCtxPtrPromise) {
      this.assetWorkerManager = new WorkerManager();
      this.assetRepl = this.assetWorkerManager.getWorker();
      this.assetCtxPtrPromise = this.assetRepl.init();
    }
    return { repl: this.assetRepl, ctxPtrPromise: this.assetCtxPtrPromise };
  }

  terminatePreviewWorker() {
    if (this.previewWorkerManager) this.previewWorkerManager.terminate();
    this.previewWorkerManager = null;
    this.previewRepl = null;
    this.previewCtxPtrPromise = null;
    this.previewResolveQueue = Promise.resolve();
  }

  terminateAssetWorker() {
    if (this.assetWorkerManager) this.assetWorkerManager.terminate();
    this.assetWorkerManager = null;
    this.assetRepl = null;
    this.assetCtxPtrPromise = null;
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
