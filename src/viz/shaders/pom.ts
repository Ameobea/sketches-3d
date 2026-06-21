export const POM_BOUNDED_SILHOUETTE_FLAG = 'pomBoundedSilhouette';

export const buildPomUniformDecls = (
  pom: boolean,
  pomBounded: boolean,
  pomHeightMap: boolean,
  pomSelfShadow: boolean
): string =>
  [
    pom ? 'uniform float pomDepth;' : '',
    pomBounded ? 'uniform highp sampler2D pomBackDepth; // R = euclidean dist camera->nearest back face' : '',
    pomBounded ? 'uniform vec2 pomResolution; // drawing-buffer size; the back-face RT may be lower-res' : '',
    pomHeightMap ? 'uniform sampler2D pomHeightMap;' : '',
    pomSelfShadow ? 'uniform vec3 pomShadowLightDir; // world-space direction toward the light' : '',
  ].join('\n');

export type PomTexturing = 'triplanar' | 'generated' | 'baseline' | 'tangent';

// Emits `getPomHeight()` + `samplePomHeightMap()`. The marcher sums both, so a
// no-op (returning 0) is emitted for whichever source the material omits.
//
// `textureLod(.,0)` rather than `texture2D`: the marcher calls this in
// non-uniform control flow (loop + conditional return) where implicit
// derivatives are undefined and the GPU picks a runaway mip. Triplanar is
// inlined for the same reason — the shared `triplanarTexture()` uses
// `texture2D`. `1. - x` matches white-is-high heightmap convention.
export const buildPomHeightSources = (opts: {
  hasHeightShader: boolean;
  hasHeightMap: boolean;
  pomTexturing: PomTexturing;
}): string => {
  const { hasHeightShader, hasHeightMap, pomTexturing } = opts;
  const proceduralDefault = hasHeightShader
    ? ''
    : 'float getPomHeight(vec3 _p, vec3 _N, float _t) { return 0.; }';
  const sampleFn = (() => {
    if (!hasHeightMap) {
      return 'float samplePomHeightMap(vec3 _p, vec3 _N) { return 0.; }';
    }
    if (pomTexturing === 'triplanar') {
      // Triplanar weights are invariant across the march (the base normal never changes), so
      // they're computed once per fragment into `_pomTriW` at the top of the POM main block.
      return /* glsl */ `
vec3 _pomTriW;
float samplePomHeightMap(vec3 p, vec3 _N) {
  vec3 sp = vTriplanarPos + (p - vWorldPos);
  vec2 _phUvScale = vec2(uvTransform[0][0], uvTransform[1][1]);
  float h = 0.;
  if (_pomTriW.x > 0.01) h += textureLod(pomHeightMap, sp.yz * _phUvScale, 0.0).r * _pomTriW.x;
  if (_pomTriW.y > 0.01) h += textureLod(pomHeightMap, sp.zx * _phUvScale, 0.0).r * _pomTriW.y;
  if (_pomTriW.z > 0.01) h += textureLod(pomHeightMap, sp.xy * _phUvScale, 0.0).r * _pomTriW.z;
  return 1. - h;
}`;
    }
    if (pomTexturing === 'tangent') {
      // Mesh-UV march: each marched point projects into the surface tangent frame
      // (`pomMeshUv`, emitted by customShader) so the height field follows the swept UV.
      return /* glsl */ `
float samplePomHeightMap(vec3 p, vec3 _N) { return 1. - textureLod(pomHeightMap, pomMeshUv(p), 0.0).r; }`;
    }
    return /* glsl */ `
float samplePomHeightMap(vec3 p, vec3 N) {
  vec2 uv = (uvTransform * vec3(generateUV(p, N), 1.)).xy;
  return 1. - textureLod(pomHeightMap, uv, 0.0).r;
}`;
  })();
  return `${proceduralDefault}\n${sampleFn}`;
};

export type PomRefinement = 'secant' | 'binary';

export const buildPomDefs = (opts: {
  pomSteps: number;
  pomBounded: boolean;
  // `projectedField` tier: emit `pomMarchProjected`, which hoists the dominant-axis
  // projection out of the march loop. The material supplies `gridHeight(vec2 uv, float t)`.
  pomProjected: boolean;
  // `grid` tier: emit `pomMarchGrid`, which additionally owns the square-lattice cell
  // decomposition and caches the material's `cellType` struct across the march. The material
  // supplies `struct <cellType>`, `gridComputeCell(vec2)`, and `gridHeight(GridCtx, <cellType>)`.
  pomGrid: boolean;
  cellType: string;
  // `projectedField` + `intersect: 'safeStep'`: emit `pomMarchProjectedSafe`, which strides by
  // the material's `gridLateralDist` (or uniformly) floored to `minFeatureWidth`, bracketing
  // walls instead of fixed-stepping. `pomSteps` becomes the max-stride cap.
  pomSafe: boolean;
  minFeatureWidth: number;
  lodFadeStart: number;
  lodFadeEnd: number;
  pomRefinement: PomRefinement;
  pomBinarySteps: number;
  // Skip binary refinement when the linear hit already pierced the floor by less
  // than this fraction of one step (in depth). 0 disables the skip.
  pomRefineSkip: number;
  // Material supplies `getPomNormal(...)`; use it instead of finite differences.
  pomHasNormalShader: boolean;
  // Per-pixel IGN phase offset for the linear march (turns step banding into noise).
  pomJitter: boolean;
  // Opt-in relief self-shadowing toward an explicit light direction.
  pomSelfShadow?: { steps: number; strength: number; softness: number };
  // Active debug view, if any; the `samples`/`skip` views enable per-fragment
  // instrumentation (compiled out otherwise).
  pomDebug?: PomDebugMode;
}): string => {
  const {
    pomSteps,
    pomBounded,
    pomProjected,
    pomGrid,
    cellType,
    pomSafe,
    minFeatureWidth,
    lodFadeStart,
    lodFadeEnd,
    pomRefinement,
    pomBinarySteps,
    pomRefineSkip,
    pomHasNormalShader,
    pomJitter,
    pomSelfShadow,
    pomDebug,
  } = opts;
  const useBinary = pomRefinement === 'binary';
  const debugCounters = pomDebug === 'samples' || pomDebug === 'evals' || pomDebug === 'skip';
  // Worst-case _pomSurf evals per fragment: linear march + binary refine (secant
  // adds none) + the 3 finite-difference normal taps (none with a normal shader).
  const maxSamples = pomSteps + (useBinary ? pomBinarySteps : 0) + (pomHasNormalShader ? 0 : 3);
  return /* glsl */ `
#define POM_STEPS ${pomSteps}
${pomSafe ? `#define POM_MIN_FEATURE ${minFeatureWidth.toFixed(6)}` : ''}
${pomSafe ? `#define POM_REFINE_TOL ${(0.01 * minFeatureWidth).toFixed(6)}` : ''}
${useBinary ? `#define POM_REFINE_BINARY\n#define POM_BINARY_STEPS ${pomBinarySteps}` : ''}
${useBinary && pomRefineSkip > 0 ? `#define POM_REFINE_SKIP ${pomRefineSkip.toFixed(4)}` : ''}
${debugCounters ? `#define POM_DEBUG_COUNTERS\n#define POM_DEBUG_MAX_SAMPLES ${maxSamples}` : ''}
${pomHasNormalShader ? '#define POM_HAS_NORMAL_SHADER' : ''}
${pomJitter ? '#define POM_JITTER' : ''}

#ifdef POM_JITTER
// Interleaved gradient noise (Jimenez 2014): per-pixel phase for the march so
// sample-interval banding becomes unstructured noise instead of coherent steps.
float pomIGN() {
  return fract(52.9829189 * fract(dot(gl_FragCoord.xy, vec2(0.06711056, 0.00583715))));
}
#define POM_JITTER_OFF pomIGN()
#else
#define POM_JITTER_OFF 0.0
#endif

#ifdef POM_DEBUG_COUNTERS
// Per-fragment instrumentation for the \`samples\` / \`skip\` debug views. File-scope
// globals reset per fragment invocation; written by _pomSurf / _pomRefineHit.
int _pomSampleCount = 0;   // total _pomSurf evals (march + refine + normal taps)
int _pomRefineState = 0;   // 0 = no refinement, 1 = refine skipped, 2 = bisected
vec3 _pomHeat(float t) {   // blue (cheap) -> green -> red (expensive)
  return clamp(vec3(1.5 - abs(4.0 * t - 3.0),
                    1.5 - abs(4.0 * t - 2.0),
                    1.5 - abs(4.0 * t - 1.0)), 0.0, 1.0);
}
#endif

// --- Procedural Parallax Occlusion Mapping (world space, subtractive only) ---
// The base polygon is the TOP of a virtual slab of thickness \`pomDepth\`
// extending inward along -N. \`getPomHeight()\` returns carved depth in [0,1];
// this wrapper scales it to world units.
float _pomSurf(vec3 p, vec3 N, float depth, float t) {
#ifdef POM_DEBUG_COUNTERS
  _pomSampleCount++;
#endif
  return clamp(getPomHeight(p, N, t) + samplePomHeightMap(p, N), 0., 0.8) * depth;
}

// Refine the bracketed crossing between \`pPrev\` (above floor) and \`p\` (below).
// Shared by both marchers so the secant/bisection logic stays in one place.
//   hPrev/dPrev = carved depth / ray-depth-below-base at pPrev
//   surfH/rayDepth = same quantities at p
vec3 _pomRefineHit(vec3 ro, vec3 N, float depth, float t,
                   vec3 pPrev, float hPrev, float dPrev,
                   vec3 p, float surfH, float rayDepth) {
#ifdef POM_REFINE_BINARY
  // The free secant root reuses the four values the linear search already has,
  // so it costs no \`_pomSurf\` eval. \`w\` is the fraction from p back toward pPrev.
  float overshoot = rayDepth - surfH;        // depth pierced past the floor at p (>=0)
  float prevGap   = hPrev - dPrev;           // depth above the floor at pPrev (>=0)
  float span      = overshoot + prevGap;     // residual swing across the bracketing step
  float w = span > 1e-6 ? overshoot / span : 0.0;
  // pPrev sits exactly on the surface (prevGap==0, e.g. the first step from an
  // uncarved/max-height start): the secant root is exact, so bisection would only
  // re-derive it. Independent of POM_REFINE_SKIP since this is exact, not a tolerance.
  if (prevGap <= 1e-6) {
#ifdef POM_DEBUG_COUNTERS
    _pomRefineState = 1;
#endif
    return mix(p, pPrev, w);
  }
#ifdef POM_REFINE_SKIP
  // rayDepth advances by exactly depth/POM_STEPS per step, so \`overshoot\` is how
  // far (in depth) the linear search overstepped the floor. Below a small
  // fraction of a step the secant root is already sub-step accurate and
  // bisection would not move it -> skip all refine evals. Safe on step
  // heightfields too: a small overshoot forces w->0, i.e. the secant collapses
  // onto p, which is the correct top-of-wall hit.
  if (overshoot <= POM_REFINE_SKIP * (depth / float(POM_STEPS))) {
#ifdef POM_DEBUG_COUNTERS
    _pomRefineState = 1;   // refine skipped
#endif
    return mix(p, pPrev, w);
  }
#endif
#ifdef POM_DEBUG_COUNTERS
  _pomRefineState = 2;     // full bisection
#endif
  // Deep plunge (steep wall / true step): bisect for robustness. Robust on step
  // heightfields where secant's linear between-samples assumption fails.
  vec3 lo = pPrev;
  vec3 hi = p;
  for (int bi = 0; bi < POM_BINARY_STEPS; bi++) {
    vec3 mid = 0.5 * (lo + hi);
    float midDepth = -dot(mid - ro, N);
    float midSurf = _pomSurf(mid, N, depth, t);
    if (midDepth >= midSurf) hi = mid; else lo = mid;
  }
  return hi;
#else
  float a = surfH - rayDepth;
  float b = hPrev - dPrev;
  float w = clamp(a / (a - b), 0.0, 1.0);   // secant root between samples
  return mix(p, pPrev, w);
#endif
}

// Linear-search + secant-refine relief intersection (Policarpo & Oliveira,
// "Real-Time Relief Mapping", I3D 2005; secant refine: Risser & Shah, "Faster
// Relief Mapping Using the Secant Method", 2007), recast world-space with a
// procedural height field. Phase 1: no \`discard\` — on no crossing, clamp to
// the deepest sample so the silhouette stays the base mesh.
vec3 pomMarch(vec3 ro, vec3 rd, vec3 N, float depth, float t) {
  float NdotVraw = dot(N, -rd);                  // ray descent rate per unit ray length
  float NdotV = max(NdotVraw, 1e-3);             // grazing clamp (step sizing only)
  float marchLen = depth / NdotV;
  float dStep = marchLen / float(POM_STEPS);
  float jit = POM_JITTER_OFF;

  vec3 pPrev = ro;
  float hPrev = 0.0;   // carved depth at previous sample
  float dPrev = 0.0;   // ray depth-below-base at previous sample

  for (int i = 1; i <= POM_STEPS; i++) {
    float s = dStep * (float(i) - jit);
    vec3 p = ro + rd * s;
    float rayDepth = s * NdotVraw;                // == -dot(p-ro,N), no large-coordinate cancellation
    float surfH = _pomSurf(p, N, depth, t);
    if (rayDepth >= surfH) {
      return _pomRefineHit(ro, N, depth, t, pPrev, hPrev, dPrev, p, surfH, rayDepth);
    }
    pPrev = p; hPrev = surfH; dPrev = rayDepth;
  }
  return pPrev;   // no crossing: clamp to deepest sample (Phase 1, no discard)
}

${
  pomBounded
    ? /* glsl */ `
// Back-face-depth-bounded convex variant (see pom-implementation-plan.md
// Appendix A for the formulation & lineage). Same kernel as \`pomMarch\`, but
// the ray is also clamped by the mesh's own nearest back face (\`maxRayLen\` =
// entry->exit chord). Thick interior: crosses the floor before the exit ->
// same as Phase-1. Near the silhouette: exits before reaching the floor ->
// \`carved\` -> caller discards (the per-step volume-exit test of Chen & Chang,
// PG/CGF 2008, retargeted to the back-face bound). Subtractive: displaced
// surface stays inside the hull, so the silhouette only ever recedes.
vec3 pomMarchBounded(vec3 ro, vec3 rd, vec3 N, float depth, float t, float maxRayLen, out bool carved) {
  carved = false;
  float NdotVraw = dot(N, -rd);
  float NdotV = max(NdotVraw, 1e-3);
  float marchLen = depth / NdotV;
  float dStep = marchLen / float(POM_STEPS);
  float jit = POM_JITTER_OFF;

  vec3 pPrev = ro;
  float hPrev = 0.0;
  float dPrev = 0.0;

  for (int i = 1; i <= POM_STEPS; i++) {
    float sRaw = dStep * (float(i) - jit);
    // Clamp the final tested point to the exact convex exit so the floor is
    // still evaluated there before declaring the fragment carved away.
    float s = min(sRaw, maxRayLen);
    vec3 p = ro + rd * s;
    float rayDepth = s * NdotVraw;                 // == -dot(p-ro,N), no large-coordinate cancellation
    float surfH = _pomSurf(p, N, depth, t);
    if (rayDepth >= surfH) {
      return _pomRefineHit(ro, N, depth, t, pPrev, hPrev, dPrev, p, surfH, rayDepth);
    }
    if (sRaw >= maxRayLen) {
      // Reached the back-face exit (tested the floor there) without crossing:
      // this view ray passes clean through a carved-away region.
      carved = true;
      return p;
    }
    pPrev = p; hPrev = surfH; dPrev = rayDepth;
  }
#ifdef POM_JITTER
  // The jitter offset shortens the last raw step, so an exit lying in the final
  // sub-step sliver is never reached in-loop; test it exactly here.
  if (maxRayLen <= marchLen) {
    vec3 p = ro + rd * maxRayLen;
    float rayDepth = maxRayLen * NdotVraw;
    float surfH = _pomSurf(p, N, depth, t);
    if (rayDepth >= surfH) {
      return _pomRefineHit(ro, N, depth, t, pPrev, hPrev, dPrev, p, surfH, rayDepth);
    }
    carved = true;
    return p;
  }
#endif
  // Full slab traversed without crossing and the exit was never reached
  // (thick interior): Phase-1 clamp, NOT carved.
  return pPrev;
}
`
    : ''
}
${
  pomProjected
    ? /* glsl */ `
// projectedField (L1) carved depth from the hoisted UV directly (cf. _pomSurf).
float _pomSurfUv(vec2 uv, float depth, float t) {
#ifdef POM_DEBUG_COUNTERS
  _pomSampleCount++;
#endif
  return clamp(gridHeight(uv, t), 0., 0.8) * depth;
}

// s-space mirror of _pomRefineHit for the projected marcher (uv = uv0 + duv*s,
// rayDepth = s*NdotVraw). Returns the hit's ray parameter. Keep in sync with _pomRefineHit.
float _pomRefineHitProjected(vec2 uv0, vec2 duv, float NdotVraw, float depth, float t,
                             float sPrev, float hPrev, float dPrev,
                             float s, float surfH, float rayDepth) {
#ifdef POM_REFINE_BINARY
  float overshoot = rayDepth - surfH;
  float prevGap   = hPrev - dPrev;
  float span      = overshoot + prevGap;
  float w = span > 1e-6 ? overshoot / span : 0.0;
  if (prevGap <= 1e-6) {
#ifdef POM_DEBUG_COUNTERS
    _pomRefineState = 1;
#endif
    return mix(s, sPrev, w);
  }
#ifdef POM_REFINE_SKIP
  if (overshoot <= POM_REFINE_SKIP * (depth / float(POM_STEPS))) {
#ifdef POM_DEBUG_COUNTERS
    _pomRefineState = 1;
#endif
    return mix(s, sPrev, w);
  }
#endif
#ifdef POM_DEBUG_COUNTERS
  _pomRefineState = 2;
#endif
  float lo = sPrev;
  float hi = s;
  for (int bi = 0; bi < POM_BINARY_STEPS; bi++) {
    float mid = 0.5 * (lo + hi);
    float midDepth = mid * NdotVraw;
    float midSurf = _pomSurfUv(uv0 + duv * mid, depth, t);
    if (midDepth >= midSurf) hi = mid; else lo = mid;
  }
  return hi;
#else
  float a = surfH - rayDepth;
  float b = hPrev - dPrev;
  float w = clamp(a / (a - b), 0.0, 1.0);
  return mix(s, sPrev, w);
#endif
}

// Same kernel as pomMarch, with the dominant-axis projection computed once: the per-step
// world point and the branchy per-sample reprojection collapse to a 2-component MAD on uv.
vec3 pomMarchProjected(vec3 ro, vec3 rd, vec3 N, float depth, float t) {
  int axis = domAxis(N);
  vec2 uv0 = domProject(ro, axis);
  vec2 duv = domProject(rd, axis);
  float NdotVraw = dot(N, -rd);
  float NdotV = max(NdotVraw, 1e-3);
  float dStep = (depth / NdotV) / float(POM_STEPS);
  float jit = POM_JITTER_OFF;

  float sPrev = 0.0;
  float hPrev = 0.0;
  float dPrev = 0.0;
  for (int i = 1; i <= POM_STEPS; i++) {
    float s = dStep * (float(i) - jit);
    float rayDepth = s * NdotVraw;
    float surfH = _pomSurfUv(uv0 + duv * s, depth, t);
    if (rayDepth >= surfH) {
      float sHit = _pomRefineHitProjected(uv0, duv, NdotVraw, depth, t, sPrev, hPrev, dPrev, s, surfH, rayDepth);
      return ro + rd * sHit;
    }
    sPrev = s; hPrev = surfH; dPrev = rayDepth;
  }
  return ro + rd * sPrev;
}
${
  pomSafe
    ? /* glsl */ `
// safeStep refine: when the bracketing stride stayed in a constant-H (flat) region the
// secant root is exact, so bisection would only re-derive it; otherwise (a feature band the
// stride landed in) bisect for steep-wall robustness. Mirrors _pomRefineHitProjected.
float _pomRefineHitProjectedSafe(vec2 uv0, vec2 duv, float NdotVraw, float depth, float t,
                                 float sPrev, float hPrev, float dPrev,
                                 float s, float surfH, float rayDepth) {
  float overshoot = rayDepth - surfH;
  float prevGap   = hPrev - dPrev;
  float span      = overshoot + prevGap;
  float w = span > 1e-6 ? overshoot / span : 0.0;
#ifdef POM_REFINE_BINARY
  if (prevGap <= 1e-6 || abs(surfH - hPrev) <= 1e-3 * depth) {
    return mix(s, sPrev, w);
  }
  float lo = sPrev;
  float hi = s;
  float latSpeed = length(duv);
  for (int bi = 0; bi < POM_BINARY_STEPS; bi++) {
    if ((hi - lo) * latSpeed <= POM_REFINE_TOL) { break; }   // bracket localized in uv; collapses to ~0 head-on, full at grazing
    float mid = 0.5 * (lo + hi);
    float midSurf = _pomSurfUv(uv0 + duv * mid, depth, t);
    if (mid * NdotVraw >= midSurf) hi = mid; else lo = mid;
  }
  return hi;
#else
  return mix(s, sPrev, w);
#endif
}

// Bracket-safe adaptive march (cf. pomMarchProjected). Each stride is the lateral distance to
// the nearest height-varying feature (gridLateralDist), floored to POM_MIN_FEATURE so no flat
// region thinner than that is strided across — guaranteeing every wall is bracketed regardless
// of its slope. Head-on (lateral speed -> 0) collapses to ~one stride + exact secant; grazing
// tightens to feature resolution. Each stride is also floored to a per-step deadline
// (remaining length / remaining steps) so the march always reaches the slab bottom within
// POM_STEPS — at grazing, where features outnumber the budget, it degrades to fixed-step
// coverage rather than stepping out short. See the capability-ladder plan.
vec3 pomMarchProjectedSafe(vec3 ro, vec3 rd, vec3 N, float depth, float t) {
  int axis = domAxis(N);
  vec2 uv0 = domProject(ro, axis);
  vec2 duv = domProject(rd, axis);
  float NdotVraw = dot(N, -rd);
  float NdotV = max(NdotVraw, 1e-3);
  float marchLen = depth / NdotV;
  float latSpeed = max(length(duv), 1e-4);          // lateral uv advance per unit s
  float minStride = POM_MIN_FEATURE / latSpeed;

  float s = 0.0, sPrev = 0.0, hPrev = 0.0, dPrev = 0.0;
  for (int i = 0; i < POM_STEPS; i++) {
    vec2 uv = uv0 + duv * s;
    float surfH = _pomSurfUv(uv, depth, t);
    float rayDepth = s * NdotVraw;
    if (rayDepth >= surfH) {
      if (i == 0) { return ro; }                     // entry already on the surface (flat top)
      float sHit = _pomRefineHitProjectedSafe(uv0, duv, NdotVraw, depth, t, sPrev, hPrev, dPrev, s, surfH, rayDepth);
      return ro + rd * sHit;
    }
    if (s >= marchLen) { break; }
    sPrev = s; hPrev = surfH; dPrev = rayDepth;
    float cover = (marchLen - s) / float(max(POM_STEPS - 1 - i, 1));
    s = min(s + max(max(gridLateralDist(uv) / latSpeed, minStride), cover), marchLen);
  }
  return ro + rd * sPrev;                            // no crossing: clamp to deepest sample
}
${
  pomBounded
    ? /* glsl */ `
// Back-face-bounded safeStep marcher (cf. pomMarchProjectedBounded + pomMarchProjectedSafe). The
// march is bounded by the nearer of the slab bottom and the mesh back face; reaching the back-face
// bound first without a crossing means the view ray passed clean through a carved-away region.
vec3 pomMarchProjectedBoundedSafe(vec3 ro, vec3 rd, vec3 N, float depth, float t, float maxRayLen, out bool carved) {
  carved = false;
  int axis = domAxis(N);
  vec2 uv0 = domProject(ro, axis);
  vec2 duv = domProject(rd, axis);
  float NdotVraw = dot(N, -rd);
  float NdotV = max(NdotVraw, 1e-3);
  float marchLen = depth / NdotV;
  float latSpeed = max(length(duv), 1e-4);
  float minStride = POM_MIN_FEATURE / latSpeed;
  float endLen = min(marchLen, maxRayLen);

  float s = 0.0, sPrev = 0.0, hPrev = 0.0, dPrev = 0.0;
  for (int i = 0; i < POM_STEPS; i++) {
    vec2 uv = uv0 + duv * s;
    float surfH = _pomSurfUv(uv, depth, t);
    float rayDepth = s * NdotVraw;
    if (rayDepth >= surfH) {
      if (i == 0) { return ro; }                     // entry already on the surface (flat top)
      float sHit = _pomRefineHitProjectedSafe(uv0, duv, NdotVraw, depth, t, sPrev, hPrev, dPrev, s, surfH, rayDepth);
      return ro + rd * sHit;
    }
    if (s >= endLen) {
      carved = maxRayLen < marchLen;                  // cut off by the back face before the slab bottom
      return ro + rd * s;
    }
    sPrev = s; hPrev = surfH; dPrev = rayDepth;
    float cover = (endLen - s) / float(max(POM_STEPS - 1 - i, 1));
    s = min(s + max(max(gridLateralDist(uv) / latSpeed, minStride), cover), endLen);
  }
  return ro + rd * sPrev;
}
`
    : ''
}
`
    : ''
}
${
  pomBounded
    ? /* glsl */ `
// Back-face-bounded projected marcher (cf. pomMarchBounded): projection hoisted, no cell cache.
vec3 pomMarchProjectedBounded(vec3 ro, vec3 rd, vec3 N, float depth, float t, float maxRayLen, out bool carved) {
  carved = false;
  int axis = domAxis(N);
  vec2 uv0 = domProject(ro, axis);
  vec2 duv = domProject(rd, axis);
  float NdotVraw = dot(N, -rd);
  float NdotV = max(NdotVraw, 1e-3);
  float marchLen = depth / NdotV;
  float dStep = marchLen / float(POM_STEPS);
  float jit = POM_JITTER_OFF;

  float sPrev = 0.0;
  float hPrev = 0.0;
  float dPrev = 0.0;
  for (int i = 1; i <= POM_STEPS; i++) {
    float sRaw = dStep * (float(i) - jit);
    float s = min(sRaw, maxRayLen);
    float rayDepth = s * NdotVraw;
    float surfH = _pomSurfUv(uv0 + duv * s, depth, t);
    if (rayDepth >= surfH) {
      float sHit = _pomRefineHitProjected(uv0, duv, NdotVraw, depth, t, sPrev, hPrev, dPrev, s, surfH, rayDepth);
      return ro + rd * sHit;
    }
    if (sRaw >= maxRayLen) {
      carved = true;
      return ro + rd * s;
    }
    sPrev = s; hPrev = surfH; dPrev = rayDepth;
  }
#ifdef POM_JITTER
  if (maxRayLen <= marchLen) {
    float s = maxRayLen;
    float rayDepth = s * NdotVraw;
    float surfH = _pomSurfUv(uv0 + duv * s, depth, t);
    if (rayDepth >= surfH) {
      float sHit = _pomRefineHitProjected(uv0, duv, NdotVraw, depth, t, sPrev, hPrev, dPrev, s, surfH, rayDepth);
      return ro + rd * sHit;
    }
    carved = true;
    return ro + rd * s;
  }
#endif
  return ro + rd * sPrev;
}
`
    : ''
}
`
    : ''
}
${
  pomGrid
    ? /* glsl */ `
// grid (L2) carved depth; the per-cell struct is computed once per cell and threaded in (cf. _pomSurf).
float _pomSurfGrid(GridCtx ctx, ${cellType} cell, float depth) {
#ifdef POM_DEBUG_COUNTERS
  _pomSampleCount++;
#endif
  return clamp(gridHeight(ctx, cell), 0., 0.8) * depth;
}

// s-space mirror of _pomRefineHit for the grid marcher. Recomputes the cell at each bisection
// midpoint (a step can straddle a cell boundary), matching the black-box path. Keep in sync.
float _pomRefineHitGrid(vec2 uv0, vec2 duv, float NdotVraw, float depth, float t,
                        float sPrev, float hPrev, float dPrev,
                        float s, float surfH, float rayDepth) {
#ifdef POM_REFINE_BINARY
  float overshoot = rayDepth - surfH;
  float prevGap   = hPrev - dPrev;
  float span      = overshoot + prevGap;
  float w = span > 1e-6 ? overshoot / span : 0.0;
  if (prevGap <= 1e-6) {
#ifdef POM_DEBUG_COUNTERS
    _pomRefineState = 1;
#endif
    return mix(s, sPrev, w);
  }
#ifdef POM_REFINE_SKIP
  if (overshoot <= POM_REFINE_SKIP * (depth / float(POM_STEPS))) {
#ifdef POM_DEBUG_COUNTERS
    _pomRefineState = 1;
#endif
    return mix(s, sPrev, w);
  }
#endif
#ifdef POM_DEBUG_COUNTERS
  _pomRefineState = 2;
#endif
  float lo = sPrev;
  float hi = s;
  for (int bi = 0; bi < POM_BINARY_STEPS; bi++) {
    float mid = 0.5 * (lo + hi);
    vec2 uv = uv0 + duv * mid;
    vec2 cellId = floor(uv / GRID_PITCH);
    GridCtx ctx = GridCtx((fract(uv / GRID_PITCH) - 0.5) * GRID_PITCH, cellId, t);
    float midSurf = _pomSurfGrid(ctx, gridComputeCell(cellId), depth);
    if (mid * NdotVraw >= midSurf) hi = mid; else lo = mid;
  }
  return hi;
#else
  float a = surfH - rayDepth;
  float b = hPrev - dPrev;
  float w = clamp(a / (a - b), 0.0, 1.0);
  return mix(s, sPrev, w);
#endif
}

// Same kernel as pomMarchProjected, plus engine-owned cell decomposition: a one-cell cache
// recomputes \`gridComputeCell\` only when the marched sample crosses into a new cell (so a
// head-on ray that stays in one cell pays a single cell eval for the whole march).
vec3 pomMarchGrid(vec3 ro, vec3 rd, vec3 N, float depth, float t) {
  int axis = domAxis(N);
  vec2 uv0 = domProject(ro, axis);
  vec2 duv = domProject(rd, axis);
  float NdotVraw = dot(N, -rd);
  float NdotV = max(NdotVraw, 1e-3);
  float dStep = (depth / NdotV) / float(POM_STEPS);
  float jit = POM_JITTER_OFF;

  vec2 cachedId = floor(uv0 / GRID_PITCH);
  ${cellType} cell = gridComputeCell(cachedId);

  float sPrev = 0.0;
  float hPrev = 0.0;
  float dPrev = 0.0;
  for (int i = 1; i <= POM_STEPS; i++) {
    float s = dStep * (float(i) - jit);
    vec2 uv = uv0 + duv * s;
    vec2 cellId = floor(uv / GRID_PITCH);
    if (any(notEqual(cellId, cachedId))) {
      cell = gridComputeCell(cellId);
      cachedId = cellId;
    }
    GridCtx ctx = GridCtx((fract(uv / GRID_PITCH) - 0.5) * GRID_PITCH, cellId, t);
    float rayDepth = s * NdotVraw;
    float surfH = _pomSurfGrid(ctx, cell, depth);
    if (rayDepth >= surfH) {
      float sHit = _pomRefineHitGrid(uv0, duv, NdotVraw, depth, t, sPrev, hPrev, dPrev, s, surfH, rayDepth);
      return ro + rd * sHit;
    }
    sPrev = s; hPrev = surfH; dPrev = rayDepth;
  }
  return ro + rd * sPrev;
}
${
  pomSafe
    ? /* glsl */ `
// grid-tier safeStep refine (cf. _pomRefineHitProjectedSafe + _pomRefineHitGrid): secant when the
// bracket stayed in a flat (constant-cell-carve) region, else cell-aware bisect (recompute the cell
// at each midpoint, since a stride can straddle a cell boundary).
float _pomRefineHitGridSafe(vec2 uv0, vec2 duv, float NdotVraw, float depth, float t,
                            float sPrev, float hPrev, float dPrev,
                            float s, float surfH, float rayDepth) {
  float overshoot = rayDepth - surfH;
  float prevGap   = hPrev - dPrev;
  float span      = overshoot + prevGap;
  float w = span > 1e-6 ? overshoot / span : 0.0;
#ifdef POM_REFINE_BINARY
  if (prevGap <= 1e-6 || abs(surfH - hPrev) <= 1e-3 * depth) {
    return mix(s, sPrev, w);
  }
  float lo = sPrev;
  float hi = s;
  float latSpeed = length(duv);
  for (int bi = 0; bi < POM_BINARY_STEPS; bi++) {
    if ((hi - lo) * latSpeed <= POM_REFINE_TOL) { break; }   // bracket localized in uv; collapses to ~0 head-on, full at grazing
    float mid = 0.5 * (lo + hi);
    vec2 uv = uv0 + duv * mid;
    vec2 cellId = floor(uv / GRID_PITCH);
    GridCtx ctx = GridCtx((fract(uv / GRID_PITCH) - 0.5) * GRID_PITCH, cellId, t);
    float midSurf = _pomSurfGrid(ctx, gridComputeCell(cellId), depth);
    if (mid * NdotVraw >= midSurf) hi = mid; else lo = mid;
  }
  return hi;
#else
  return mix(s, sPrev, w);
#endif
}

// Bracket-safe adaptive grid march (cf. pomMarchProjectedSafe + pomMarchGrid): adaptive stride
// (gridLateralDist floored to POM_MIN_FEATURE) plus the engine-owned one-cell cache.
vec3 pomMarchGridSafe(vec3 ro, vec3 rd, vec3 N, float depth, float t) {
  int axis = domAxis(N);
  vec2 uv0 = domProject(ro, axis);
  vec2 duv = domProject(rd, axis);
  float NdotVraw = dot(N, -rd);
  float NdotV = max(NdotVraw, 1e-3);
  float marchLen = depth / NdotV;
  float latSpeed = max(length(duv), 1e-4);
  float minStride = POM_MIN_FEATURE / latSpeed;

  vec2 cachedId = floor(uv0 / GRID_PITCH);
  ${cellType} cell = gridComputeCell(cachedId);

  float s = 0.0, sPrev = 0.0, hPrev = 0.0, dPrev = 0.0;
  for (int i = 0; i < POM_STEPS; i++) {
    vec2 uv = uv0 + duv * s;
    vec2 cellId = floor(uv / GRID_PITCH);
    if (any(notEqual(cellId, cachedId))) {
      cell = gridComputeCell(cellId);
      cachedId = cellId;
    }
    GridCtx ctx = GridCtx((fract(uv / GRID_PITCH) - 0.5) * GRID_PITCH, cellId, t);
    float surfH = _pomSurfGrid(ctx, cell, depth);
    float rayDepth = s * NdotVraw;
    if (rayDepth >= surfH) {
      if (i == 0) { return ro; }
      float sHit = _pomRefineHitGridSafe(uv0, duv, NdotVraw, depth, t, sPrev, hPrev, dPrev, s, surfH, rayDepth);
      return ro + rd * sHit;
    }
    if (s >= marchLen) { break; }
    sPrev = s; hPrev = surfH; dPrev = rayDepth;
    float cover = (marchLen - s) / float(max(POM_STEPS - 1 - i, 1));
    s = min(s + max(max(gridLateralDist(uv) / latSpeed, minStride), cover), marchLen);
  }
  return ro + rd * sPrev;
}
${
  pomBounded
    ? /* glsl */ `
// Back-face-bounded grid safeStep marcher (cf. pomMarchProjectedBoundedSafe + pomMarchGridSafe).
vec3 pomMarchGridBoundedSafe(vec3 ro, vec3 rd, vec3 N, float depth, float t, float maxRayLen, out bool carved) {
  carved = false;
  int axis = domAxis(N);
  vec2 uv0 = domProject(ro, axis);
  vec2 duv = domProject(rd, axis);
  float NdotVraw = dot(N, -rd);
  float NdotV = max(NdotVraw, 1e-3);
  float marchLen = depth / NdotV;
  float latSpeed = max(length(duv), 1e-4);
  float minStride = POM_MIN_FEATURE / latSpeed;
  float endLen = min(marchLen, maxRayLen);

  vec2 cachedId = floor(uv0 / GRID_PITCH);
  ${cellType} cell = gridComputeCell(cachedId);

  float s = 0.0, sPrev = 0.0, hPrev = 0.0, dPrev = 0.0;
  for (int i = 0; i < POM_STEPS; i++) {
    vec2 uv = uv0 + duv * s;
    vec2 cellId = floor(uv / GRID_PITCH);
    if (any(notEqual(cellId, cachedId))) {
      cell = gridComputeCell(cellId);
      cachedId = cellId;
    }
    GridCtx ctx = GridCtx((fract(uv / GRID_PITCH) - 0.5) * GRID_PITCH, cellId, t);
    float surfH = _pomSurfGrid(ctx, cell, depth);
    float rayDepth = s * NdotVraw;
    if (rayDepth >= surfH) {
      if (i == 0) { return ro; }
      float sHit = _pomRefineHitGridSafe(uv0, duv, NdotVraw, depth, t, sPrev, hPrev, dPrev, s, surfH, rayDepth);
      return ro + rd * sHit;
    }
    if (s >= endLen) {
      carved = maxRayLen < marchLen;
      return ro + rd * s;
    }
    sPrev = s; hPrev = surfH; dPrev = rayDepth;
    float cover = (endLen - s) / float(max(POM_STEPS - 1 - i, 1));
    s = min(s + max(max(gridLateralDist(uv) / latSpeed, minStride), cover), endLen);
  }
  return ro + rd * sPrev;
}
`
    : ''
}
`
    : ''
}
${
  pomBounded
    ? /* glsl */ `
// Back-face-bounded grid marcher (cf. pomMarchBounded), with the engine-owned cell cache.
vec3 pomMarchGridBounded(vec3 ro, vec3 rd, vec3 N, float depth, float t, float maxRayLen, out bool carved) {
  carved = false;
  int axis = domAxis(N);
  vec2 uv0 = domProject(ro, axis);
  vec2 duv = domProject(rd, axis);
  float NdotVraw = dot(N, -rd);
  float NdotV = max(NdotVraw, 1e-3);
  float marchLen = depth / NdotV;
  float dStep = marchLen / float(POM_STEPS);
  float jit = POM_JITTER_OFF;

  vec2 cachedId = floor(uv0 / GRID_PITCH);
  ${cellType} cell = gridComputeCell(cachedId);

  float sPrev = 0.0;
  float hPrev = 0.0;
  float dPrev = 0.0;
  for (int i = 1; i <= POM_STEPS; i++) {
    float sRaw = dStep * (float(i) - jit);
    float s = min(sRaw, maxRayLen);
    vec2 uv = uv0 + duv * s;
    vec2 cellId = floor(uv / GRID_PITCH);
    if (any(notEqual(cellId, cachedId))) {
      cell = gridComputeCell(cellId);
      cachedId = cellId;
    }
    GridCtx ctx = GridCtx((fract(uv / GRID_PITCH) - 0.5) * GRID_PITCH, cellId, t);
    float rayDepth = s * NdotVraw;
    float surfH = _pomSurfGrid(ctx, cell, depth);
    if (rayDepth >= surfH) {
      float sHit = _pomRefineHitGrid(uv0, duv, NdotVraw, depth, t, sPrev, hPrev, dPrev, s, surfH, rayDepth);
      return ro + rd * sHit;
    }
    if (sRaw >= maxRayLen) {
      carved = true;
      return ro + rd * s;
    }
    sPrev = s; hPrev = surfH; dPrev = rayDepth;
  }
#ifdef POM_JITTER
  if (maxRayLen <= marchLen) {
    float s = maxRayLen;
    vec2 uv = uv0 + duv * s;
    vec2 cellId = floor(uv / GRID_PITCH);
    if (any(notEqual(cellId, cachedId))) {
      cell = gridComputeCell(cellId);
      cachedId = cellId;
    }
    GridCtx ctx = GridCtx((fract(uv / GRID_PITCH) - 0.5) * GRID_PITCH, cellId, t);
    float rayDepth = s * NdotVraw;
    float surfH = _pomSurfGrid(ctx, cell, depth);
    if (rayDepth >= surfH) {
      float sHit = _pomRefineHitGrid(uv0, duv, NdotVraw, depth, t, sPrev, hPrev, dPrev, s, surfH, rayDepth);
      return ro + rd * sHit;
    }
    carved = true;
    return ro + rd * s;
  }
#endif
  return ro + rd * sPrev;
}
`
    : ''
}
`
    : ''
}
// finite-difference normal of the carved floor (\`eps\` in world space). 3-tap
// forward differences rather than 4-tap central: one fewer \`_pomSurf\` eval, and
// the bias is imperceptible on this relief.
vec3 pomAnalyticNormal(vec3 pHit, vec3 N, float depth, float t, float eps, float aa) {
#ifdef POM_HAS_NORMAL_SHADER
  // Closed-form floor normal from the material; skips the taps below. \`aa\` is
  // the anisotropic pixel footprint, for the material's analytic relief AA.
  return getPomNormal(pHit, N, depth, t, aa);
#else
  vec3 up = abs(N.y) < 0.99 ? vec3(0., 1., 0.) : vec3(1., 0., 0.);
  vec3 T = normalize(cross(N, up));
  vec3 B = cross(N, T);
  float h0 = _pomSurf(pHit,           N, depth, t);
  float hR = _pomSurf(pHit + T * eps, N, depth, t);
  float hU = _pomSurf(pHit + B * eps, N, depth, t);
  // Surface point = base + uT + vB - h*N  =>  outward normal ∝ N + h_u T + h_v B.
  // Forward differences over distance \`eps\`, so N is scaled by eps (not 2*eps).
  vec3 grad = T * (hR - h0) + B * (hU - h0);
  return normalize(N * eps + grad);
#endif
}

// 1 = full POM, 0 = flat base surface. Smoothly retracts grooves with
// distance so high-frequency detail does not sub-pixel alias.
float pomLodFade(float distanceToCamera) {
  return 1.0 - smoothstep(${lodFadeStart.toFixed(3)}, ${lodFadeEnd.toFixed(3)}, distanceToCamera);
}
${
  pomSelfShadow
    ? /* glsl */ `
#define POM_SHADOW_STEPS ${pomSelfShadow.steps}
#define POM_SHADOW_SOFTNESS ${pomSelfShadow.softness.toFixed(4)}

// Relief self-shadowing: march from the displaced hit toward \`L\`; occlusion in
// [0,1] (0 = lit). \`N\`/\`ro\` are the base surface normal/point. Soft, contact-
// hardened (cf. Policarpo & Oliveira relief shadows, soft variant).
float pomSelfShadow(vec3 hit, vec3 ro, vec3 N, vec3 L, float depth, float t) {
  float NdotL = dot(N, L);
  if (NdotL <= 1e-3) { return 1.0; }              // light at/below the surface plane
  float h0 = -dot(hit - ro, N);                    // start depth below base
  if (h0 <= depth * 0.03) { return 0.0; }          // on the top surface: nothing casts onto it
  float marchLen = depth / NdotL;
  float stepLen = marchLen / float(POM_SHADOW_STEPS);
  float bias = depth * 0.02;
  float jit = POM_JITTER_OFF;
  float occ = 0.0;
  for (int i = 1; i <= POM_SHADOW_STEPS; i++) {
    float s = stepLen * (float(i) - jit);
    vec3 p = hit + L * s;
    float rayH = -dot(p - ro, N);                  // ray depth below base (shrinks as it rises)
    if (rayH <= 0.0) { break; }                     // cleared the base plane: rest of the ray is in open air
    float surfH = _pomSurf(p, N, depth, t);        // local carved-surface depth below base
    float pen = rayH - surfH - bias;               // >0 => ray buried inside the wall
    if (pen > 0.0) {
      float prox = 1.0 - s / marchLen;             // 1 at the hit -> 0 a full march away
      occ = max(occ, clamp(pen / (depth * POM_SHADOW_SOFTNESS), 0.0, 1.0) * prox);
    }
  }
  return occ;
}
`
    : ''
}
`;
};

export const buildPomMainBlock = (
  pomBounded: boolean,
  pomProjected: boolean,
  pomGrid: boolean,
  pomSafe: boolean,
  pomTexturing: PomTexturing,
  normalEps: number | undefined,
  pomSelfShadow: { strength: number } | null,
  hasHeightMap: boolean,
  // `hitType` tier: a statement evaluating the shared at-hit cell field into `_pomHitData`. Runs
  // once on the final `_pomHit` (the else covers the LOD-faded path where the march is skipped).
  hitFramePrep: string | null
): string => {
  const tail = (() => {
    switch (pomTexturing) {
      case 'triplanar':
        return `vec3 triplanarSamplePos = vTriplanarPos + (_pomHit - vWorldPos);`;
      case 'generated':
        // Axis pick uses the base normal, not the POM-perturbed one: the
        // perturbed normal causes dominant-axis flips at bevels near a 45°
        // boundary, producing UV seams. Tradeoff: stretching on steep walls.
        return `vec2 _pomGenUv = ( uvTransform * vec3( generateUV(_pomHit, vWorldNormal), 1.0 ) ).xy;`;
      case 'tangent':
        // Project the displaced hit into the mesh tangent frame (see `pomMeshUv`).
        return `vec2 _pomGenUv = pomMeshUv(_pomHit);`;
      case 'baseline':
        // Reuse the interpolated pre-POM UV; texture warps with parallax.
        return '';
    }
  })();

  return /* glsl */ `
  // --- Procedural Parallax Occlusion Mapping (phase 1: heightfield, no discard) ---
  // Raymarch the slab; expose displaced world hit + analytic floor normal.
  vec3 _pomHit = vWorldPos;
  vec3 _pomNormalW = normalize(vWorldNormal);
  ${pomTexturing === 'triplanar' && hasHeightMap ? '_pomTriW = generateTriplanarWeights(_pomNormalW);' : ''}
  ${pomSelfShadow ? 'float _pomShadowVis = 1.0;' : ''}
  {
    vec3 _pomRd = normalize(vWorldPos - cameraPosition);
    float _pomFade = pomLodFade(distanceToCamera);
    if (_pomFade > 0.001) {
      float _pomD = pomDepth * _pomFade;
      ${
        pomBounded
          ? /* glsl */ `// Convex back-face-bounded silhouette path. UV normalized by the
      // drawing-buffer size (the exit RT may be lower-res than the framebuffer).
      vec2 _pomUv = gl_FragCoord.xy / pomResolution;
      float _pomExitDist = texture(pomBackDepth, _pomUv).r;
      float _pomChord = _pomExitDist - distanceToCamera;
      // No valid bound: empty texel; chord<=0 means a nearer POM mesh stole
      // this texel (combined-buffer aliasing); or we're a back face (camera
      // inside the hull during occlusion-xray DoubleSide) where exit and
      // front converge and the chord is ULP noise — fall back to Phase 1.
      bool _pomNoBound = (_pomExitDist <= 0.0) || (_pomChord <= 0.0) || !gl_FrontFacing;
      // A positive but sub-epsilon chord is ill-conditioned (differencing two
      // near-equal large distances) -> kept/discarded flickers (edge shimmer).
      // Commit to carved up front, skipping the doomed march; valid-bound path only.
      if (!_pomNoBound && _pomChord < max(2e-3, _pomD * 0.05)) {
        discard;
      }
      float _pomMaxLen = _pomNoBound ? 1e9 : _pomChord;
      bool _pomCarved = false;
      _pomHit = ${pomGrid ? (pomSafe ? 'pomMarchGridBoundedSafe' : 'pomMarchGridBounded') : pomProjected ? (pomSafe ? 'pomMarchProjectedBoundedSafe' : 'pomMarchProjectedBounded') : 'pomMarchBounded'}(vWorldPos, _pomRd, _pomNormalW, _pomD, curTimeSeconds, _pomMaxLen, _pomCarved);
      if (_pomCarved) {
        discard;
      }`
          : pomGrid
            ? /* glsl */ `_pomHit = ${pomSafe ? 'pomMarchGridSafe' : 'pomMarchGrid'}(vWorldPos, _pomRd, _pomNormalW, _pomD, curTimeSeconds);`
            : pomProjected
              ? /* glsl */ `_pomHit = ${pomSafe ? 'pomMarchProjectedSafe' : 'pomMarchProjected'}(vWorldPos, _pomRd, _pomNormalW, _pomD, curTimeSeconds);`
              : /* glsl */ `_pomHit = pomMarch(vWorldPos, _pomRd, _pomNormalW, _pomD, curTimeSeconds);`
      }
      ${hitFramePrep ?? ''}
      // Skip the analytic-normal taps once the LOD fade is negligible (<=2%):
      // saves the _pomSurf evals on distant fragments.
      if (_pomFade > 0.02) {
        float _pomEps = max(unitsPerPx, _pomD * 0.02);
        ${typeof normalEps === 'number' ? /* glsl */ `_pomEps = max(_pomEps, ${normalEps.toFixed(6)});` : ''}
        // Anisotropic footprint (one pixel stretched by 1/NdotV toward grazing,
        // clamped to ~6.7x) for the material's analytic relief AA.
        float _pomAA = unitsPerPx / max(abs(dot(_pomNormalW, _pomRd)), 0.15);
        _pomNormalW = mix(
          _pomNormalW,
          pomAnalyticNormal(_pomHit, _pomNormalW, _pomD, curTimeSeconds, _pomEps, _pomAA),
          _pomFade
        );
      }
      ${
        pomSelfShadow
          ? /* glsl */ `_pomShadowVis = 1.0 - pomSelfShadow(_pomHit, vWorldPos, normalize(vWorldNormal), normalize(pomShadowLightDir), _pomD, curTimeSeconds) * ${pomSelfShadow.strength.toFixed(4)} * _pomFade;`
          : ''
      }
    }${hitFramePrep ? ` else { ${hitFramePrep} }` : ''}
  }
  ${tail}
  `;
};

// Applies the relief self-shadow term to the direct lighting only (ambient /
// indirect stay lit). Emitted after `<lights_fragment_end>`.
export const buildPomSelfShadowApply = (): string => /* glsl */ `
  reflectedLight.directDiffuse *= _pomShadowVis;
  reflectedLight.directSpecular *= _pomShadowVis;`;

// POM owns the shading normal. Without a normal map the analytic floor normal
// is used directly; with one, its tangent-space detail is added to the floor
// normal (UDN-style: add the world-space perturbation, then normalize) so the
// map layers onto the carved relief instead of replacing it.
export const buildPomNormalApply = (
  pomTexturing: PomTexturing,
  hasNormalMap: boolean,
  applyReliefNormal: boolean
): string => {
  if (!hasNormalMap) {
    return applyReliefNormal
      ? /* glsl */ `normal = normalize((viewMatrix * vec4(_pomNormalW, 0.)).xyz);`
      : '';
  }

  if (pomTexturing === 'triplanar') {
    // Reuse the analytic per-axis triplanar frame, added onto the floor normal
    // instead of the geometric normal.
    return /* glsl */ `
  vec3 _pomNormalDetailW = normalize(
    triplanarNormalMapPerturbation(normalMap, triplanarSamplePos, vec2(uvTransform[0][0], uvTransform[1][1]), _pomNormalW, normalScale, normalMapMeanColor)
    + _pomNormalW
  );
  normal = normalize((viewMatrix * vec4(_pomNormalDetailW, 0.)).xyz);
  `;
  }

  // Single-axis swizzle; axis picked from base normal to match `_pomGenUv`
  // and avoid the perturbed-normal axis-flip seam.
  return /* glsl */ `
  {
    vec3 _pgN = vWorldNormal;
    vec3 _pgA = abs(_pgN);
    vec2 _pgT = (texture2D(normalMap, _pomGenUv).xy * 2.0 - 1.0) * normalScale;
    vec3 _pgP;
    if (_pgA.x >= _pgA.y && _pgA.x >= _pgA.z) {
      _pgT.x *= sign(_pgN.x);
      _pgP = vec3(0.0, _pgT.y, _pgT.x);
    } else if (_pgA.y >= _pgA.z) {
      _pgT.x *= sign(_pgN.y);
      _pgP = vec3(_pgT.x, 0.0, _pgT.y);
    } else {
      _pgT.x *= sign(_pgN.z);
      _pgP = vec3(_pgT.x, _pgT.y, 0.0);
    }
    vec3 _pomNormalDetailW = normalize(_pgP + _pomNormalW);
    normal = normalize((viewMatrix * vec4(_pomNormalDetailW, 0.)).xyz);
  }
  `;
};

export type PomDebugMode =
  | 'heightmap'
  | 'depth'
  | 'normal'
  | 'normalDelta'
  | 'axis'
  | 'hit'
  | 'samples'
  | 'evals'
  | 'skip';

// Overrides `outFragColor` at the end of `main()` with a diagnostic, bypassing
// the color shader, normal map, and fog (tonemapping still applies downstream).
export const buildPomDebug = (debug: PomDebugMode | undefined): string => {
  if (!debug) return '';
  if (debug === 'axis') {
    return /* glsl */ `
  {
    vec3 _dbgN = abs(_pomNormalW);
    vec3 _dbgAxis = (_dbgN.x >= _dbgN.y && _dbgN.x >= _dbgN.z) ? vec3(1.0, 0.0, 0.0)
                  : (_dbgN.y >= _dbgN.z)                       ? vec3(0.0, 1.0, 0.0)
                  :                                              vec3(0.0, 0.0, 1.0);
    outFragColor = vec4(_dbgAxis, 1.0);
  }`;
  }
  if (debug === 'skip') {
    // Refinement decision per fragment (binary refinement only):
    //   green     = bisection skipped, the linear/secant hit was accepted
    //   red       = full bisection ran (deep plunge / steep wall)
    //   dark blue = no refinement reached (no crossing, or POM faded to flat)
    return /* glsl */ `
  {
    vec3 _dbgSkip = _pomRefineState == 1 ? vec3(0.0, 1.0, 0.0)
                  : _pomRefineState == 2 ? vec3(1.0, 0.0, 0.0)
                  :                        vec3(0.0, 0.0, 0.3);
    outFragColor = vec4(_dbgSkip, 1.0);
  }`;
  }
  if (debug === 'samples') {
    // Total height-field evals this fragment (linear march + binary refine +
    // analytic-normal taps), heat-mapped against the per-fragment worst case.
    return /* glsl */ `
  {
    float _dbgT = clamp(float(_pomSampleCount) / float(POM_DEBUG_MAX_SAMPLES), 0.0, 1.0);
    outFragColor = vec4(_pomHeat(_dbgT), 1.0);
  }`;
  }
  if (debug === 'evals') {
    // Same count as 'samples' but written linearly (grayscale = evals/worst-case)
    // so a framebuffer readback recovers the eval fraction without inverting _pomHeat.
    return /* glsl */ `
  {
    float _dbgT = clamp(float(_pomSampleCount) / float(POM_DEBUG_MAX_SAMPLES), 0.0, 1.0);
    outFragColor = vec4(vec3(_dbgT), 1.0);
  }`;
  }
  const expr: Record<Exclude<PomDebugMode, 'axis' | 'skip' | 'samples' | 'evals'>, string> = {
    heightmap: 'vec3(samplePomHeightMap(_pomHit, vWorldNormal))',
    depth: 'vec3(_pomSurf(_pomHit, vWorldNormal, 1.0, curTimeSeconds))',
    normal: '_pomNormalW * 0.5 + 0.5',
    normalDelta: 'vec3(length(_pomNormalW - normalize(vWorldNormal)), 0.0, 0.0)',
    hit: 'fract(_pomHit)',
  };
  return `\n  outFragColor = vec4(${expr[debug]}, 1.0);`;
};
