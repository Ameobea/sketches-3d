import * as Comlink from 'comlink';
import AnalysisWorkerConstructor from 'src/geoscript/analysisWorker.worker?worker';
import type { AnalysisWorkerMethods } from './analysisWorker.worker';

export interface AnalysisDiagnostic {
  start_line: number;
  start_col: number;
  end_line: number;
  end_col: number;
  severity: 'Error' | 'Warning' | 'Info';
  message: string;
}

export interface AnalysisResult {
  diagnostics: AnalysisDiagnostic[];
}

export interface HoverInfo {
  content: string;
  start_line: number;
  start_col: number;
  end_line: number;
  end_col: number;
}

export interface CompletionItem {
  label: string;
  kind: string;
  detail: string;
  info: string;
}

export interface DefinitionLocation {
  start_line: number;
  start_col: number;
  end_line: number;
  end_col: number;
}

/** One `input_*` site to rewrite; payload in the injection-wire shape (`controlValueToWire`). */
export interface InputDefaultRequest {
  handle_id: string;
  kind: string;
  value: number[];
  str_value: string | null;
}

/** UTF-16 offsets into the source the request was planned against. */
export interface SourceEdit {
  from: number;
  to: number;
  insert: string;
}

export interface RewriteInputDefaultsResult {
  edits: SourceEdit[];
  errors: { handle_id: string; message: string }[];
}

/** Splices non-overlapping edits (all relative to `src`) into a new string. */
export const applySourceEdits = (src: string, edits: SourceEdit[]): string => {
  let out = '';
  let cursor = 0;
  for (const e of [...edits].sort((a, b) => a.from - b.from)) {
    out += src.slice(cursor, e.from) + e.insert;
    cursor = e.to;
  }
  return out + src.slice(cursor);
};

export class AnalysisClient {
  private worker: Worker;
  private proxy: Comlink.Remote<AnalysisWorkerMethods>;
  private initPromise: Promise<void>;

  constructor() {
    this.worker = new AnalysisWorkerConstructor();
    this.proxy = Comlink.wrap<AnalysisWorkerMethods>(this.worker);
    this.initPromise = this.proxy.init();
  }

  async analyze(src: string, includePrelude: boolean, ambientSrc: string): Promise<AnalysisResult> {
    await this.initPromise;
    const json = await this.proxy.analyze(src, includePrelude, ambientSrc);
    return JSON.parse(json);
  }

  async hover(
    src: string,
    line: number,
    col: number,
    includePrelude: boolean,
    ambientSrc: string
  ): Promise<HoverInfo | null> {
    await this.initPromise;
    const json = await this.proxy.hover(src, line, col, includePrelude, ambientSrc);
    return json ? JSON.parse(json) : null;
  }

  async completions(
    src: string,
    line: number,
    col: number,
    includePrelude: boolean,
    ambientSrc: string
  ): Promise<CompletionItem[]> {
    await this.initPromise;
    const json = await this.proxy.completions(src, line, col, includePrelude, ambientSrc);
    return JSON.parse(json);
  }

  async gotoDefinition(
    src: string,
    line: number,
    col: number,
    includePrelude: boolean,
    ambientSrc: string
  ): Promise<DefinitionLocation | null> {
    await this.initPromise;
    const json = await this.proxy.gotoDefinition(src, line, col, includePrelude, ambientSrc);
    return json ? JSON.parse(json) : null;
  }

  async rewriteInputDefaults(
    src: string,
    requests: InputDefaultRequest[]
  ): Promise<RewriteInputDefaultsResult> {
    await this.initPromise;
    return JSON.parse(await this.proxy.rewriteInputDefaults(src, JSON.stringify(requests)));
  }

  terminate(): void {
    this.worker.terminate();
  }
}

let sharedClient: AnalysisClient | null = null;

/**
 * Get or create a shared analysis client.  The analysis worker is loaded lazily
 * on first call.
 */
export const getAnalysisClient = (): AnalysisClient => {
  if (!sharedClient) {
    sharedClient = new AnalysisClient();
  }
  return sharedClient;
};
