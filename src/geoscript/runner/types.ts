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
  materials: Record<string, { def: MaterialDef; mat: MatEntry | THREE.Material }>;
  includePrelude: boolean;
  materialOverride?: 'wireframe' | 'normal' | null;
  onStart?: () => void;
  onError?: (error: string) => void;
  renderMode?: boolean;
}

export interface GeoscriptRunResult {
  objects: GeneratedObject[];
  stats: RunStats;
  error: string | null;
}
