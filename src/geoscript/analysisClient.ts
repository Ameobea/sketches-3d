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

export class AnalysisClient {
  private worker: Worker;
  private proxy: Comlink.Remote<AnalysisWorkerMethods>;
  private initPromise: Promise<void>;

  constructor() {
    this.worker = new AnalysisWorkerConstructor();
    this.proxy = Comlink.wrap<AnalysisWorkerMethods>(this.worker);
    this.initPromise = this.proxy.init();
  }

  async analyze(src: string, includePrelude: boolean): Promise<AnalysisResult> {
    await this.initPromise;
    const json = await this.proxy.analyze(src, includePrelude);
    return JSON.parse(json);
  }

  async hover(src: string, line: number, col: number, includePrelude: boolean): Promise<HoverInfo | null> {
    await this.initPromise;
    const json = await this.proxy.hover(src, line, col, includePrelude);
    return json ? JSON.parse(json) : null;
  }

  async completions(
    src: string,
    line: number,
    col: number,
    includePrelude: boolean
  ): Promise<CompletionItem[]> {
    await this.initPromise;
    const json = await this.proxy.completions(src, line, col, includePrelude);
    return JSON.parse(json);
  }

  async gotoDefinition(
    src: string,
    line: number,
    col: number,
    includePrelude: boolean
  ): Promise<DefinitionLocation | null> {
    await this.initPromise;
    const json = await this.proxy.gotoDefinition(src, line, col, includePrelude);
    return json ? JSON.parse(json) : null;
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
