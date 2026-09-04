import type { Text } from '@codemirror/state';

import type { AnalysisClient } from './analysisClient';

let clientPromise: Promise<AnalysisClient> | null = null;

export const getClient = (): Promise<AnalysisClient> => {
  if (!clientPromise) {
    clientPromise = import('./analysisClient').then(mod => mod.getAnalysisClient());
  }
  return clientPromise;
};

// Analysis columns count chars (as Pest does); CM positions are UTF-16 offsets.

/** Convert 1-based line/col from analysis to a CM6 character offset. */
export const lcToPos = (doc: Text, line: number, col: number): number => {
  if (line < 1 || line > doc.lines) {
    return 0;
  }
  const lineObj = doc.line(line);
  return (
    lineObj.from +
    Array.from(lineObj.text)
      .slice(0, col - 1)
      .join('').length
  );
};

/** Convert a CM6 character offset to 1-based line/col for the analysis API. */
export const posToLc = (doc: Text, pos: number): [line: number, col: number] => {
  const lineObj = doc.lineAt(pos);
  return [lineObj.number, Array.from(lineObj.text.slice(0, pos - lineObj.from)).length + 1];
};

export type GetIncludePrelude = () => boolean;
/** Extra always-in-scope source (the Geotoy `_globals` node); `''` when none / editing it. */
export type GetAmbientSource = () => string;
