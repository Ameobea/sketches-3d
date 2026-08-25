import type * as THREE from 'three';
import type * as Comlink from 'comlink';
import type { GeoscriptWorkerMethods } from '../geoscriptWorker.worker';
import type { MaterialDef } from '../materials';
import type { TreeKind } from '../geotoyAPIClient';
import type { ChannelStats } from '../textureStats';

/** Cross-run const-eval cache occupancy. Textures dominate it, so it is the number that goes
 *  wrong first in a long editing session — hence the cap, reported alongside. */
export interface ConstEvalCacheStats {
  entries: number;
  bytes: number;
  maxBytes: number;
}

export interface RunStats {
  runtimeMs: number;
  renderedMeshCount: number;
  renderedPathCount: number;
  renderedLightCount: number;
  renderedTextureCount: number;
  totalVtxCount: number;
  totalFaceCount: number;
  /** Async dep names actually used during the eval (from the Rust bitmask). */
  asyncDeps: string[];
  constEvalCache: ConstEvalCacheStats;
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
  /** Module that rendered this light; empty string for ambient/global lights. */
  sourceModule: string;
}

/** Semantic role of a rendered texture output; drives colorspace handling and 3D-preview
 *  auto-binding. Mirrors the Rust `TextureUsage` enum. */
export type TextureUsage = 'albedo' | 'normal' | 'roughness' | 'height' | 'metalness' | 'ao' | 'mask';

export interface GeneratedTexture {
  type: 'texture';
  /** Output channel name (`render_texture(name=…)`); the stable reference key. */
  name: string;
  usage: TextureUsage | null;
  wrap: 'repeat' | 'clamp' | 'mirror';
  /** UI-owned GPU sampler/format params baked onto the handle at render time; null =
   *  unset (consumer defaults apply). Pixels stay f32 regardless — `format` only declares
   *  the GPU materialization encoding. */
  minFilter: string | null;
  magFilter: string | null;
  format: string | null;
  width: number;
  height: number;
  channels: number;
  /** Slice count: 1 for `render_texture` outputs, ≥2 for `render_texture_stack`. */
  layers: number;
  /** Row-major interleaved f32 with slices concatenated in layer order;
   *  len = width·height·channels·layers. Absent for 3-channel outputs, whose consumers all
   *  read `rgba` or `encoded` instead (see the worker's `getRenderedTexture`). */
  data?: Float32Array;
  /** Pixels pre-encoded (SIMD, in-worker) for the resolved materialization format;
   *  present only when that format is a u8 format. */
  encoded?: Uint8Array;
  /** The resolved format `encoded` was encoded for; consumers must check it matches their
   *  own resolution before using `encoded`. */
  encodedFormat?: string;
  /** RGBA-expanded copy of `data`, present iff channels === 3 — the one count the 2D
   *  preview can't upload direct (RGB32F isn't color-renderable, breaking GPU mipgen). */
  rgba?: Float32Array;
  sourceModule: string;
  /** Stable across runs for unchanged textures (cache replay preserves it). */
  textureId: number;
  /** Per-channel value stats of the first layer. */
  stats: ChannelStats[];
}

export type GeneratedObject = GeneratedMesh | GeneratedPath | GeneratedLight | GeneratedTexture;

/** One output's injected GPU params; omitted fields mean unset. */
export interface TextureParamsEntry {
  tabId: string;
  name: string;
  minFilter?: string;
  magFilter?: string;
  format?: string;
}

/** A handle value the host injects per-run, keyed `moduleName → handleId`. Covers both
 *  draggable gizmos and control-panel inputs (they share the injection store). `value`
 *  carries the numeric payload (3 for `vec3`/`color`, 16 col-major for `transform`, 1 for
 *  `float`/`int`/`bool`, 3·N for `spline`); `str_value` carries the `string`/`select` payload. */
export interface GizmoValueWire {
  kind:
    | 'vec3'
    | 'transform'
    | 'float'
    | 'int'
    | 'bool'
    | 'color'
    | 'string'
    | 'select'
    | 'spline'
    | 'ramp'
    | 'image_levels'
    | 'uv_params';
  value?: number[];
  str_value?: string;
}
export type GizmoValuesByModule = Record<string, Record<string, GizmoValueWire>>;

/** An `input_*(...)` control site reported by the runtime for the last eval. */
export interface RenderedControl {
  sourceModule: string | null;
  handleId: string;
  kind: 'float' | 'int' | 'bool' | 'color' | 'select' | 'spline' | 'ramp' | 'image_levels' | 'uv_params';
  /** Display label override; falls back to `handleId` when null. */
  label: string | null;
  /** Numeric payload (float/int: 1 num; color: 3 rgb; spline: 3·N points). Empty for select. */
  value: number[];
  /** Chosen option for `select`; serialized `RampSpecJson` for `ramp`; null otherwise. */
  str_value: string | null;
  min: number | null;
  max: number | null;
  step: number | null;
  /** Numeric widget style: `slider` (default) | `entry` | `knob`. */
  style: string | null;
  /** Selectable options for `select`; empty otherwise. */
  options: string[];
  /** Input-texture stats behind `image_levels` (the histogram source); null otherwise. */
  stats: ChannelStats[] | null;
  /** Whether a stored value was injected for this site (vs. the source `default=`). */
  hasOverride: boolean;
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

/** How much of each rendered texture the host wants back. `gpu` drops the products only
 *  the Geotoy texture UI reads (value stats, f32 rgba expansion). */
export type TextureDetail = 'full' | 'gpu';

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
  /** Tree kind whose prelude to prepend; `undefined` when the entry tree ejected it. */
  preludeKind: TreeKind | undefined;
  materialOverride?: 'wireframe' | 'wireframe-xray' | 'normal' | null;
  onStart?: () => void;
  onError?: (error: string) => void;
  renderMode?: boolean;
  /** Defaults to `'full'`; level loading passes `'gpu'`. */
  textureDetail?: TextureDetail;
  modules?: Record<string, string>;
  /** Modules that get a tree kind's prelude prepended (resolved wasm-side), keyed by module
   *  name. Dependency roots never receive the entry prelude, so a synthesized module that
   *  wants one asks for it here. */
  modulePreludes?: Record<string, TreeKind>;
  /**
   * Sources to use to build the ambient scope (cloned for each module evaluation).
   * Typically `[prelude_src, globals_src]`. Empty array clears any existing ambient.
   * When omitted, ambient scope is left untouched (caller is responsible for
   * clearing it via a prior `reset()`).
   */
  ambientSources?: string[];
  /**
   * Per-tab ambient scopes for multi-tab runs, active tab LAST (its ambient construction
   * ends the RNG stream the entry program continues). Takes precedence over
   * `ambientSources`; preludes are resolved wasm-side from `preludeKind`.
   */
  tabAmbients?: { tabId: string; preludeKind: TreeKind | ''; globalsSource: string }[];
  /**
   * Gizmo handle values to inject, keyed `moduleName → handleId`. Always sent before
   * eval (the runner defaults to `{}`) so a prior run's values can't leak.
   */
  gizmoValues?: GizmoValuesByModule;
  /**
   * Per-output texture GPU params to inject (UI-owned; applied to rendered handles after
   * eval). Always sent (the runner defaults to `[]`) so a prior run's params can't leak.
   */
  textureParams?: TextureParamsEntry[];
  /**
   * Module name to attribute the entry program to. Hosts that qualify their module keys
   * (`<tabId>:<nodeName>`) must pass `<tabId>:_root` so the entry's own bare imports
   * resolve within that tab. Defaults to the unqualified `_root`.
   */
  rootModuleName?: string;
  vectorize?: VectorizeFlags;
}

/** Per-texel-body outcome of the texture auto-vectorizer (`tex_vectorize.rs`); one entry
 *  per body that ran this run. `line`/`col` are within `module`'s registered source. */
export interface VectorizeReport {
  vectorized: boolean;
  reason: string | null;
  line: number;
  col: number;
  module: string | null;
  /** Plan listing with per-step timings; only when `VectorizeFlags.profile` was on. */
  plan: string | null;
}

export interface VectorizeFlags {
  /** Kill switch: every texel body runs the per-texel interpreter (A/B timing). */
  disabled: boolean;
  /** Run both paths and assert bit-equality per body; slow, debugging only. */
  verify: boolean;
  /** Render each vectorized body's plan + per-step wall time into its report. */
  profile: boolean;
}

export interface GeoscriptRunResult {
  objects: GeneratedObject[];
  stats: RunStats;
  error: string | null;
  /** Gizmos evaluated this run, for the editor's interactive overlay (empty on error). */
  gizmos: RenderedGizmo[];
  /** Input controls declared this run, for the auto-generated control panel (empty on error). */
  controls: RenderedControl[];
  /** Empty on error. Bodies replayed from the const-eval cache don't run, so don't report. */
  vectorizeReports: VectorizeReport[];
}
