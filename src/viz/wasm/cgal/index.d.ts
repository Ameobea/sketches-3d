export interface CGALModule {
  HEAPF32: Float32Array;
  HEAPU32: Uint32Array;
  [key: string]: any;
}

export interface CGALFactory {
  (moduleArg?: { locateFile?: (path: string) => string }): Promise<CGALModule>;
  locateFile?: (path: string) => string;
}

export const CGAL: CGALFactory;
