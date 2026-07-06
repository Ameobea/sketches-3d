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
  /** Stable across runs for unchanged meshes; used by the populator as a reuse key. */
  meshId: number;
}

export interface GeneratedPath {
  type: 'path';
  geometry: THREE.BufferGeometry;
  material: THREE.Material;
  castShadow: boolean;
  receiveShadow: boolean;
  pathId: number;
  /** Module that rendered this path; resolved to a tree node so subtree framing includes it. Empty string for ambient/global paths. */
  sourceModule: string;
}

export interface GeneratedLight {
  type: 'light';
  light: THREE.Light;
  lightId: number;
}

export type GeneratedObject = GeneratedMesh | GeneratedPath | GeneratedLight;

/** A handle value the host injects per-run, keyed `moduleName → handleId`. Covers both
 *  draggable gizmos and control-panel inputs (they share the injection store). `value`
 *  carries the numeric payload (3 for `vec3`/`color`, 16 col-major for `transform`, 1 for
 *  `float`/`int`/`bool`); `str_value` carries the `string`/`select` payload. */
export interface GizmoValueWire {
  kind: 'vec3' | 'transform' | 'float' | 'int' | 'bool' | 'color' | 'string' | 'select';
  value?: number[];
  str_value?: string;
}
export type GizmoValuesByModule = Record<string, Record<string, GizmoValueWire>>;

/** An `input_*(...)` control site reported by the runtime for the last eval. */
export interface RenderedControl {
  sourceModule: string | null;
  handleId: string;
  kind: 'float' | 'int' | 'bool' | 'color' | 'select';
  /** Display label override; falls back to `handleId` when null. */
  label: string | null;
  /** Numeric payload (float/int: 1 num; color: 3 rgb). Empty for select. */
  value: number[];
  /** Chosen option for `select`; null otherwise. */
  str_value: string | null;
  min: number | null;
  max: number | null;
  step: number | null;
  /** Numeric widget style: `slider` (default) | `entry` | `knob`. */
  style: string | null;
  /** Selectable options for `select`; empty otherwise. */
  options: string[];
}

/** A `gizmo(...)`/`gizmo_transform(...)` site reported by the runtime for the last eval. */
export interface RenderedGizmo {
  sourceModule: string | null;
  handleId: string;
  kind: 'vec3' | 'transform';
  origin: [number, number, number];
  /** vec3: 3 numbers; transform: 16 (column-major mat4). */
  value: number[];
  /** vec3 `absolute=` (transform always true); host resolves delta-vs-absolute mode from this. */
  absolute: boolean;
  /** Per-axis drag mask; `gizmo2d`/`gizmo1d` restrict the live gizmo to a subset. */
  axes: [boolean, boolean, boolean];
  /** Per-gizmo ghost override: `null` defers to the global setting; else forces on/off. */
  ghost: boolean | null;
}

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
  /**
   * Gizmo handle values to inject, keyed `moduleName → handleId`. Always sent before
   * eval (the runner defaults to `{}`) so a prior run's values can't leak.
   */
  gizmoValues?: GizmoValuesByModule;
}

export interface GeoscriptRunResult {
  objects: GeneratedObject[];
  stats: RunStats;
  error: string | null;
  /** Gizmos evaluated this run, for the editor's interactive overlay (empty on error). */
  gizmos: RenderedGizmo[];
  /** Input controls declared this run, for the auto-generated control panel (empty on error). */
  controls: RenderedControl[];
}
