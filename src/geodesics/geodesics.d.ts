export interface GeodesicsModule {
  HEAPF32: Float32Array;
  HEAPU32: Uint32Array;
  [key: string]: any;
}

export interface GeodesicsFactory {
  (moduleArg?: { locateFile?: (path: string) => string }): Promise<GeodesicsModule>;
  locateFile?: (path: string) => string;
}

export const Geodesics: GeodesicsFactory;
