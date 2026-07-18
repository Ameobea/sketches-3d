// Oxidized copper tiles: square tiles with chamfered corners, so the recessed
// seams meet in diamond-shaped pits where four notches join. Faces sit flat at
// the base surface (the marcher first-sample-terminates there); the carve is a
// seam valley with broad pillowy shoulders. Weathering is per-tile: a hashed
// palette position plus mottling from a pre-composed 8-octave FBM texture
// (uNoiseTex), sampled in cell-local coords with a per-tile domain offset so
// every tile's grime pattern is unique and breaks at the seams like
// independently-weathered sheets.

const float CU_CELL    = 2.;
const float CU_NOTCH   = 0.20;  // chamfer cut |x|+|y| = CELL − NOTCH; the pit reaches NOTCH along each edge
const float CU_SEAM_HW = 0.014; // seam flat-bottom half-width
const float CU_BEVEL_W = 0.12;  // rounded shoulder from valley edge up to the flat face
const float CU_EDGE_W  = CU_SEAM_HW + CU_BEVEL_W;
const float CU_DEPTH   = 0.85;  // seam recess (fraction of pom.depth)

const vec3 CU_COL_A      = vec3(0.30, 0.17, 0.14);   // dusty salmon
const vec3 CU_COL_B      = vec3(0.135, 0.09, 0.085); // mauve brown
const vec3 CU_PATINA_COL = vec3(0.10, 0.145, 0.135); // gray verdigris
const vec3 CU_SEAM_COL   = vec3(0.07, 0.05, 0.042);

const float CU_PATINA_MAX = 0.85; // patina never fully replaces the base tone
const float CU_VAL_MIN    = 0.62; // per-tile value spread — some tiles distinctly dark
const float CU_VAL_MAX    = 1.18;
const float CU_GRAIN_AMP  = 0.09;
const float CU_BLOTCH_VAL = 0.10; // within-tile soft value drift from the blotch field
const float CU_RIM_W      = 0.14; // weathering creep past the bevel onto the face
const float CU_RIM_PATINA = 0.55;
const float CU_RIM_DARK   = 0.78;

const float CU_BLOTCH_SCALE = 0.06;
const float CU_GRAIN_SCALE  = 0.8;
const float CU_NOISE_TEX_SIZE = 1024.;

const float CU_ROUGH_TILE   = 0.5;
const float CU_ROUGH_PATINA = 0.92;
const float CU_ROUGH_GRAIN  = 0.06;
const float CU_ROUGH_SEAM   = 0.85;

const float CU_SEAM_VIS_HW = CU_SEAM_HW + 0.12 * CU_BEVEL_W; // visually-dark band
const float CU_SEAM_MEAN   = 0.08; // seam+pit area fraction; the distant-blend target

const float CU_AO_SEAM     = 0.55;
const float CU_DIRECT_SEAM = 0.7;

// Distance to the tile boundary: two straight cell-edge families plus the
// corner chamfer line; `dir` = outward pattern-space direction across the
// nearest one (∇b = −dir). b goes negative inside a corner pit.
float cuDist(vec2 uv, out vec2 cl, out vec2 cellId, out vec2 dir) {
  vec2 c = uv / CU_CELL;
  cellId = floor(c);
  cl = (fract(c) - 0.5) * CU_CELL;
  vec2 bd = 0.5 * CU_CELL - abs(cl);
  float bn = (CU_CELL - CU_NOTCH - abs(cl.x) - abs(cl.y)) * 0.70710678;
  if (bn <= min(bd.x, bd.y)) {
    dir = vec2(sign(cl.x), sign(cl.y)) * 0.70710678;
    return bn;
  }
  dir = bd.x <= bd.y ? vec2(sign(cl.x), 0.) : vec2(0., sign(cl.y));
  return min(bd.x, bd.y);
}

float cuCarve(float b) {
  return CU_DEPTH * (1. - smoothstep(CU_SEAM_HW, CU_EDGE_W, b));
}

float cuDirAA(vec2 dir) {
  return max(length(dir * patAA()), 1e-4);
}

// Box-filtered coverage of the dark half-space b < CU_SEAM_VIS_HW (so pit
// interiors where b < 0 stay covered), settling on the area mean once the
// footprint outgrows the cell.
float cuSeamVis(float b, float aa) {
  float w = max(aa, 0.25 * CU_BEVEL_W);
  float cov = clamp((CU_SEAM_VIS_HW - b + w) / (2. * w), 0., 1.);
  return fadeToMean(cov, CU_SEAM_MEAN, aa, CU_CELL);
}

// Per-tile character: x = palette position, y = patina coverage, z = value
// jitter, w = rim-weathering strength. Fades to the ensemble mean as tiles go
// sub-pixel so distant walls settle instead of shimmering.
vec4 cuTileParams(vec2 cellId, float aa) {
  vec4 h = vec4(hash(cellId + 0.7), hash(cellId + 3.1), hash(cellId + 5.9), hash(cellId + 9.4));
  return mix(h, vec4(0.5), fadeToMeanFactor(aa, CU_CELL));
}

// One tap of the pre-composed 8-octave FBM texture (uNoiseTex custom uniform).
// Explicit LOD from the analytic pattern footprint — screen-space derivatives are
// unreliable in POM-displaced slots — and the mip chain takes the signal to its
// mean as it goes sub-pixel, standing in for fadeToMean.
float cuNoise(vec2 p, float scale, float aa) {
  float lod = log2(max(aa * scale * CU_NOISE_TEX_SIZE, 1.));
  return textureLod(uNoiseTex, p * scale, lod).r;
}

// (patina, grain, rim, blotch), cached per fragment — color + roughness both
// hit it. The grain also jitters the patina boundary so blotch edges stay ragged.
vec4 cuSurfCache = vec4(-1.);
vec4 cuSurf(vec2 cl, vec2 cellId, float b, float aa) {
  if (cuSurfCache.x < 0.) {
    vec4 tp = cuTileParams(cellId, aa);
    vec2 o = 37. * vec2(hash(cellId + 11.3), hash(cellId + 17.9));
    float blotch = cuNoise(cl + o, CU_BLOTCH_SCALE, aa);
    float grain = cuNoise(cl + o + 61.7, CU_GRAIN_SCALE, aa);
    float th = mix(0.92, 0.44, tp.y * tp.y); // squared: heavy-patina tiles are the minority
    float patina = smoothstep(th - 0.22, th + 0.22, blotch + 0.15 * (grain - 0.5));
    float rim = 1. - smoothstep(CU_SEAM_HW, CU_EDGE_W + CU_RIM_W, b);
    patina = clamp(patina + CU_RIM_PATINA * tp.w * rim, 0., 1.);
    cuSurfCache = vec4(patina, grain, rim, blotch);
  }
  return cuSurfCache;
}
