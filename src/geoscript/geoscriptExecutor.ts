import * as Comlink from 'comlink';

import { runGeoscript } from './runner/geoscriptRunner';
import type { GeneratedObject } from './runner/types';
import type { GeoscriptAsyncDeps } from './geoscriptWorker.worker';
import { WorkerManager } from './workerManager';

export interface GeoscriptJob {
  id: string;
  modules: Record<string, string>;
  code: string;
  includePrelude: boolean;
  /** Async dep names from _meta; empty if unknown / first run. */
  asyncDeps: string[];
  /** Other job ids this job depends on (topo edges). */
  deps: string[];
  /** If true, clear the const-eval cache before running so timing is standalone. */
  collectMetadata: boolean;
}

export interface GeoscriptJobResult {
  objects: GeneratedObject[];
  error: string | null;
  meta?: { runtimeMs: number; asyncDeps: string[] };
}

export class GeoscriptExecutor {
  private workerManager: WorkerManager;
  private ctxPtrPromise: Promise<number>;

  constructor() {
    this.workerManager = new WorkerManager();
    const repl = this.workerManager.getWorker();
    this.ctxPtrPromise = repl.init();
  }

  submit(jobs: GeoscriptJob[]): Map<string, Promise<GeoscriptJobResult>> {
    const repl = this.workerManager.getWorker();
    const results = new Map<string, Promise<GeoscriptJobResult>>();

    // Collect all unique non-text_to_path async deps across all jobs.
    const allDeps = new Set<string>();
    for (const job of jobs) {
      for (const dep of job.asyncDeps) {
        if (dep !== 'text_to_path') {
          allDeps.add(dep);
        }
      }
    }

    // Kick off all dep loads concurrently.
    const depPromises = new Map<string, Promise<void>>();
    for (const dep of allDeps) {
      depPromises.set(
        dep,
        this.ctxPtrPromise.then(() => repl.initAsyncDep(dep as keyof GeoscriptAsyncDeps))
      );
    }

    // Run jobs sequentially (single worker requires shared-ctx ordering).
    // Each job's promise resolves as soon as that job completes.
    let chain = this.ctxPtrPromise.then(() => {});
    for (const job of jobs) {
      const jobDeps = job.asyncDeps.filter(d => d !== 'text_to_path');

      const jobPromise = new Promise<GeoscriptJobResult>(res => {
        chain = chain
          .then(async () => {
            // Wait for this job's specific deps.
            if (jobDeps.length > 0) {
              await Promise.all(jobDeps.map(d => depPromises.get(d)!));
            }

            const ctxPtr = await this.ctxPtrPromise;

            if (job.collectMetadata) {
              await repl.clearConstEvalCache(ctxPtr);
            }

            // runGeoscript handles reset + setModuleSources internally.
            const runResult = await runGeoscript({
              code: job.code,
              ctxPtr,
              repl,
              includePrelude: job.includePrelude,
              modules: job.modules,
            });

            const result: GeoscriptJobResult = {
              objects: runResult.objects,
              error: runResult.error,
            };
            if (job.collectMetadata && !runResult.error) {
              result.meta = {
                runtimeMs: runResult.stats.runtimeMs,
                asyncDeps: runResult.stats.asyncDeps,
              };
            }

            res(result);
          })
          .catch(err => {
            res({ objects: [], error: String(err) });
          });
      });

      results.set(job.id, jobPromise);
    }

    return results;
  }

  /**
   * Compute the convex hull of `verts` (flat xyz Float32Array, asset-local space) using
   * Manifold inside the worker.  Independent of the geoscript ctx — does not share any
   * state with submitted jobs and does not need to wait for jobs in the queue.
   */
  async computeConvexHull(verts: Float32Array): Promise<{ verts: Float32Array; indices: Uint32Array }> {
    await this.ctxPtrPromise;
    const repl = this.workerManager.getWorker();
    return repl.computeConvexHull(Comlink.transfer(verts, [verts.buffer]));
  }

  terminate(): void {
    this.workerManager.terminate();
  }
}
