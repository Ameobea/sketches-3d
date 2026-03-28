export interface UVUnwrapModule {
  HEAPF32: Float32Array;
  HEAPU32: Uint32Array;
  [key: string]: any;
}

export interface UVUnwrapFactory {
  (moduleArg?: { locateFile?: (path: string) => string }): Promise<UVUnwrapModule>;
  locateFile?: (path: string) => string;
}

export const UVUnwrap: UVUnwrapFactory;
