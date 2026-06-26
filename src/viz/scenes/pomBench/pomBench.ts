/**
 * Standalone POM benchmark scene (Phase 0 of the capability-ladder plan).
 *
 * Loads the real boost_nova procedural materials and measures the GPU cost of
 * each under two pinned camera presets (head-on / grazing), so the ladder
 * migrations have hard before/after baselines. Raw rendering, no postprocessing:
 * every config renders into a fixed-resolution target so fragment coverage is
 * identical run-to-run. Primary timing is `EXT_disjoint_timer_query_webgl2`;
 * when that's blocked (common on Mac/ANGLE/Metal) it falls back to a
 * repeated-draw + `gl.finish()` slope fit, which is vsync-immune. The `samples`
 * debug view is read back as an algorithmic eval-count cross-check.
 *
 * `window.pomBench` exposes { run, setMaterial, setPreset, results } for poking.
 */
import * as THREE from 'three';

import type { Viz } from 'src/viz';
import type { VizConfig } from 'src/viz/conf';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { buildMaterial } from 'src/viz/materials';
import type { MaterialDef } from 'src/viz/levelDef/types';
import type { SceneConfig } from '..';
import boostNovaMaterials from 'src/levels/boost_nova/materials.json';
import raisedTilesAsset from 'src/assets/materials/procedural/raised_tiles/raised_tiles.json';
import chevronStripsAsset from 'src/assets/materials/procedural/chevron_strips/chevron_strips.json';
import pitGridAsset from 'src/assets/materials/procedural/pit_grid/pit_grid.json';
import sparseSeamsAsset from 'src/assets/materials/procedural/sparse_seams/sparse_seams.json';

const RT_W = 1280;
const RT_H = 720;
const FIXED_TIME = 10.0;
const WARMUP_FRAMES = 30;
const GPU_SAMPLES = 90;
const SLOPE_REPEATS = 14;
const SLOPE_KS = [1, 2, 4, 8];

/** Materials worth a baseline, with their ladder classification. The three raised_tiles rows
 *  are an ablation of the same material: field (re-project+re-hash per sample) → projectedField
 *  (projection hoisted) → grid (projection hoisted + per-cell hash cached). */
const BENCH_MATERIALS: { name: string; cls: string }[] = [
  { name: 'grooved_plastic', cls: 'L2 grid (parity)' },
  { name: 'panel_seams', cls: 'L2 projectedField' },
  { name: 'superellipse_tiles', cls: 'L2 ~analytic' },
  { name: 'grate_trench', cls: 'L2 1D / Tier-A' },
  { name: 'triangle_grid', cls: 'L1 tri-lattice' },
  { name: 'chevron_strips', cls: 'L1 chevron / hitType' },
  { name: 'pit_grid', cls: 'L1 pit SDF' },
  { name: 'sparse_seams', cls: 'L1 sparse seams' },
  { name: 'raised_tiles_field', cls: 'L2 hash · field' },
  { name: 'raised_tiles_proj', cls: 'L2 hash · projField' },
  { name: 'raised_tiles', cls: 'L2 hash · grid' },
];

interface Preset {
  pos: THREE.Vector3;
  target: THREE.Vector3;
  up: THREE.Vector3;
  fov: number;
}

// The bench surface is a large horizontal floor (normal +Y). head-on looks
// straight down (NdotV≈1, frame-filling); grazing skims it toward the horizon
// (shallow incidence over a big chunk of frame → long marches, the worst case).
const PRESETS: Record<string, Preset> = {
  headOn: {
    pos: new THREE.Vector3(0, 12, 0),
    target: new THREE.Vector3(0, 0, 0),
    up: new THREE.Vector3(0, 0, -1),
    fov: 60,
  },
  grazing: {
    pos: new THREE.Vector3(0, 1.5, 28),
    target: new THREE.Vector3(0, 0, -60),
    up: new THREE.Vector3(0, 1, 0),
    fov: 60,
  },
  // Grazing along world-X (down a grate trench), so the view crosses the thin
  // slat-gap walls perpendicularly — the worst case for safeStep bracket precision,
  // where coarse refinement serrates the long feature edges.
  grazingX: {
    pos: new THREE.Vector3(26, 2.2, 0),
    target: new THREE.Vector3(-90, 0.5, 0),
    up: new THREE.Vector3(0, 1, 0),
    fov: 58,
  },
  // High top-down for large-cell materials (sparse_seams ~40u cells): spans ~90 units so several
  // cells are visible while the footprint stays small enough to keep seams + relief crisp.
  aerial: {
    pos: new THREE.Vector3(0, 80, 0),
    target: new THREE.Vector3(0, 0, 0.0001),
    up: new THREE.Vector3(0, 0, -1),
    fov: 60,
  },
};

const glslModules = import.meta.glob(
  [
    '/src/levels/boost_nova/*.glsl',
    '/src/assets/materials/procedural/raised_tiles/*.glsl',
    '/src/assets/materials/procedural/chevron_strips/*.glsl',
    '/src/assets/materials/procedural/pit_grid/*.glsl',
    '/src/assets/materials/procedural/sparse_seams/*.glsl',
  ],
  { query: '?raw', import: 'default', eager: true }
) as Record<string, string>;
const glslByName: Record<string, string> = {};
for (const [path, src] of Object.entries(glslModules)) {
  glslByName[path.split('/').pop()!] = src;
}

// raised_tiles ships at the `grid` tier; these reconstruct its pre-cache height for the in-bench
// ablation. `field` re-projects + re-hashes per sample; `projectedField` hoists the projection
// only; `grid` (the file) also caches the per-cell hash. All three are output-identical.
const RT_FIELD_HEIGHT = /* glsl */ `float getPomHeight(vec3 pos, vec3 normal, float curTimeSeconds) {
  vec2 cellId, cl, edgeDir;
  float b = rtCellField(rtProjectUV(pos, normal), cellId, cl, edgeDir);
  float cThis = rtTileCarve(cellId);
  if (b >= RT_WALL_W) { return cThis; }
  return rtWallCarve(b, cThis, rtTileCarve(cellId + edgeDir));
}`;
const RT_PROJ_HEIGHT = /* glsl */ `float gridHeight(vec2 uv, float curTimeSeconds) {
  vec2 cellId, cl, edgeDir;
  float b = rtCellField(uv, cellId, cl, edgeDir);
  float cThis = rtTileCarve(cellId);
  if (b >= RT_WALL_W) { return cThis; }
  return rtWallCarve(b, cThis, rtTileCarve(cellId + edgeDir));
}`;

const RT_VARIANTS = new Set(['raised_tiles', 'raised_tiles_field', 'raised_tiles_proj']);

// pit_grid ships one library file (square, aligned); these inject the shape/stagger defines via
// baked `shaders.constants` so the bench can render every option (loadLevelData's `extends`/constants
// path is bypassed here, but buildMaterial still bakes `shaders.constants`).
const PIT_GRID_CONSTANTS: Record<string, Record<string, unknown>> = {
  pit_grid: {},
  pit_grid_diamond: { PG_SHAPE: { type: 'int', value: 1 } },
  pit_grid_triangle: { PG_SHAPE: { type: 'int', value: 2 } },
  pit_grid_trapezoid: { PG_SHAPE: { type: 'int', value: 3 } },
  pit_grid_stagger: { PG_STAGGER: { type: 'int', value: 1 }, PG_SHAPE: { type: 'int', value: 1 } },
};

const getRawDef = (name: string): unknown => {
  if (RT_VARIANTS.has(name)) {
    return (raisedTilesAsset as { material: unknown }).material;
  }
  if (name === 'chevron_strips') {
    return (chevronStripsAsset as { material: unknown }).material;
  }
  if (name === 'sparse_seams' || name === 'sparse_seams_demo') {
    const base = structuredClone((sparseSeamsAsset as { material: any }).material);
    // _demo shrinks the plate grid (and thus the derived seam pitch) so many cells fit a bench frame.
    if (name === 'sparse_seams_demo') {
      (base.shaders ??= {}).constants = {
        SS_PLATE_W: { type: 'float', value: 7 },
        SS_PLATE_H: { type: 'float', value: 6 },
      };
    }
    return base;
  }
  if (name in PIT_GRID_CONSTANTS) {
    const base = structuredClone((pitGridAsset as { material: any }).material);
    const consts = PIT_GRID_CONSTANTS[name];
    if (Object.keys(consts).length > 0) {
      (base.shaders ??= {}).constants = consts;
    }
    return base;
  }
  const raw = (boostNovaMaterials.materials as Record<string, unknown>)[name];
  if (!raw) {
    throw new Error(`pomBench: material "${name}" not in boost_nova/materials.json`);
  }
  return raw;
};

// Clone the raw def, numericize "#rrggbb" colors (the level loader's Zod transform
// normally does this; we import the JSON raw), and inline `{ file }` shader refs to
// GLSL strings (the server does this in prod).
const loadInlinedDef = (name: string): any => {
  const def = structuredClone(getRawDef(name)) as any;
  for (const key of ['color', 'sheenColor']) {
    if (typeof def.props?.[key] === 'string') {
      def.props[key] = new THREE.Color(def.props[key]).getHex();
    }
  }
  const shaders = def.shaders ?? {};
  for (const key of Object.keys(shaders)) {
    const ref = shaders[key];
    if (ref && typeof ref === 'object' && 'file' in ref) {
      const src = glslByName[ref.file];
      if (src === undefined) {
        throw new Error(`pomBench: missing GLSL file "${ref.file}" for material "${name}"`);
      }
      shaders[key] = src;
    }
  }
  return def;
};

// Force the marcher fully active and unbounded so we measure the raw march cost
// rather than the LOD/silhouette systems.
const buildBenchMaterial = (name: string, debugSamples: boolean): THREE.Material => {
  const def = loadInlinedDef(name);
  const shaders = def.shaders ?? {};
  const pom = (def.options ??= {}).pom ?? {};
  pom.lodFadeStart = 1e6;
  pom.lodFadeRange = 1;
  pom.boundedSilhouette = false;
  if (debugSamples) {
    pom.debug = 'evals';
  }
  // raised_tiles ablation twins: drop back to the pre-cache height variants. These stay on the
  // fixed-step march (safeStep stripped) so they remain the hoist/cache baselines; the shipping
  // `raised_tiles` row carries safeStep.
  if (name === 'raised_tiles_field' || name === 'raised_tiles_proj') {
    delete pom.intersect;
    delete pom.minFeatureWidth;
    delete pom.lateralDist;
    delete pom.cellPitch;
    delete pom.cellType;
    if (name === 'raised_tiles_field') {
      pom.tier = 'field';
      shaders.pomHeightShader = RT_FIELD_HEIGHT;
    } else {
      pom.tier = 'projectedField';
      shaders.pomHeightShader = RT_PROJ_HEIGHT;
    }
  }
  def.options.pom = pom;
  return buildMaterial(def as MaterialDef, new Map());
};

// Visual-comparison build: native material settings except POM LOD-fade forced off
// (so the whole floor shows relief, including the grazing distance). `forceMarch`
// strips safeStep back to the fixed-step marcher for before/after captures.
const buildShotMaterial = (name: string, forceMarch: boolean, refine?: number): THREE.Material => {
  const def = loadInlinedDef(name);
  const pom = (def.options ??= {}).pom ?? {};
  pom.lodFadeStart = 1e6;
  pom.lodFadeRange = 1;
  if (forceMarch) {
    delete pom.intersect;
    delete pom.minFeatureWidth;
    delete pom.lateralDist;
  }
  if (refine !== undefined) {
    pom.refinement = 'binary';
    pom.refinementSteps = refine;
  }
  def.options.pom = pom;
  return buildMaterial(def as MaterialDef, new Map());
};

const median = (xs: number[]): number => {
  const s = [...xs].sort((a, b) => a - b);
  return s.length === 0 ? NaN : s[Math.floor(s.length / 2)];
};
const slopeOf = (pts: [number, number][]): number => {
  const n = pts.length;
  let sx = 0,
    sy = 0,
    sxx = 0,
    sxy = 0;
  for (const [x, y] of pts) {
    sx += x;
    sy += y;
    sxx += x * x;
    sxy += x * y;
  }
  return (n * sxy - sx * sy) / (n * sxx - sx * sx);
};

const nextFrame = (): Promise<void> => new Promise(r => requestAnimationFrame(() => r()));

type Method = 'gpu-timer' | 'finish-slope';
interface AggEntry {
  material: string;
  cls: string;
  preset: 'headOn' | 'grazing';
  method: Method;
  runs: number[]; // per-sweep median ms; averaged across repeats
  evalProxy: number;
}

const mean = (xs: number[]): number => xs.reduce((a, b) => a + b, 0) / xs.length;
const stdev = (xs: number[]): number => {
  if (xs.length < 2) return 0;
  const m = mean(xs);
  return Math.sqrt(xs.reduce((a, b) => a + (b - m) ** 2, 0) / xs.length);
};

export const processLoadedScene = async (
  viz: Viz,
  _loadedWorld: THREE.Group,
  _vizConf: VizConfig
): Promise<SceneConfig> => {
  const { renderer } = viz;
  const gl = renderer.getContext() as WebGL2RenderingContext;
  const camera = viz.camera as THREE.PerspectiveCamera;

  // Dedicated scene so the sweep renders only the wall + lights, isolated from
  // whatever the framework parks in `viz.scene` (whose per-frame uniform setters
  // we bypass with the no-op render override below).
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x000000);
  scene.add(new THREE.AmbientLight(0xffffff, 0.6));
  const sun = new THREE.DirectionalLight(0xffffff, 2.2);
  sun.position.set(6, 10, 8);
  scene.add(sun);

  const wall: THREE.Mesh = new THREE.Mesh(new THREE.PlaneGeometry(400, 400), new THREE.MeshBasicMaterial());
  wall.rotation.x = -Math.PI / 2; // lie flat, normal +Y
  scene.add(wall);

  const flatMat = buildCustomShader({ color: 0x808080, roughness: 0.6, metalness: 0 }, {}, {});

  // Build every material (+ its samples-debug twin) up front.
  const built = new Map<string, { mat: THREE.Material; dbg: THREE.Material; cls: string }>();
  built.set('__flat__', { mat: flatMat, dbg: flatMat, cls: 'baseline (no POM)' });
  for (const { name, cls } of BENCH_MATERIALS) {
    built.set(name, {
      mat: buildBenchMaterial(name, false),
      dbg: buildBenchMaterial(name, true),
      cls,
    });
  }
  for (const { mat } of built.values()) {
    (mat as any).setCurTimeSeconds?.(FIXED_TIME);
  }

  const benchRT = new THREE.WebGLRenderTarget(RT_W, RT_H, {
    minFilter: THREE.NearestFilter,
    magFilter: THREE.NearestFilter,
    depthBuffer: true,
  });

  const applyPreset = (p: Preset) => {
    camera.fov = p.fov;
    camera.aspect = RT_W / RT_H;
    camera.near = 0.01;
    camera.far = 1000;
    camera.position.copy(p.pos);
    camera.up.copy(p.up);
    camera.lookAt(p.target);
    camera.updateProjectionMatrix();
    camera.updateMatrixWorld();
  };

  const renderToRT = () => {
    renderer.setRenderTarget(benchRT);
    renderer.render(scene, camera);
    renderer.setRenderTarget(null);
  };
  const renderToScreen = () => {
    renderer.setRenderTarget(null);
    renderer.render(scene, camera);
  };

  const timerExt = gl.getExtension('EXT_disjoint_timer_query_webgl2');

  // One GPU TIME_ELAPSED query around a single render; resolves a frame or two
  // later. Returns ms, or null if unavailable / flagged disjoint.
  const measureGpuMs = async (): Promise<number | null> => {
    if (!timerExt) return null;
    const q = gl.createQuery()!;
    gl.beginQuery(timerExt.TIME_ELAPSED_EXT, q);
    renderToRT();
    gl.endQuery(timerExt.TIME_ELAPSED_EXT);
    for (let i = 0; i < 90; i++) {
      await nextFrame();
      if (gl.getQueryParameter(q, gl.QUERY_RESULT_AVAILABLE)) {
        const disjoint = gl.getParameter(timerExt.GPU_DISJOINT_EXT);
        const ns = disjoint ? null : (gl.getQueryParameter(q, gl.QUERY_RESULT) as number);
        gl.deleteQuery(q);
        return ns === null ? null : ns / 1e6;
      }
    }
    gl.deleteQuery(q);
    return null;
  };

  // Fallback: render K times, gl.finish(), time it; sweep K and take the slope
  // (cancels fixed CPU/sync overhead, immune to vsync).
  const measureFinishMs = (): number => {
    const pts: [number, number][] = SLOPE_KS.map(k => {
      const t0 = performance.now();
      for (let i = 0; i < k; i++) {
        renderToRT();
      }
      gl.finish();
      return [k, performance.now() - t0];
    });
    return slopeOf(pts);
  };

  // Mean of (evals / worst-case) over POM-covered pixels, from the linear `evals`
  // view (grayscale = eval fraction). Covered = red>0 (every marched fragment does
  // ≥1 eval; background clears to 0), so coverage is divided out → comparable across
  // presets. Per-material normalized (fraction of that material's own worst case).
  const readEvalProxy = (dbg: THREE.Material): number => {
    wall.material = dbg;
    renderToRT();
    const buf = new Uint8Array(RT_W * RT_H * 4);
    renderer.readRenderTargetPixels(benchRT, 0, 0, RT_W, RT_H, buf);
    renderer.setRenderTarget(null);
    let sum = 0;
    let covered = 0;
    for (let i = 0; i < buf.length; i += 4) {
      if (buf[i] > 0) {
        sum += buf[i];
        covered++;
      }
    }
    return covered === 0 ? 0 : sum / 255 / covered;
  };

  const overlay = document.createElement('div');
  overlay.style.cssText =
    'position:fixed;top:8px;left:8px;z-index:9999;font:11px/1.45 monospace;' +
    'color:#cfd2d6;background:rgba(10,11,13,.86);padding:10px 12px;white-space:pre;' +
    'border:1px solid #2a2d31;max-width:92vw;overflow:auto;pointer-events:none';
  document.body.appendChild(overlay);
  const setStatus = (s: string) => {
    overlay.textContent = s;
  };

  const orderedKeys = ['__flat__', ...BENCH_MATERIALS.map(m => m.name)];
  const presetNames = ['headOn', 'grazing'] as const;
  const aggKey = (mat: string, preset: string) => `${mat}|${preset}`;
  const agg = new Map<string, AggEntry>();
  const published: Record<string, unknown>[] = [];

  const measureConfig = async (
    key: string,
    presetName: 'headOn' | 'grazing',
    tag: string
  ): Promise<{ ms: number; evalProxy: number; method: Method }> => {
    const entry = built.get(key)!;
    wall.material = entry.mat;
    applyPreset(PRESETS[presetName]);

    for (let i = 0; i < WARMUP_FRAMES; i++) {
      renderToScreen();
      await nextFrame();
    }

    let method: Method = 'finish-slope';
    const samples: number[] = [];
    if (timerExt) {
      method = 'gpu-timer';
      let misses = 0;
      while (samples.length < GPU_SAMPLES && misses < GPU_SAMPLES) {
        const ms = await measureGpuMs();
        if (ms !== null && ms > 0) {
          samples.push(ms);
        } else {
          misses++;
        }
        setStatus(`${tag} ${key} · ${presetName} · gpu-timer ${samples.length}/${GPU_SAMPLES}`);
      }
    }
    if (samples.length === 0) {
      method = 'finish-slope';
      for (let i = 0; i < SLOPE_REPEATS; i++) {
        samples.push(measureFinishMs());
        setStatus(`${tag} ${key} · ${presetName} · finish-slope ${i + 1}/${SLOPE_REPEATS}`);
        await nextFrame();
      }
    }
    // flat has no `samples` debug twin (dbg === mat), so its eval proxy is N/A.
    const evalProxy = entry.dbg === entry.mat ? NaN : readEvalProxy(entry.dbg);
    return { ms: median(samples), evalProxy, method };
  };

  const renderTable = (note: string) => {
    const ordered: AggEntry[] = [];
    for (const preset of presetNames) {
      for (const key of orderedKeys) {
        const e = agg.get(aggKey(key === '__flat__' ? 'flat' : key, preset));
        if (e) ordered.push(e);
      }
    }
    const flatBy: Record<string, number> = {};
    for (const e of ordered) {
      if (e.material === 'flat') flatBy[e.preset] = mean(e.runs);
    }
    const lines = ordered.map(e => {
      const m = mean(e.runs);
      const dpom = e.material === 'flat' ? '—' : (m - (flatBy[e.preset] ?? 0)).toFixed(3);
      return [
        e.material.padEnd(19),
        e.preset.padEnd(8),
        e.cls.padEnd(22),
        m.toFixed(3).padStart(8),
        ('±' + stdev(e.runs).toFixed(3)).padStart(8),
        dpom.padStart(9),
        (Number.isNaN(e.evalProxy) ? '—' : e.evalProxy.toFixed(3)).padStart(10),
      ].join(' ');
    });
    const table =
      `[pomBench] ${RT_W}×${RT_H}  method=${ordered[0]?.method}  dpr=${window.devicePixelRatio}  ${note}\n` +
      'material            preset   class                  full(ms)    ±std   Δpom(ms)  evalProxy\n' +
      lines.join('\n');
    setStatus(table + '\n\n(window.pomBench.results)');
    console.clear();
    console.log('%c' + table, 'font-family:monospace');
    published.length = 0;
    for (const e of ordered) {
      const m = mean(e.runs);
      published.push({
        material: e.material,
        preset: e.preset,
        class: e.cls,
        'full(ms)': +m.toFixed(3),
        std: +stdev(e.runs).toFixed(3),
        'Δpom(ms)': e.material === 'flat' ? null : +(m - (flatBy[e.preset] ?? 0)).toFixed(3),
        evalProxy: Number.isNaN(e.evalProxy) ? null : +e.evalProxy.toFixed(3),
        n: e.runs.length,
        method: e.method,
      });
    }
    console.table(published);
  };

  let running = false;
  const runAll = async (repeats = 1) => {
    if (running) return;
    running = true;
    agg.clear();
    try {
      for (let run = 0; run < repeats; run++) {
        const tag = `[pomBench] sweep ${run + 1}/${repeats} ·`;
        for (const preset of presetNames) {
          for (const key of orderedKeys) {
            const m = await measureConfig(key, preset, tag);
            const material = key === '__flat__' ? 'flat' : key;
            const k = aggKey(material, preset);
            let e = agg.get(k);
            if (!e) {
              e = { material, cls: built.get(key)!.cls, preset, method: m.method, runs: [], evalProxy: 0 };
              agg.set(k, e);
            }
            e.runs.push(m.ms);
            e.evalProxy = m.evalProxy;
            e.method = m.method;
          }
        }
        renderTable(`(${run + 1}/${repeats} sweeps)`);
      }
    } finally {
      running = false;
    }
  };

  // Take over rendering: viz's loop renders nothing, we drive the sweep below.
  viz.setRenderOverride(() => {});
  renderer.autoClear = true;
  wall.material = built.get(BENCH_MATERIALS[0].name)!.mat;
  applyPreset(PRESETS.headOn);
  renderToScreen();

  // sRGB target for visual before/after capture (benchRT stays linear for eval readback).
  const shotRT = new THREE.WebGLRenderTarget(RT_W, RT_H, { depthBuffer: true });
  shotRT.texture.colorSpace = THREE.SRGBColorSpace;
  const shotCanvas = document.createElement('canvas');
  shotCanvas.width = RT_W;
  shotCanvas.height = RT_H;
  const shotCtx = shotCanvas.getContext('2d')!;
  const shotBuf = new Uint8Array(RT_W * RT_H * 4);
  // Render one material/preset (native settings, LOD-fade off) into shotRT → PNG data URL.
  // mode 'march' strips safeStep for the before/after pair; reads pixels back so it's immune
  // to drawing-buffer clears.
  const captureShot = (name: string, presetName: string, mode: 'march' | 'safe', refine?: number): string => {
    wall.material = buildShotMaterial(name, mode === 'march', refine);
    (wall.material as any).setCurTimeSeconds?.(FIXED_TIME);
    applyPreset(PRESETS[presetName]);
    renderer.compile(scene, camera);
    renderer.setRenderTarget(shotRT);
    renderer.render(scene, camera);
    renderer.readRenderTargetPixels(shotRT, 0, 0, RT_W, RT_H, shotBuf);
    renderer.setRenderTarget(null);
    const img = shotCtx.createImageData(RT_W, RT_H);
    for (let y = 0; y < RT_H; y++) {
      const src = (RT_H - 1 - y) * RT_W * 4;
      img.data.set(shotBuf.subarray(src, src + RT_W * 4), y * RT_W * 4);
    }
    shotCtx.putImageData(img, 0, 0);
    return shotCanvas.toDataURL('image/png');
  };

  (window as any).pomBench = {
    results: published,
    run: runAll,
    shot: captureShot,
    setMaterial: (name: string, debug = false) => {
      wall.material = name === 'flat' ? flatMat : buildBenchMaterial(name, debug);
      (wall.material as any).setCurTimeSeconds?.(FIXED_TIME);
      renderToScreen();
    },
    setPreset: (name: 'headOn' | 'grazing') => {
      applyPreset(PRESETS[name]);
      renderToScreen();
    },
    presets: PRESETS,
  };

  setStatus('[pomBench] ready\n' + 'window.pomBench.run(5) to average 5 sweeps · .run() for one');
  // Kick off after a beat so shader compiles overlap the warmup, not the first sample.
  // setTimeout(() => runAll(1), 400);

  return {
    locations: {
      spawn: { pos: new THREE.Vector3(0, 12, 0), rot: new THREE.Vector3(0, 0, 0) },
    },
    spawnLocation: 'spawn',
    viewMode: {
      type: 'orbit',
      pos: new THREE.Vector3(0, 12, 0),
      target: new THREE.Vector3(0, 0, 0),
    },
  };
};
