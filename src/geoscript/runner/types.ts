import type * as THREE from 'three';
import type * as Comlink from 'comlink';
import type { GeoscriptWorkerMethods } from '../geoscriptWorker.worker';
import type { MaterialDef } from '../materials';

export interface RunStats {
  runtimeMs: number;
  renderedMeshCount: number;
  renderedPathCount: number;
  renderedLightCount: number;
  totalVtxCount: number;
  totalFaceCount: number;
  /** Async dep names actually used during the eval (from the Rust bitmask). */
  asyncDeps: string[];
}

export interface GeneratedMesh {
  type: 'mesh';
  geometry: THREE.BufferGeometry;
  material: THREE.Material;
  materialName: string;
  materialPromise: Promise<THREE.Material> | null;
  transform: THREE.Matrix4;
  castShadow: boolean;
  receiveShadow: boolean;
  /**
   * Name of the geoscript module that called `render()` to register this mesh.
   * The JS-side scene populator looks up the corresponding tree node and composes
   * its ancestor chain of transforms before adding the mesh to the Three.js scene.
   * Empty string for the legacy/flat-source path.
   */
  sourceModule: string;
}

export interface GeneratedPath {
  type: 'path';
  geometry: THREE.BufferGeometry;
  material: THREE.Material;
  castShadow: boolean;
  receiveShadow: boolean;
}

export interface GeneratedLight {
  type: 'light';
  light: THREE.Light;
}

export type GeneratedObject = GeneratedMesh | GeneratedPath | GeneratedLight;

export type RenderedObject =
  | THREE.Mesh<THREE.BufferGeometry, THREE.Material>
  | THREE.Line<THREE.BufferGeometry, THREE.Material>
  | THREE.Light;

export interface MatEntry {
  promise: Promise<THREE.Material>;
  resolved: THREE.Material | null;
  beforeRenderCb?: (curTimeSeconds: number) => void;
}

export interface RunGeoscriptOptions {
  code: string;
  // TODO: maybe make this optional
  ctxPtr: number;
  repl: Comlink.Remote<GeoscriptWorkerMethods>;
  /**
   * Map of material name → material entry. When a geoscript mesh references a material name
   * not present in this map, the runner automatically falls back to `FallbackMat`.
   * Defaults to `{}` when omitted.
   */
  materials?: Record<string, { def: MaterialDef; mat: MatEntry | THREE.Material }>;
  includePrelude: boolean;
  materialOverride?: 'wireframe' | 'wireframe-xray' | 'normal' | null;
  onStart?: () => void;
  onError?: (error: string) => void;
  renderMode?: boolean;
  modules?: Record<string, string>;
  /**
   * Sources to use to build the ambient scope (cloned for each module evaluation).
   * Typically `[prelude_src, globals_src]`. Empty array clears any existing ambient.
   * When omitted, ambient scope is left untouched (caller is responsible for
   * clearing it via a prior `reset()`).
   */
  ambientSources?: string[];
}

export interface GeoscriptRunResult {
  objects: GeneratedObject[];
  stats: RunStats;
  error: string | null;
}
