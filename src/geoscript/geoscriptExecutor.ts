import * as Comlink from 'comlink';

import { buildRunResult } from './runner/geoscriptRunner';
import type {
  GeneratedObject,
  GizmoValuesByModule,
  RenderedControl,
  RenderedGizmo,
  TextureDetail,
  TextureParamsEntry,
  WorkerRunPayload,
} from './runner/types';
import type { TreeKind } from './geotoyAPIClient';
import type { GeoscriptAsyncDeps } from './geoscriptWorker.worker';
import { WorkerManager } from './workerManager';
import { getGeoscriptWorkerWasmURLs } from 'src/viz/wasmComp/wasmAssetURLs';

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
  /** Ambient scope sources (e.g. `[prelude, globalsSource]`); for composition-tree jobs. */
  ambientSources?: string[];
  /** Baked gizmo handle values, keyed `moduleName → handleId`; for composition-tree jobs. */
  gizmoValues?: GizmoValuesByModule;
  /**
   * geotoy material names to register on the shared ctx before running, so the tree's
   * `set_material('<name>')` calls resolve; for composition-tree jobs. Survives the per-run
   * reset, so it's set fresh per job rather than relying on prior ctx state.
   */
  availableMaterials?: string[];
  /** Default material name applied to meshes that don't call `set_material`. */
  defaultMaterialName?: string | null;
  /**
   * Per-tab ambient scopes for multi-tab composition runs (dep tabs first, entry tab last);
   * takes precedence over `ambientSources`/`includePrelude`-derived ambients.
   */
  tabAmbients?: { tabId: string; preludeKind: TreeKind | ''; globalsSource: string }[];
  /** Entry-program prelude kind; defaults to `includePrelude ? 'mesh' : undefined`. */
  preludeKind?: TreeKind;
  /** Qualified entry module name (`<tabId>:_root`) for multi-tab runs. */
  rootModuleName?: string;
  /** UI-owned per-output GPU materialization params, from composition tab metadata. */
  textureParams?: TextureParamsEntry[];
  /** Defaults to `'full'`; level loading passes `'gpu'` to skip host-only texture products. */
  textureDetail?: TextureDetail;
}

export interface GeoscriptJobResult {
  objects: GeneratedObject[];
  error: string | null;
  /** Input controls declared by the program; used by level-def to validate injected inputs. */
  controls: RenderedControl[];
  /** `gizmo(...)` sites reported by the run; used by the level editor's handle overlay. */
  gizmos: RenderedGizmo[];
  meta?: { runtimeMs: number; asyncDeps: string[] };
}

export class GeoscriptExecutor {
  private workerManager: WorkerManager;
  private ctxPtrPromise: Promise<number>;

  constructor(eagerDeps?: {
    cgal?: boolean;
    clipper2?: boolean;
    geodesics?: boolean;
    uv_unwrap?: boolean;
    uv_solvers?: boolean;
  }) {
    this.workerManager = new WorkerManager();
    const repl = this.workerManager.getWorker();
    this.ctxPtrPromise = repl.init(getGeoscriptWorkerWasmURLs(), eagerDeps);
  }

  /** Every job runs inside the worker back to back; results stream in per job, so the main
   *  thread being busy (placement, physics, uploads) never stalls generation. Concurrent
   *  `submit`s are safe: each job is atomic on the worker from reset through extraction. */
  submit(jobs: GeoscriptJob[]): Map<string, Promise<GeoscriptJobResult>> {
    const results = new Map<string, Promise<GeoscriptJobResult>>();
    const pending = new Map<string, (r: GeoscriptJobResult) => void>();
    for (const job of jobs) {
      results.set(job.id, new Promise(res => pending.set(job.id, res)));
    }
    const failed = (error: string): GeoscriptJobResult => ({ objects: [], error, controls: [], gizmos: [] });

    const byId = new Map(jobs.map(j => [j.id, j]));
    const workerJobs = jobs.map(job => ({
      id: job.id,
      code: job.code,
      modules: job.modules,
      preludeKind: job.preludeKind ?? (job.includePrelude ? 'mesh' : undefined),
      ambientSources: job.tabAmbients ? undefined : job.ambientSources,
      tabAmbients: job.tabAmbients,
      gizmoValues: job.gizmoValues ?? {},
      textureParams: job.textureParams ?? [],
      textureDetail: job.textureDetail ?? 'full',
      rootModuleName: job.rootModuleName,
      vectorize: { disabled: false, verify: false, profile: false },
      materials: job.availableMaterials
        ? { defaultName: job.defaultMaterialName ?? null, available: job.availableMaterials }
        : undefined,
      clearConstEvalCache: job.collectMetadata,
      asyncDeps: job.asyncDeps.filter(d => d !== 'text_to_path') as (keyof GeoscriptAsyncDeps)[],
    }));
    const onResult = (id: string, payload: WorkerRunPayload) => {
      const res = pending.get(id);
      if (!res) {
        return;
      }
      pending.delete(id);
      if (payload.error) {
        res(failed(payload.error));
        return;
      }
      try {
        const { objects, gizmos, controls, stats } = buildRunResult(payload, {});
        res({
          objects,
          error: null,
          controls,
          gizmos,
          meta: byId.get(id)!.collectMetadata
            ? { runtimeMs: stats.runtimeMs, asyncDeps: stats.asyncDeps }
            : undefined,
        });
      } catch (err) {
        res(failed(String(err)));
      }
    };

    this.ctxPtrPromise
      .then(ctxPtr => this.workerManager.getWorker().runJobs(ctxPtr, workerJobs, Comlink.proxy(onResult)))
      .catch(err => {
        for (const res of pending.values()) {
          res(failed(String(err)));
        }
        pending.clear();
      });
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

  /** The standard geoscript prelude source — for building a composition run's ambient scope. */
  async getPrelude(kind: string): Promise<string> {
    await this.ctxPtrPromise;
    return this.workerManager.getWorker().getPrelude(kind);
  }

  terminate(): void {
    this.workerManager.terminate();
  }
}
