import * as Comlink from 'comlink';
import GeoscriptWorker from 'src/geoscript/geoscriptWorker.worker?worker';
import type { GeoscriptWorkerMethods } from './geoscriptWorker.worker';

export class WorkerManager {
  private rawWorker: Worker | null = null;
  private wrappedWorker: Comlink.Remote<GeoscriptWorkerMethods> | null = null;
  private terminated = false;

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

  public terminate(): void {
    if (this.rawWorker) {
      this.rawWorker.terminate();
      this.rawWorker = null;
      this.wrappedWorker = null;
      this.terminated = true;
    }
  }

  public async recreate(): Promise<Comlink.Remote<GeoscriptWorkerMethods>> {
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
