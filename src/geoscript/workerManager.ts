import * as Comlink from 'comlink';
import GeoscriptWorker from 'src/geoscript/geoscriptWorker.worker?worker';
import type { GeoscriptWorkerMethods } from './geoscriptWorker.worker';

export class WorkerManager {
  private rawWorker: Worker | null = null;
  private wrappedWorker: Comlink.Remote<GeoscriptWorkerMethods> | null = null;
  private terminated = false;
  private pendingRelease: ReturnType<typeof setTimeout> | null = null;

  constructor() {
    this.createWorker();
  }

  private createWorker(): void {
    this.rawWorker = new GeoscriptWorker();
    this.wrappedWorker = Comlink.wrap<GeoscriptWorkerMethods>(this.rawWorker);
    this.terminated = false;
  }

  public getWorker(): Comlink.Remote<GeoscriptWorkerMethods> {
    if (!this.wrappedWorker || this.terminated) {
      throw new Error('Worker is terminated. Call recreate() first.');
    }
    return this.wrappedWorker;
  }

  /** Claim by a new owner, cancelling a pending `release()`. Only an incoming owner may keep the
   *  worker alive, so a stray `getWorker()` after a real unmount can't resurrect it. */
  public acquire(): Comlink.Remote<GeoscriptWorkerMethods> {
    this.cancelRelease();
    return this.getWorker();
  }

  private cancelRelease(): void {
    if (this.pendingRelease !== null) {
      clearTimeout(this.pendingRelease);
      this.pendingRelease = null;
    }
  }

  /**
   * Teardown for an owner that may immediately be replaced by another. Svelte HMR destroys the
   * old component — running its teardown — before synchronously constructing the replacement,
   * which `acquire()`s this same manager and cancels the release. Deferring by a task turns what
   * would be a permanently dead worker into a no-op, and keeps the worker's warm wasm instance
   * across an edit. A real unmount has no successor, so the terminate lands.
   */
  public release(): void {
    this.pendingRelease ??= setTimeout(() => {
      this.pendingRelease = null;
      this.terminate();
    });
  }

  public terminate(): void {
    this.cancelRelease();
    if (this.rawWorker) {
      this.rawWorker.terminate();
      this.rawWorker = null;
      this.wrappedWorker = null;
      this.terminated = true;
    }
  }

  public async recreate(): Promise<Comlink.Remote<GeoscriptWorkerMethods>> {
    this.cancelRelease();
    if (this.rawWorker) {
      this.rawWorker.terminate();
    }
    this.createWorker();
    return this.wrappedWorker!;
  }

  public isTerminated(): boolean {
    return this.terminated;
  }
}
