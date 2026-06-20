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
  analyze: async (src: string, includePrelude: boolean, ambientSrc: string): Promise<string> => {
    await ensureInit();
    return Analysis.analysis_analyze(ctxPtr!, src, includePrelude, ambientSrc);
  },
  hover: async (
    src: string,
    line: number,
    col: number,
    includePrelude: boolean,
    ambientSrc: string
  ): Promise<string> => {
    await ensureInit();
    return Analysis.analysis_hover(ctxPtr!, src, line, col, includePrelude, ambientSrc);
  },
  completions: async (
    src: string,
    line: number,
    col: number,
    includePrelude: boolean,
    ambientSrc: string
  ): Promise<string> => {
    await ensureInit();
    return Analysis.analysis_completions(ctxPtr!, src, line, col, includePrelude, ambientSrc);
  },
  gotoDefinition: async (
    src: string,
    line: number,
    col: number,
    includePrelude: boolean,
    ambientSrc: string
  ): Promise<string> => {
    await ensureInit();
    return Analysis.analysis_goto_definition(ctxPtr!, src, line, col, includePrelude, ambientSrc);
  },
};

export type AnalysisWorkerMethods = typeof methods;

Comlink.expose(methods);
