export const POM_BOUNDED_SILHOUETTE_FLAG = 'pomBoundedSilhouette';

export const buildPomUniformDecls = (pom: boolean, pomBounded: boolean, pomHeightMap: boolean): string =>
  [
    pom ? 'uniform float pomDepth;' : '',
    pomBounded ? 'uniform highp sampler2D pomBackDepth; // R = euclidean dist camera->nearest back face' : '',
    pomBounded ? 'uniform vec2 pomResolution; // drawing-buffer size; the back-face RT may be lower-res' : '',
    pomHeightMap ? 'uniform sampler2D pomHeightMap;' : '',
  ].join('\n');

export type PomTexturing = 'triplanar' | 'generated' | 'baseline';

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
      return /* glsl */ `
float samplePomHeightMap(vec3 p, vec3 N) {
  vec3 sp = vTriplanarPos + (p - vWorldPos);
  vec2 _phUvScale = vec2(uvTransform[0][0], uvTransform[1][1]);
  vec3 w = generateTriplanarWeights(N);
  float h = 0.;
  if (w.x > 0.01) h += textureLod(pomHeightMap, sp.yz * _phUvScale, 0.0).r * w.x;
  if (w.y > 0.01) h += textureLod(pomHeightMap, sp.zx * _phUvScale, 0.0).r * w.y;
  if (w.z > 0.01) h += textureLod(pomHeightMap, sp.xy * _phUvScale, 0.0).r * w.z;
  return 1. - h;
}`;
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
  lodFadeStart: number;
  lodFadeEnd: number;
  pomRefinement: PomRefinement;
  pomBinarySteps: number;
  // Skip binary refinement when the linear hit already pierced the floor by less
  // than this fraction of one step (in depth). 0 disables the skip.
  pomRefineSkip: number;
  // Material supplies `getPomNormal(...)`; use it instead of finite differences.
  pomHasNormalShader: boolean;
  // Active debug view, if any; the `samples`/`skip` views enable per-fragment
  // instrumentation (compiled out otherwise).
  pomDebug?: PomDebugMode;
}): string => {
  const {
    pomSteps,
    pomBounded,
    lodFadeStart,
    lodFadeEnd,
    pomRefinement,
    pomBinarySteps,
    pomRefineSkip,
    pomHasNormalShader,
    pomDebug,
  } = opts;
  const useBinary = pomRefinement === 'binary';
  const debugCounters = pomDebug === 'samples' || pomDebug === 'skip';
  // Worst-case _pomSurf evals per fragment: linear march + binary refine (secant
  // adds none) + the 3 finite-difference normal taps (none with a normal shader).
  const maxSamples = pomSteps + (useBinary ? pomBinarySteps : 0) + (pomHasNormalShader ? 0 : 3);
  return /* glsl */ `
#define POM_STEPS ${pomSteps}
${useBinary ? `#define POM_REFINE_BINARY\n#define POM_BINARY_STEPS ${pomBinarySteps}` : ''}
${useBinary && pomRefineSkip > 0 ? `#define POM_REFINE_SKIP ${pomRefineSkip.toFixed(4)}` : ''}
${debugCounters ? `#define POM_DEBUG_COUNTERS\n#define POM_DEBUG_MAX_SAMPLES ${maxSamples}` : ''}
${pomHasNormalShader ? '#define POM_HAS_NORMAL_SHADER' : ''}

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
  float NdotV = max(dot(N, -rd), 1e-3);          // grazing-angle clamp
  float marchLen = depth / NdotV;
  float dStep = marchLen / float(POM_STEPS);

  vec3 pPrev = ro;
  float hPrev = 0.0;   // carved depth at previous sample
  float dPrev = 0.0;   // ray depth-below-base at previous sample

  for (int i = 1; i <= POM_STEPS; i++) {
    vec3 p = ro + rd * (dStep * float(i));
    float rayDepth = -dot(p - ro, N);
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
  float NdotV = max(dot(N, -rd), 1e-3);
  float marchLen = depth / NdotV;
  float dStep = marchLen / float(POM_STEPS);

  vec3 pPrev = ro;
  float hPrev = 0.0;
  float dPrev = 0.0;

  for (int i = 1; i <= POM_STEPS; i++) {
    float sRaw = dStep * float(i);
    // Clamp the final tested point to the exact convex exit so the floor is
    // still evaluated there before declaring the fragment carved away.
    float s = min(sRaw, maxRayLen);
    vec3 p = ro + rd * s;
    float rayDepth = -dot(p - ro, N);
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
  // Full slab traversed without crossing and the exit was never reached
  // (thick interior): Phase-1 clamp, NOT carved.
  return pPrev;
}
`
    : ''
}
// finite-difference normal of the carved floor (\`eps\` in world space). 3-tap
// forward differences rather than 4-tap central: one fewer \`_pomSurf\` eval, and
// the bias is imperceptible on this relief.
vec3 pomAnalyticNormal(vec3 pHit, vec3 N, float depth, float t, float eps) {
#ifdef POM_HAS_NORMAL_SHADER
  // Closed-form floor normal from the material; skips the taps below.
  return getPomNormal(pHit, N, depth, t);
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
`;
};

export const buildPomMainBlock = (
  pomBounded: boolean,
  pomTexturing: PomTexturing,
  normalEps: number | undefined
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
      float _pomChord = _pomExitDist - distance(vWorldPos, cameraPosition);
      // No valid bound: empty texel; chord<=0 means a nearer POM mesh stole
      // this texel (combined-buffer aliasing); or we're a back face (camera
      // inside the hull during occlusion-xray DoubleSide) where exit and
      // front converge and the chord is ULP noise — fall back to Phase 1.
      bool _pomNoBound = (_pomExitDist <= 0.0) || (_pomChord <= 0.0) || !gl_FrontFacing;
      float _pomMaxLen = _pomNoBound ? 1e9 : _pomChord;
      bool _pomCarved = false;
      _pomHit = pomMarchBounded(vWorldPos, _pomRd, _pomNormalW, _pomD, curTimeSeconds, _pomMaxLen, _pomCarved);
      // A positive but sub-epsilon chord is ill-conditioned (differencing two
      // near-equal large distances) -> kept/discarded flickers (edge shimmer).
      // Commit it to carved for a stable edge; valid-bound path only.
      if (!_pomNoBound && _pomChord < max(2e-3, _pomD * 0.05)) {
        _pomCarved = true;
      }
      if (_pomCarved) {
        discard;
      }`
          : /* glsl */ `_pomHit = pomMarch(vWorldPos, _pomRd, _pomNormalW, _pomD, curTimeSeconds);`
      }
      // Skip the analytic-normal taps once the LOD fade is negligible (<=2%):
      // saves the _pomSurf evals on distant fragments.
      if (_pomFade > 0.02) {
        float _pomEps = max(unitsPerPx, _pomD * 0.02);
        ${typeof normalEps === 'number' ? /* glsl */ `_pomEps = max(_pomEps, ${normalEps.toFixed(6)});` : ''}
        _pomNormalW = mix(
          _pomNormalW,
          pomAnalyticNormal(_pomHit, _pomNormalW, _pomD, curTimeSeconds, _pomEps),
          _pomFade
        );
      }
    }
  }
  ${tail}
  `;
};

// POM owns the shading normal. Without a normal map the analytic floor normal
// is used directly; with one, its tangent-space detail is added to the floor
// normal (UDN-style: add the world-space perturbation, then normalize) so the
// map layers onto the carved relief instead of replacing it.
export const buildPomNormalApply = (pomTexturing: PomTexturing, hasNormalMap: boolean): string => {
  if (!hasNormalMap) {
    return '';
    return `normal = normalize((viewMatrix * vec4(_pomNormalW, 0.)).xyz);`;
  }

  if (pomTexturing === 'triplanar') {
    // Reuse the analytic per-axis triplanar frame, added onto the floor normal
    // instead of the geometric normal.
    return /* glsl */ `
  vec3 _pomNormalDetailW = normalize(
    triplanarNormalMapPerturbation(normalMap, triplanarSamplePos, vec2(uvTransform[0][0], uvTransform[1][1]), _pomNormalW, normalScale)
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
  const expr: Record<Exclude<PomDebugMode, 'axis' | 'skip' | 'samples'>, string> = {
    heightmap: 'vec3(samplePomHeightMap(_pomHit, vWorldNormal))',
    depth: 'vec3(_pomSurf(_pomHit, vWorldNormal, 1.0, curTimeSeconds))',
    normal: '_pomNormalW * 0.5 + 0.5',
    normalDelta: 'vec3(length(_pomNormalW - normalize(vWorldNormal)), 0.0, 0.0)',
    hit: 'fract(_pomHit)',
  };
  return `\n  outFragColor = vec4(${expr[debug]}, 1.0);`;
};
