import * as THREE from 'three';

import noiseShader from 'src/viz/shaders/noise.frag?raw';

import buildingGeomShader from './shaders/buildingGeom.glsl?raw';
import gradientGlsl from './shaders/gradient.glsl?raw';
import groundGlsl from './shaders/ground.glsl?raw';
import hazeFieldShader from './shaders/hazeField.glsl?raw';
import skyUnifiedPrelude from './shaders/skyUnified.prelude.frag?raw';
import type { BuildingsLayerConfig } from './layers/BuildingsLayer';
import type { CloudsLayerConfig } from './layers/CloudsLayer';
import type { GroundLayerConfig } from './layers/GroundLayer';
import type { StarsLayerConfig } from './layers/StarsLayer';
import { asUniformRecord, type SkyStackUniforms } from './uniforms';

export interface UnifiedConfigs {
  stars?: StarsLayerConfig;
  buildings?: BuildingsLayerConfig;
  cloudsBack?: CloudsLayerConfig;
  cloudsFront?: CloudsLayerConfig;
  ground?: GroundLayerConfig;
}

export interface UnifiedLayerUniforms {
  stars?: Record<string, THREE.IUniform>;
  buildings?: Record<string, THREE.IUniform>;
  cloudsBack?: Record<string, THREE.IUniform>;
  cloudsFront?: Record<string, THREE.IUniform>;
  ground?: Record<string, THREE.IUniform>;
}

/**
 * One layer in the unified compositor. The compose stage runs them in
 * declaration order (caller hands them in front-to-back) and wraps every body
 * in `if (accumAlpha < SKY_SATURATION_ALPHA) { body }`, so any layer that
 * pushes accumAlpha past saturation auto-skips everything behind it.
 *
 * `body` must call `accumulate(color, emissive, alpha, emissiveAlpha)`
 * exactly once per contributing fragment (skipping is fine — a layer that has
 * nothing to add can early-return). `color` and the alpha-blend channel are
 * pre-multiplied at the call site (cloud passes `haze.rgb * haze.a`, etc).
 *
 * `gate` is an optional cheap predicate that gates body evaluation; it should
 * reference only compositor-provided variables (`dir`, `elev`, `azimuth`,
 * `cosElev`, `aboveHorizon`, `horizonBlend`, `baseGradient`) — never the state
 * of any other layer. This is purely a per-layer perf optimization (skip the
 * function call when we know it would no-op anyway), not a coupling point.
 *
 * `name` is purely for the boundary comment in the generated shader.
 */
interface SkyLayer {
  name: string;
  body: string;
  gate?: string;
}

const vec3Uniform = (v: [number, number, number] | undefined, fallback: [number, number, number]) =>
  new THREE.Vector3(...(v ?? fallback));

export const createStarsOwnUniforms = (c: StarsLayerConfig): Record<string, THREE.IUniform> => ({
  uStarColor: { value: new THREE.Color(c.color ?? 0xffffff) },
  uStarIntensity: { value: c.intensity },
  uStarDensity: { value: c.density },
  uStarThreshold: { value: c.threshold },
  uStarSize: { value: c.size },
  uStarTwinkleSpeed: { value: c.twinkleSpeed },
  uStarTwinkleDepth: { value: c.twinkleDepth ?? 0.25 },
  uStarMinElev: { value: c.minElev ?? 0.04 },
  uStarFadeRange: { value: c.fadeRange ?? 0.03 },
});

export const createBuildingsOwnUniforms = (c: BuildingsLayerConfig): Record<string, THREE.IUniform> => ({
  uBuildingCount: { value: c.buildingCount },
  uBuildingPresence: { value: c.buildingPresence ?? 0.85 },
  uBuildingGap: { value: c.buildingGap ?? 0.15 },
  uBuildingMinHeight: { value: c.buildingMinHeight },
  uBuildingMaxHeight: { value: c.buildingMaxHeight },
  uFloorsMin: { value: c.floorsMin ?? 4 },
  uFloorsMax: { value: c.floorsMax ?? 16 },
  uWindowsMin: { value: c.windowsMin ?? 2 },
  uWindowsMax: { value: c.windowsMax ?? 6 },
  uMaxFloorStride: { value: c.maxFloorStride ?? 2 },
  uMaxWindowStride: { value: c.maxWindowStride ?? 1 },
  uLitFractionMin: { value: c.litFractionMin ?? 0.2 },
  uLitFractionMax: { value: c.litFractionMax ?? 0.8 },
  uGroundElev: { value: c.groundElev ?? 0.0 },
  uWindowWidth: { value: c.windowWidth ?? 0.4 },
  uWindowHeight: { value: c.windowHeight ?? 0.5 },
  uCityColor: { value: new THREE.Color(c.color ?? 0xffb070) },
  uCityColorAlt: { value: new THREE.Color(c.colorAlt ?? 0xffd89a) },
  uCityIntensity: { value: c.intensity },
  uTwinkleSpeed: { value: c.twinkleSpeed },
  uTwinkleDepth: { value: c.twinkleDepth ?? 0.15 },
  uSilhouetteDarkness: { value: c.silhouetteDarkness ?? 0.15 },
});

export const createCloudsOwnUniforms = (
  c: CloudsLayerConfig,
  prefix: 'Back' | 'Front'
): Record<string, THREE.IUniform> => ({
  [`uHaze${prefix}Color`]: { value: new THREE.Color(c.color) },
  [`uHaze${prefix}HighColor`]: { value: new THREE.Color(c.highColor ?? c.color) },
  [`uHaze${prefix}Intensity`]: { value: c.intensity },
  [`uHaze${prefix}Center`]: { value: c.center },
  [`uHaze${prefix}Width`]: { value: c.width },
  [`uHaze${prefix}Sharpness`]: { value: c.sharpness ?? 0.15 },
  [`uHaze${prefix}Scale`]: { value: vec3Uniform(c.scale, [1, 4, 1]) },
  [`uHaze${prefix}Speed`]: { value: vec3Uniform(c.speed, [0, 0, 0]) },
  [`uHaze${prefix}Octaves`]: { value: c.octaves ?? 4 },
  [`uHaze${prefix}Lacunarity`]: { value: c.lacunarity ?? 2.0 },
  [`uHaze${prefix}Gain`]: { value: c.gain ?? 0.5 },
  [`uHaze${prefix}Bias`]: { value: c.bias ?? 0.0 },
  [`uHaze${prefix}Pow`]: { value: c.pow ?? 1.0 },
});

export const createGroundOwnUniforms = (c: GroundLayerConfig): Record<string, THREE.IUniform> => ({
  uGroundHeight: { value: c.height ?? 100 },
  uGroundHorizonFadeStart: { value: c.horizonFadeStart ?? 0.0 },
  uGroundHorizonFadeEnd: { value: c.horizonFadeEnd ?? 0.08 },
  uGroundAtmoTintColor: { value: new THREE.Color(c.atmosphericTint?.color ?? 0x000000) },
  uGroundAtmoTintRange: { value: c.atmosphericTint?.range ?? 0.2 },
  uGroundAtmoTintStrength: { value: c.atmosphericTint?.strength ?? 0 },
  ...(c.uniforms ?? {}),
});

/**
 * Textual substitution helper for duplicating `hazeField.glsl` with per-instance
 * uniform and function names. Keeps two independent cloud layers in one shader
 * without struct-uniform complexity. The base file only defines `skyFbm` + its
 * `MAX_HAZE_OCTAVES` reference, but we always emit `skyFbm` once (see
 * `buildHazeShared`).
 */
const buildHazeInstance = (prefix: 'Back' | 'Front'): string => {
  // Rename uniforms (uHaze* → uHaze<prefix>*) and the entry function
  // (sampleHaze → sampleHaze<prefix>); strip the shared `skyFbm` body that's
  // emitted once by `buildHazeShared`.
  let src = hazeFieldShader;
  src = src.replace(/\buHaze([A-Z])/g, `uHaze${prefix}$1`);
  src = src.replace(/\bsampleHaze\b/g, `sampleHaze${prefix}`);
  src = src.replace(/float skyFbm\([\s\S]*?\n\}\n/, '');
  return src;
};

const buildHazeShared = (): string => {
  // Extract just the `skyFbm` function from hazeField.glsl. The
  // `MAX_HAZE_OCTAVES` define is now emitted by the unified-shader builder
  // alongside the other count constants.
  const fbmMatch = hazeFieldShader.match(/float skyFbm\([\s\S]*?\n\}/);
  if (!fbmMatch) {
    throw new Error('hazeField.glsl layout changed — skyFbm extraction failed');
  }
  return `${fbmMatch[0]}\n`;
};

export const buildUnifiedSkyShader = (
  shared: SkyStackUniforms,
  configs: UnifiedConfigs,
  groundPaintShaderSource?: string
): {
  fragmentShader: string;
  uniforms: Record<string, THREE.IUniform>;
  ownUniforms: UnifiedLayerUniforms;
} => {
  const ownUniforms: UnifiedLayerUniforms = {};
  const allUniforms: Record<string, THREE.IUniform> = { ...asUniformRecord(shared) };

  // Count constants get baked as #defines so loop bounds + uniform-array
  // sizes are literal compile-time constants the driver can unroll/inline.
  // `MAX_HAZE_OCTAVES` is the upper bound across all configured cloud layers
  // (the per-cloud `uHazeXxxOctaves` uniform still cuts the loop short below
  // that bound at runtime). At least 1 to keep the loop body well-formed.
  // Stop / band counts come from the array sizes baked into `shared` by the
  // SkyStack constructor.
  const stopCount = shared.uStopColors.value.length;
  const bandCount = shared.uBandColors.value.length;
  const maxHazeOctaves = Math.max(configs.cloudsBack?.octaves ?? 0, configs.cloudsFront?.octaves ?? 0, 1);

  const defines: string[] = [
    `#define STOP_COUNT ${stopCount}`,
    `#define BAND_COUNT ${bandCount}`,
    `#define MAX_HAZE_OCTAVES ${maxHazeOctaves}`,
  ];
  const headerParts: string[] = [];
  // Layers are collected in front-to-back order. Each layer is wrapped in
  // `if (accumAlpha < SKY_SATURATION_ALPHA) { body }` at emit time so the
  // first layer that pushes accumAlpha past saturation short-circuits the
  // rest. The gradient layer is implicitly appended last as the back-most
  // fallback — it always emits alpha=1, filling whatever coverage remains.
  const layers: SkyLayer[] = [];

  // Gradient is always emitted (other layers' math depends on evalGradient).
  headerParts.push(gradientGlsl);

  if (configs.stars) {
    ownUniforms.stars = createStarsOwnUniforms(configs.stars);
    Object.assign(allUniforms, ownUniforms.stars);
    headerParts.push(STARS_HEADER);
  }
  if (configs.buildings) {
    ownUniforms.buildings = createBuildingsOwnUniforms(configs.buildings);
    Object.assign(allUniforms, ownUniforms.buildings);
    headerParts.push(buildingGeomShader);
    headerParts.push(WINDOWS_HELPERS);
  }
  if (configs.cloudsBack || configs.cloudsFront) {
    headerParts.push(buildHazeShared());
  }
  if (configs.cloudsBack) {
    ownUniforms.cloudsBack = createCloudsOwnUniforms(configs.cloudsBack, 'Back');
    Object.assign(allUniforms, ownUniforms.cloudsBack);
    headerParts.push(buildHazeInstance('Back'));
  }
  if (configs.cloudsFront) {
    ownUniforms.cloudsFront = createCloudsOwnUniforms(configs.cloudsFront, 'Front');
    Object.assign(allUniforms, ownUniforms.cloudsFront);
    headerParts.push(buildHazeInstance('Front'));
  }
  if (configs.ground) {
    if (!groundPaintShaderSource) {
      throw new Error('ground layer requires a paintShader source string');
    }
    ownUniforms.ground = createGroundOwnUniforms(configs.ground);
    Object.assign(allUniforms, ownUniforms.ground);
    // Paint shader first (defines `paintGround`), then ground.glsl (uses it).
    headerParts.push(`// === ground paintShader ===\n${groundPaintShaderSource}`);
    headerParts.push(groundGlsl);
  }

  // Front-to-back order: the layer that's nearest the viewer goes first, so
  // any opaque layer (alpha=1) auto-blocks everything behind via the
  // SKY_SATURATION_ALPHA early-out. The gradient is implicitly appended last
  // as the back-most fallback (always alpha=1, fills remaining coverage).
  if (configs.cloudsFront) {
    layers.push({ name: 'cloudsFront', body: CLOUDS_FRONT_BODY, gate: 'aboveHorizon' });
  }
  if (configs.buildings) {
    layers.push({ name: 'buildings', body: BUILDINGS_BODY, gate: 'aboveHorizon' });
  }
  if (configs.cloudsBack) {
    layers.push({ name: 'cloudsBack', body: CLOUDS_BACK_BODY, gate: 'aboveHorizon' });
  }
  if (configs.stars) {
    layers.push({ name: 'stars', body: STARS_BODY, gate: 'aboveHorizon' });
  }
  if (configs.ground) {
    // Ground is below-horizon; the gate mirrors sampleGround's internal
    // early-out so we skip the function call too. dir.y < 0.01 covers the
    // small derivative-coherence band above the geometric horizon.
    layers.push({ name: 'ground', body: GROUND_BODY, gate: 'dir.y < 0.01' });
  }
  // Gradient — always last; the back-most fallback. Outputs alpha=1 so it
  // fills any remaining coverage and definitively saturates the stack.
  layers.push({ name: 'gradient', body: GRADIENT_BODY });

  const fragmentShader = [
    defines.join('\n'),
    noiseShader,
    skyUnifiedPrelude,
    headerParts.join('\n// === header boundary ===\n'),
    buildMainBody(layers),
  ].join('\n');

  return { fragmentShader, uniforms: allUniforms, ownUniforms };
};

/**
 * Wrap a layer body in the saturation early-out + optional cheap gate.
 * Renders as:
 *   // === <name> ===
 *   if (accumAlpha < SKY_SATURATION_ALPHA) {
 *     [if (gate)] { body [}]
 *   }
 */
const emitLayer = (layer: SkyLayer): string => {
  const inner = layer.gate ? `if (${layer.gate}) {\n${layer.body}\n    }` : layer.body;
  return `
  // === ${layer.name} ===
  if (accumAlpha < SKY_SATURATION_ALPHA) {
    ${inner}
  }`;
};

const buildMainBody = (layers: SkyLayer[]): string => `
void main() {
  // Occlusion discard — applies to every layer equally. Discarding here
  // leaves BOTH oColor and oEmissive at their cleared values, which is what
  // the old per-layer pipeline produced (MainRenderPass overwrites oColor
  // where geo sits in front; emissiveRT alpha=0 means no contribution).
  discardIfOccluded();

  vec3 dir = skyViewDir();
  float elev, azimuth, cosElev;
  skyCoords(dir, elev, azimuth, cosElev);

  // Compositor-provided variables in scope for every layer body and gate:
  //   dir, elev, azimuth, cosElev   — view direction
  //   horizonBlend, aboveHorizon    — derived horizon helpers
  //   baseGradient                   — bandless gradient color, computed once
  //                                    so layers that need it (gradient,
  //                                    building silhouettes) don't re-run the
  //                                    oklab loop
  // Plus the file-scope accumulators (accumSkyColor, accumEmissive, etc.) and
  // the accumulate() helper from the prelude.
  float horizonBlend = smoothstep(-uHorizonBlend, uHorizonBlend, elev);
  bool aboveHorizon = elev >= -uHorizonBlend;
  vec3 baseGradient = evalGradient(elev);

  // === layers (front to back) ===
${layers.map(emitLayer).join('\n')}

  oColor = vec4(accumSkyColor, 1.0);
  oEmissive = vec4(accumEmissive, accumEmissiveAlpha);
}
`;

const STARS_HEADER = `
uniform vec3 uStarColor;
uniform float uStarIntensity;
uniform float uStarDensity;
uniform float uStarThreshold;
uniform float uStarSize;
uniform float uStarTwinkleSpeed;
uniform float uStarTwinkleDepth;
uniform float uStarMinElev;
uniform float uStarFadeRange;

// Returns (color * brightness, brightness) — the same premul-ish form the
// original stars.frag used so accumulation works additively.
vec4 sampleStars(vec3 dir, float elev, float azimuth, float cosElev) {
  if (uStarIntensity <= 0.0) {
    return vec4(0.0);
  }

  float horizonAlpha = smoothstep(uStarMinElev - uStarFadeRange, uStarMinElev + uStarFadeRange, elev);
  if (horizonAlpha <= 0.0) {
    return vec4(0.0);
  }

  float vCells = max(1.0, floor(uStarDensity * 0.5 + 0.5));
  float v = elev * 0.5 + 0.5;
  float ring = floor(v * vCells);

  float cellsPerRing = max(1.0, floor(uStarDensity * cosElev + 0.5));
  float u = azimuth / TWO_PI + 0.5;
  float cellX = mod(floor(u * cellsPerRing), cellsPerRing);
  vec2 cell = vec2(cellX, ring);
  vec2 local = vec2(fract(u * cellsPerRing), fract(v * vCells));

  float present = hash(cell);
  if (present > uStarThreshold) {
    return vec4(0.0);
  }

  vec2 starPos = vec2(hash(cell + vec2(1.3, 2.7)), hash(cell + vec2(4.7, 6.1)));
  float d = distance(local, starPos);
  float point = smoothstep(uStarSize, 0.0, d);
  if (point <= 0.0) {
    return vec4(0.0);
  }

  float fastPhase = hash(cell + vec2(7.7, 9.3)) * TWO_PI;
  float slowPhase = hash(cell + vec2(5.1, 2.9)) * TWO_PI;
  float fast = 0.5 + 0.5 * sin(uTime * uStarTwinkleSpeed + fastPhase);
  float slow = 0.5 + 0.5 * sin(uTime * uStarTwinkleSpeed * 0.15 + slowPhase);
  float flickerMag = smoothstep(0.4, 1.0, slow);
  float twinkle = 1.0 - uStarTwinkleDepth * flickerMag * fast;

  float brightness = point * twinkle * uStarIntensity * horizonAlpha;
  return vec4(uStarColor * brightness, brightness);
}
`;

// Body-only snippets — emitted inside `if (accumAlpha < SAT) { [if (gate)] {
// ... } }` by `emitLayer`. Each body calls `accumulate(color, emissive,
// alpha, emissiveAlpha)` exactly once per contributing fragment (or zero
// times if the layer has nothing to add). `color` and the alpha-blend
// channel are pre-multiplied at the call site.

const STARS_BODY = `
      // Pure emissive: no skyColor contribution, no alpha. Stars behind
      // alpha-blending layers get auto-attenuated by the (1 - accumAlpha)
      // weight inside accumulate().
      vec4 stars = sampleStars(dir, elev, azimuth, cosElev);
      accumulate(vec3(0.0), stars.rgb, 0.0, stars.a);
`;

const WINDOWS_HELPERS = `
uniform float uWindowWidth;
uniform float uWindowHeight;
uniform vec3 uCityColor;
uniform vec3 uCityColorAlt;
uniform float uCityIntensity;
uniform float uTwinkleSpeed;
uniform float uTwinkleDepth;
uniform float uSilhouetteDarkness;

// Returns (color * brightness, brightness) for a lit window at this fragment,
// or vec4(0) if outside a building body or in a dark cell.
vec4 sampleWindows(BuildingHit hit) {
  vec3 buildingCol = mix(uCityColor, uCityColorAlt, hit.colorH);

  float floorF = hit.localY * hit.floorCount;
  float windowF = hit.localX * hit.windowCount;
  float cellRate = max(fwidth(floorF), fwidth(windowF));
  float lod = smoothstep(0.35, 1.0, cellRate);

  float avgCoverage = uWindowWidth * uWindowHeight * hit.litFrac /
                      max(hit.floorStride * hit.windowStride, 1.0);

  float sharp = 0.0;
  vec3 sharpCol = buildingCol;
  float twinkle = 1.0;

  bool strideOk = mod(hit.floorIdx, hit.floorStride) < 0.5 &&
                  mod(hit.windowIdx, hit.windowStride) < 0.5;
  if (strideOk) {
    vec2 wCell = hit.bCell + vec2(hit.windowIdx * 1.7 + 3.0, hit.floorIdx * 2.3 + 7.0);
    float windowLitH = hash(wCell);
    if (windowLitH <= hit.litFrac) {
      vec2 halfSize = vec2(uWindowWidth, uWindowHeight) * 0.5;
      vec2 offset = abs(hit.cellLocal - 0.5);
      vec2 outside = offset - halfSize;
      float sdf = max(outside.x, outside.y);
      float aaW = max(fwidth(sdf), 1e-5) * 0.5;
      sharp = 1.0 - smoothstep(-aaW, aaW, sdf);

      float winColorH = hash(wCell + vec2(13.3, 17.1));
      sharpCol = mix(buildingCol, uCityColorAlt, winColorH * 0.5);

      float fastPhase = windowLitH * TWO_PI;
      float slowPhase = winColorH * TWO_PI;
      float fast = 0.5 + 0.5 * sin(uTime * uTwinkleSpeed + fastPhase);
      float slow = 0.5 + 0.5 * sin(uTime * uTwinkleSpeed * 0.15 + slowPhase);
      float flickerMag = smoothstep(0.4, 1.0, slow);
      twinkle = 1.0 - uTwinkleDepth * flickerMag * fast;
    }
  }

  float coverage = mix(sharp * twinkle, avgCoverage, lod);
  if (coverage <= 0.0) {
    return vec4(0.0);
  }

  float brightness = coverage * uCityIntensity;
  return vec4(sharpCol * brightness, brightness);
}
`;

const BUILDINGS_BODY = `
      // Probe runs inline (kept layer-local — no other layer needs to know
      // about its result). On hit the layer emits an opaque silhouette
      // (alpha=1) plus window emissive — accumAlpha saturates and any layers
      // behind get short-circuited.
      BuildingHit hit = probeBuilding(elev, azimuth);
      if (hit.hasBody) {
        // Silhouette color reuses the cached baseGradient (no bands; bands
        // fade out at horizon via horizonBlend anyway, so the visible
        // difference is nil).
        vec3 silhouette = baseGradient * uSilhouetteDarkness;
        vec4 win = sampleWindows(hit);
        accumulate(silhouette, win.rgb, 1.0, win.a);
      }
`;

const CLOUDS_BACK_BODY = `
      // Alpha-blend cloud — color premultiplied by haze.a at the call site so
      // accumulate() applies the same (1 - accumAlpha) weighting to both the
      // color contribution and the alpha increment.
      vec4 haze = sampleHazeBack(dir, elev);
      accumulate(haze.rgb * haze.a, vec3(0.0), haze.a, 0.0);
`;

const CLOUDS_FRONT_BODY = `
      vec4 haze = sampleHazeFront(dir, elev);
      accumulate(haze.rgb * haze.a, vec3(0.0), haze.a, 0.0);
`;

const GROUND_BODY = `
      // sampleGround keeps its own dir.y > 0.01 internal early-out as a
      // safety net, but the outer gate (dir.y < 0.01) means we never reach
      // it from the unsafe side. Ground is purely emissive — alpha=0 means
      // it doesn't block the gradient's belowColor showing through behind
      // (matches the old "ground paint over uBelowColor" look).
      vec4 g;
      sampleGround(dir, g);
      accumulate(vec3(0.0), g.rgb * g.a, 0.0, g.a);
`;

const GRADIENT_BODY = `
      // Always-last fallback layer. Outputs alpha=1 to fill any remaining
      // coverage. Bands are part of the gradient layer (additive sky color)
      // and get auto-attenuated by (1 - accumAlpha) along with the gradient
      // itself, matching how clouds dimmed bands in the back-to-front model.
      vec3 g = baseGradient + evalBands(elev, cosElev) * horizonBlend;
      accumulate(g, vec3(0.0), 1.0, 0.0);
`;
