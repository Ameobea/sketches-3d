import type * as Comlink from 'comlink';

import type { GeoscriptWorkerMethods } from 'src/geoscript/geoscriptWorker.worker';
import type { WorkerManager } from 'src/geoscript/workerManager';
import { getGeoscriptWorkerWasmURLs } from 'src/viz/wasmComp/wasmAssetURLs';
import {
  runGeoscript,
  type RunGeoscriptOptions,
  type RunResult,
  type RunStats,
} from 'src/geoscript/runner/runner';

export interface RunInput {
  code: string;
  modules: Record<string, string>;
  /** Ambient scope sources beyond the prelude (globals source etc.). */
  extraAmbientSources: string[];
  includePrelude: boolean;
  materials: NonNullable<RunGeoscriptOptions['materials']>;
  materialOverride: RunGeoscriptOptions['materialOverride'];
  renderMode: boolean;
  gizmoValues: RunGeoscriptOptions['gizmoValues'];
  moduleNameToNodeId: Record<string, string>;
}

interface ExecutionOpts<T extends RunInput> {
  workerManager: WorkerManager;
  buildRunInput: () => T;
  /** Called at the top of each run (draft save). */
  onRunStart: () => void;
  setLastRunWasSuccessful: (success: boolean) => void;
  /**
   * Scene-side bookkeeping for a successful run. Runs inside the try, before `isRunning`
   * clears; `isCurrent` guards async continuations against a cancel mid-flight.
   */
  onRunSuccess: (result: RunResult, input: T, isCurrent: () => boolean) => void;
  /** Tear down rendered objects when a run is cancelled. */
  onCancelCleanup: () => void;
  /** Fired when the control-edit debounce settles (shell routes to its fast path). */
  onDebouncedRun: () => void;
  /**
   * Fired whenever a fresh wasm ctx exists (initial init + post-cancel recreate). The
   * shell must re-push ctx-scoped state here (materials): a recreated wasm instance
   * usually allocates the ctx at the same address, so `ctxPtr` alone can't signal it.
   */
  onCtxReady: () => void;
}

const extractFailedModuleName = (msg: string): string | null => {
  const m = msg.match(/module\s+["']([^"']+)["']/i);
  return m ? m[1] : null;
};

/**
 * Owns the geoscript worker + run lifecycle: init, run, cancel (worker terminate +
 * recreate), and the control-edit run debounce. Run requests during an in-flight run
 * coalesce into one trailing re-run (latest-wins; input is rebuilt when it fires).
 * Ignorant of tree shape; compilation input comes from `buildRunInput` and results
 * flow back through `onRunSuccess`.
 */
export class GeoscriptExecution<T extends RunInput = RunInput> {
  private readonly opts: ExecutionOpts<T>;
  // Bumped on cancel(). A run captures its gen up front and bails on any post-
  // await continuation whose gen no longer matches — distinguishes "worker
  // terminated mid-call" from a real eval failure.
  private runGen = 0;
  private pendingRun = false;
  private controlRunTimer = 0;
  private controlRunPending = false;

  repl = $state.raw() as Comlink.Remote<GeoscriptWorkerMethods>;
  ctxPtr: number | null = $state(null);
  isRunning = $state(false);
  err: string | null = $state(null);
  runStats: RunStats | null = $state(null);
  failedNodeIds: Set<string> = $state(new Set());

  constructor(opts: ExecutionOpts<T>) {
    this.opts = opts;
    this.repl = opts.workerManager.getWorker();
  }

  get lastOutcome(): { type: 'ok'; stats: RunStats } | { type: 'err'; err: string | null } | null {
    if (this.err) {
      return { type: 'err', err: this.err };
    }
    if (this.runStats) {
      return { type: 'ok', stats: this.runStats };
    }
    return null;
  }

  init = async () => {
    this.ctxPtr = await this.repl.init(getGeoscriptWorkerWasmURLs());
    this.opts.onCtxReady();
  };

  private maybeRunPending() {
    if (!this.pendingRun) {
      return;
    }
    this.pendingRun = false;
    this.run();
  }

  run = async () => {
    if (this.ctxPtr === null) {
      return;
    }
    if (this.isRunning) {
      this.pendingRun = true;
      return;
    }

    this.opts.onRunStart();

    const myGen = this.runGen;
    this.isRunning = true;
    this.err = null;
    this.failedNodeIds = new Set();
    this.runStats = null;

    const input = this.opts.buildRunInput();

    try {
      const ambientSources: string[] = [];
      if (input.includePrelude) {
        ambientSources.push(await this.repl.getPrelude());
      }
      ambientSources.push(...input.extraAmbientSources);

      this.opts.setLastRunWasSuccessful(false);
      const result = await runGeoscript({
        code: input.code,
        modules: input.modules,
        ambientSources,
        ctxPtr: this.ctxPtr,
        repl: this.repl,
        materials: input.materials,
        includePrelude: input.includePrelude,
        materialOverride: input.materialOverride,
        renderMode: input.renderMode,
        gizmoValues: input.gizmoValues,
      });

      if (myGen !== this.runGen) return;

      if (result.error) {
        // Keep the previous scene visible on failure.
        this.err = result.error;
        const failedModule = extractFailedModuleName(result.error);
        if (failedModule && input.moduleNameToNodeId[failedModule]) {
          this.failedNodeIds = new Set([input.moduleNameToNodeId[failedModule]]);
        }
        this.isRunning = false;
        this.maybeRunPending();
        return;
      }

      this.opts.setLastRunWasSuccessful(true);
      this.runStats = result.stats;
      this.opts.onRunSuccess(result, input, () => myGen === this.runGen);
      this.isRunning = false;
      this.maybeRunPending();
    } catch (e) {
      if (myGen !== this.runGen) {
        return;
      }
      console.error('geoscript run failed', e);
      this.err = `Run failed: ${e instanceof Error ? e.message : String(e)}`;
      this.isRunning = false;
      this.maybeRunPending();
    }
  };

  cancel = async () => {
    if (!this.isRunning) {
      return;
    }

    this.runGen++;
    this.opts.workerManager.terminate();

    this.opts.onCancelCleanup();
    this.runStats = null;

    this.repl = await this.opts.workerManager.recreate();
    this.ctxPtr = await this.repl.init(getGeoscriptWorkerWasmURLs());
    this.opts.onCtxReady();

    this.err = 'Execution interrupted';
    this.isRunning = false;
    // Discard anything queued before/during the cancel — the user said stop.
    this.discardPending();
  };

  private discardPending() {
    this.pendingRun = false;
    this.controlRunPending = false;
    clearTimeout(this.controlRunTimer);
  }

  dispose = () => {
    this.discardPending();
  };

  // Continuous inputs (sliders) fire rapidly; coalesce into a trailing re-run once edits settle.
  scheduleControlRun = () => {
    this.controlRunPending = true;
    clearTimeout(this.controlRunTimer);
    this.controlRunTimer = window.setTimeout(this.fireControlRun, 120);
  };

  private fireControlRun = () => {
    if (!this.controlRunPending) return;
    this.controlRunPending = false;
    // If a run is in flight this lands in the latest-wins queue via `run()`.
    this.opts.onDebouncedRun();
  };
}
