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
  /**
   * Content hash of everything the eval depends on except per-node transforms, computed
   * by the shell at build time. The module records it on ok-settle as `lastOkInputKey`
   * so the transform-only fast path can prove the scene still reflects current inputs.
   */
  inputKey: string;
}

export type RunOutcome<T extends RunInput = RunInput> =
  | { type: 'ok'; result: RunResult; input: T; isCurrent: () => boolean }
  | { type: 'err'; err: string; failedNodeIds: Set<string> }
  | { type: 'cancelled' };

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (v: T) => void;
}
const deferred = <T>(): Deferred<T> => {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>(r => (resolve = r));
  return { promise, resolve };
};

interface ExecutionOpts<T extends RunInput> {
  workerManager: WorkerManager;
  buildRunInput: () => T;
  /** Called at the top of each run (draft save). */
  onRunStart: () => void;
  setLastRunWasSuccessful: (success: boolean) => void;
  /**
   * Applies a successful run to the scene. Module-invoked (trailing/debounced runs have
   * no awaiting caller) outside the eval error domain, so a failure here surfaces as
   * "Failed to apply run result" instead of masquerading as an eval error; `isCurrent`
   * guards async continuations against a cancel mid-flight.
   */
  consume: (result: RunResult, input: T, isCurrent: () => boolean) => void;
  /** Tear down rendered objects when a run is cancelled. */
  onCancelCleanup: () => void;
}

const extractFailedModuleName = (msg: string): string | null => {
  const m = msg.match(/module\s+["']([^"']+)["']/i);
  return m ? m[1] : null;
};

/**
 * Owns the geoscript worker + run lifecycle: init, run, cancel (worker terminate +
 * recreate), and the control-edit run debounce. Run requests during an in-flight run
 * coalesce into one trailing re-run (latest-wins; input is rebuilt when it fires).
 * Ignorant of tree shape; compilation input comes from `buildRunInput`, and settled
 * runs flow back as `RunOutcome` values (`run()`'s resolution) with successful results
 * applied via the module-invoked `consume` hook.
 */
export class GeoscriptExecution<T extends RunInput = RunInput> {
  private readonly opts: ExecutionOpts<T>;
  // Bumped on cancel(). A run captures its gen up front and bails on any post-
  // await continuation whose gen no longer matches — distinguishes "worker
  // terminated mid-call" from a real eval failure.
  private runGen = 0;
  private pendingRun = false;
  private inFlightSettled: Deferred<RunOutcome<T>> | null = null;
  private queuedSettled: Deferred<RunOutcome<T>> | null = null;

  /** `inputKey` of the last ok-settled run — i.e. what the current scene was built from.
   *  Null after cancel (scene torn down) and before the first successful run. */
  lastOkInputKey: string | null = null;

  // Boxed: Svelte's dev-mode tracing tags values written to $state via a symbol-keyed
  // property probe, which a Comlink proxy turns into a doomed RPC (uncaught rejection
  // on every boot). Tagging the box instead is a no-op.
  private replBox = $state.raw() as { repl: Comlink.Remote<GeoscriptWorkerMethods> };
  get repl() {
    return this.replBox.repl;
  }
  ctxPtr: number | null = $state(null);
  // Bumped alongside every ctxPtr assignment (init() is the sole assigner). Ctx-scoped
  // state (materials) keys re-pushes off this: a recreated wasm instance usually
  // allocates the ctx at the same address, so ctxPtr equality can't signal recreation.
  ctxEpoch = $state(0);
  isRunning = $state(false);
  err: string | null = $state(null);
  runStats: RunStats | null = $state(null);
  failedNodeIds: Set<string> = $state(new Set());

  constructor(opts: ExecutionOpts<T>) {
    this.opts = opts;
    this.replBox = { repl: opts.workerManager.getWorker() };
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
    this.ctxEpoch++;
  };

  private maybeRunPending() {
    if (!this.pendingRun) {
      return;
    }
    this.pendingRun = false;
    this.run();
  }

  /**
   * Resolves with the run's settled outcome — for a request coalesced into the
   * latest-wins queue, the trailing run's outcome. Returns null when no ctx exists yet.
   * The outcome settles only after `consume` has applied it.
   */
  run = async (): Promise<RunOutcome<T> | null> => {
    if (this.ctxPtr === null) {
      return null;
    }
    if (this.isRunning) {
      this.pendingRun = true;
      this.queuedSettled ??= deferred();
      return this.queuedSettled.promise;
    }

    this.opts.onRunStart();

    const myGen = this.runGen;
    this.isRunning = true;
    this.err = null;
    this.failedNodeIds = new Set();
    this.runStats = null;

    // A queued requester's deferred is adopted by the trailing run it coalesced into.
    const settled = this.queuedSettled ?? deferred();
    this.queuedSettled = null;
    this.inFlightSettled = settled;

    const input = this.opts.buildRunInput();
    // Race against `settled`: cancel() terminates the worker, which strands the eval's
    // Comlink promise forever — awaiting evalRun alone would hang this caller.
    const outcome = await Promise.race([this.evalRun(input, myGen), settled.promise]);
    if (outcome === null || outcome.type === 'cancelled') {
      // Cancelled mid-flight (or a gen-stale rejection racing the cancel): cancel()
      // resolves `settled` and owns all state cleanup.
      return settled.promise;
    }

    if (outcome.type === 'ok') {
      this.runStats = outcome.result.stats;
      try {
        this.opts.consume(outcome.result, outcome.input, outcome.isCurrent);
        // Recorded only after consume succeeds: a half-populated scene must not
        // hash-match the fast path, else it never re-evals its way back to health.
        this.lastOkInputKey = outcome.input.inputKey;
      } catch (e) {
        console.error('applying run result failed', e);
        this.err = `Failed to apply run result: ${e instanceof Error ? e.message : String(e)}`;
        this.lastOkInputKey = null;
      }
    } else if (outcome.type === 'err') {
      // Keep the previous scene visible on failure.
      this.err = outcome.err;
      this.failedNodeIds = outcome.failedNodeIds;
    }
    this.isRunning = false;
    this.inFlightSettled = null;
    settled.resolve(outcome);
    this.maybeRunPending();
    return outcome;
  };

  /** Eval error domain only — shell-side consumption failures must not land here. */
  private evalRun = async (input: T, myGen: number): Promise<RunOutcome<T> | null> => {
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
        ctxPtr: this.ctxPtr!,
        repl: this.repl,
        materials: input.materials,
        includePrelude: input.includePrelude,
        materialOverride: input.materialOverride,
        renderMode: input.renderMode,
        gizmoValues: input.gizmoValues,
      });

      if (myGen !== this.runGen) return null;

      if (result.error) {
        const failedModule = extractFailedModuleName(result.error);
        return {
          type: 'err',
          err: result.error,
          failedNodeIds:
            failedModule && input.moduleNameToNodeId[failedModule]
              ? new Set([input.moduleNameToNodeId[failedModule]])
              : new Set(),
        };
      }

      this.opts.setLastRunWasSuccessful(true);
      return { type: 'ok', result, input, isCurrent: () => myGen === this.runGen };
    } catch (e) {
      if (myGen !== this.runGen) return null;
      console.error('geoscript run failed', e);
      return {
        type: 'err',
        err: `Run failed: ${e instanceof Error ? e.message : String(e)}`,
        failedNodeIds: new Set(),
      };
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

    this.replBox = { repl: await this.opts.workerManager.recreate() };
    await this.init();

    this.lastOkInputKey = null;
    this.err = 'Execution interrupted';
    this.isRunning = false;
    // Discard anything queued before/during the cancel — the user said stop.
    this.discardPending();
    // terminate() strands the in-flight Comlink promise forever; settle awaiters here.
    const settled = this.inFlightSettled;
    this.inFlightSettled = null;
    settled?.resolve({ type: 'cancelled' });
  };

  private discardPending() {
    this.pendingRun = false;
    const queued = this.queuedSettled;
    this.queuedSettled = null;
    queued?.resolve({ type: 'cancelled' });
  }

  dispose = () => {
    this.discardPending();
    const settled = this.inFlightSettled;
    this.inFlightSettled = null;
    settled?.resolve({ type: 'cancelled' });
  };
}
