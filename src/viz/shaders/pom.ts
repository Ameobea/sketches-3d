export const POM_BOUNDED_SILHOUETTE_FLAG = 'pomBoundedSilhouette';

export const buildPomUniformDecls = (pom: boolean, pomBounded: boolean): string =>
  [
    pom ? 'uniform float pomDepth;' : '',
    pomBounded ? 'uniform highp sampler2D pomBackDepth; // R = euclidean dist camera->nearest back face' : '',
    pomBounded ? 'uniform vec2 pomResolution; // drawing-buffer size; the back-face RT may be lower-res' : '',
  ].join('\n');

export const buildPomDefs = (opts: {
  pomSteps: number;
  pomBounded: boolean;
  lodFadeStart: number;
  lodFadeEnd: number;
}): string => {
  const { pomSteps, pomBounded, lodFadeStart, lodFadeEnd } = opts;
  return /* glsl */ `
#define POM_STEPS ${pomSteps}

// --- Procedural Parallax Occlusion Mapping (world space, subtractive only) ---
// The base polygon is the TOP of a virtual slab of thickness \`pomDepth\`
// extending inward along -N. \`getPomHeight()\` returns carved depth in [0,1];
// this wrapper scales it to world units.
float _pomSurf(vec3 p, vec3 N, float depth, float t) {
  return getPomHeight(p, N, t) * depth;
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
      float a = surfH - rayDepth;
      float b = hPrev - dPrev;
      float w = clamp(a / (a - b), 0.0, 1.0);   // secant root between samples
      return mix(p, pPrev, w);
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
      float a = surfH - rayDepth;
      float b = hPrev - dPrev;
      float w = clamp(a / (a - b), 0.0, 1.0);
      return mix(p, pPrev, w);
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
// finite-difference normal of the carved floor.  \`eps\` is in world space.
vec3 pomAnalyticNormal(vec3 pHit, vec3 N, float depth, float t, float eps) {
  vec3 up = abs(N.y) < 0.99 ? vec3(0., 1., 0.) : vec3(1., 0., 0.);
  vec3 T = normalize(cross(N, up));
  vec3 B = cross(N, T);
  float hL = _pomSurf(pHit - T * eps, N, depth, t);
  float hR = _pomSurf(pHit + T * eps, N, depth, t);
  float hD = _pomSurf(pHit - B * eps, N, depth, t);
  float hU = _pomSurf(pHit + B * eps, N, depth, t);
  // Surface point = base + uT + vB - h*N  =>  outward normal ∝ N + h_u T + h_v B
  vec3 grad = T * (hR - hL) + B * (hU - hD);
  return normalize(N * (2.0 * eps) + grad);
}

// 1 = full POM, 0 = flat base surface. Smoothly retracts grooves with
// distance so high-frequency detail does not sub-pixel alias.
float pomLodFade(float distanceToCamera) {
  return 1.0 - smoothstep(${lodFadeStart.toFixed(3)}, ${lodFadeEnd.toFixed(3)}, distanceToCamera);
}
`;
};

export const buildPomMainBlock = (pomBounded: boolean): string => /* glsl */ `
  // --- Procedural Parallax Occlusion Mapping (phase 1: heightfield, no discard) ---
  // Raymarch the view ray through the procedural slab, then redirect the
  // triplanar samplers to the displaced world-space hit point and replace the
  // shading normal with the analytic floor normal.
  vec3 triplanarSamplePos = vTriplanarPos;
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
      vec3 _pomHit = pomMarchBounded(vWorldPos, _pomRd, _pomNormalW, _pomD, curTimeSeconds, _pomMaxLen, _pomCarved);
      // A positive but sub-epsilon chord is ill-conditioned (differencing two
      // near-equal large distances) -> kept/discarded flickers (edge shimmer).
      // Commit it to carved for a stable edge; valid-bound path only.
      if (!_pomNoBound && _pomChord < max(2e-3, _pomD * 0.05)) {
        _pomCarved = true;
      }
      if (_pomCarved) {
        discard;
      }`
          : /* glsl */ `vec3 _pomHit = pomMarch(vWorldPos, _pomRd, _pomNormalW, _pomD, curTimeSeconds);`
      }
      triplanarSamplePos = vTriplanarPos + (_pomHit - vWorldPos);
      float _pomEps = max(unitsPerPx, _pomD * 0.02);
      _pomNormalW = mix(
        _pomNormalW,
        pomAnalyticNormal(_pomHit, _pomNormalW, _pomD, curTimeSeconds, _pomEps),
        _pomFade
      );
    }
  }
  `;

export const POM_NORMAL_OVERRIDE = /* glsl */ `
  // POM owns the shading normal; override the (view-space) normal with the analytic floor normal.
  normal = normalize((viewMatrix * vec4(_pomNormalW, 0.)).xyz);
`;
