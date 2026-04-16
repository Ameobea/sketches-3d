import * as Comlink from 'comlink';
import * as Analysis from 'src/viz/wasmComp/geoscript_analysis_wasm';

let ctxPtr: number | null = null;

const ensureInit = async () => {
  if (ctxPtr !== null) {
    return;
  }
  await Analysis.default();
  ctxPtr = Analysis.analysis_init();
};

const methods = {
  init: async () => {
    await ensureInit();
  },
  analyze: async (src: string, includePrelude: boolean): Promise<string> => {
    await ensureInit();
    return Analysis.analysis_analyze(ctxPtr!, src, includePrelude);
  },
  hover: async (src: string, line: number, col: number, includePrelude: boolean): Promise<string> => {
    await ensureInit();
    return Analysis.analysis_hover(ctxPtr!, src, line, col, includePrelude);
  },
  completions: async (src: string, line: number, col: number, includePrelude: boolean): Promise<string> => {
    await ensureInit();
    return Analysis.analysis_completions(ctxPtr!, src, line, col, includePrelude);
  },
  gotoDefinition: async (
    src: string,
    line: number,
    col: number,
    includePrelude: boolean
  ): Promise<string> => {
    await ensureInit();
    return Analysis.analysis_goto_definition(ctxPtr!, src, line, col, includePrelude);
  },
};

export type AnalysisWorkerMethods = typeof methods;

Comlink.expose(methods);
