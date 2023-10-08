import * as Comlink from 'comlink';

import type { RuneGenCtx } from './runeGenWorker.worker';

let RuneGenWorker: Promise<Comlink.Remote<RuneGenCtx>> | null = null;

export const getRuneGenWorker = async () => {
  if (!RuneGenWorker) {
    RuneGenWorker = new Promise(async resolve => {
      const workerMod = await import('./runeGenWorker.worker?worker');
      const worker = new workerMod.default();
      resolve(Comlink.wrap<RuneGenCtx>(worker));
    });
  }

  return RuneGenWorker;
};
