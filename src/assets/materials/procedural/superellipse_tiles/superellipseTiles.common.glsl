// Raised rounded-square (superellipse, n=8) tiles with beveled edges: flat
// tile tops at the base surface, a straight chamfer down the rim with
// C1-rounded top edge + floor fillet, and narrow carved gaps between tiles.
// Subtractive POM fakes the "raised" read by carving everything except the
// tops. Exponent fixed at 8 so the field is pure multiplies + sqrts — no pow.

const float SE_CELL  = 8.;   // tile grid pitch
const float SE_R     = 3.7;  // tile half-size: superellipse radius of the bevel's top edge
const float SE_BEV   = 0.15; // bevel width (horizontal run, approx-distance units)
const float SE_RND   = 0.08;  // corner rounding radius as a fraction of SE_BEV (< 0.5)
const float SE_FLOOR = 0.8;  // gap carve depth (0.8 = full pom.depth)

// Insets the visual (color/attenuation) profile so POM hit imprecision at the
// tile edge can't bleed top color down the bevel — the TRI_WALL_BAND_PAD trick.
const float SE_COL_PAD = 0.05;

const vec3 SE_TOP_COLOR   = vec3(0.21, 0.20, 0.185);  // warm light gray, away from the plastic family
const vec3 SE_FLOOR_COLOR = vec3(0.045, 0.042, 0.038);

const float SE_AO_FLOOR     = 0.6;
const float SE_DIRECT_FLOOR = 0.75; // mild: the gaps still catch direct sun

vec2 seCellLocal(vec2 uv) {
  return (fract(uv / SE_CELL) - 0.5) * SE_CELL;
}

// Superellipse radius f = (x⁸+y⁸)^⅛ of the cell-local point.
float seRadius(vec2 cl) {
  vec2 q2 = cl * cl;
  q2 *= q2;
  return sqrt(sqrt(sqrt(q2.x * q2.x + q2.y * q2.y)));
}

// First-order signed distance to the tile outline (f = SE_R level set):
// s = (f−R)/|∇f| with |∇f| = √(x¹⁴+y¹⁴)/f⁷. |∇f| ∈ [2^-⅜ ≈ 0.77, 1] for n=8,
// so this is near-exact and the bevel width stays uniform around corners.
// `dir` is the outward gradient direction (x⁷, y⁷) normalized — used as ∇s by
// the normal shader (exact up to curvature terms).
float seOutlineDist(vec2 cl, float f, out vec2 dir) {
  vec2 q = cl * cl;
  vec2 g = cl * q * q * q;
  float glen = max(length(g), 1e-9);
  dir = g / glen;
  float f2 = f * f;
  float f4 = f2 * f2;
  return (f - SE_R) * (f4 * f2 * f) / glen;
}

// C1 ramp: linear chamfer mid-section, parabolic blends of radius r at both
// corners. Returns (value, d/du). Requires r < 0.5.
vec2 seRamp(float u, float r) {
  if (u <= -r) {
    return vec2(0., 0.);
  }
  if (u >= 1. + r) {
    return vec2(1., 0.);
  }
  if (u < r) {
    float w = u + r;
    return vec2(w * w / (4. * r), w / (2. * r));
  }
  if (u <= 1. - r) {
    return vec2(u, 1.);
  }
  float w = 1. + r - u;
  return vec2(1. - w * w / (4. * r), w / (2. * r));
}

// One per-fragment evaluation of the cell field, shared by the hit-time slots (height marches
// separately). dir/s are meaningful only in the bevel band — the union of the color-padded and
// unpadded normal bands — so the flats early-out before reading them.
struct SeHit { float f; float s; vec2 dir; };
SeHit gridComputeHit(vec2 uv) {
  vec2 cl = seCellLocal(uv);
  float f = seRadius(cl);
  vec2 dir = vec2(0.);
  float s = 0.;
  if (f > SE_R - SE_RND * SE_BEV - SE_COL_PAD && f - SE_R < (1. + SE_RND) * SE_BEV) {
    s = seOutlineDist(cl, f, dir);
  }
  return SeHit(f, s, dir);
}

// Visual carve for the color/attenuation slots: the profile shifted SE_COL_PAD
// inward. Early-out bounds stay exact because |∇f| ≤ 1.
float seVisCarveHit(SeHit h) {
  if (h.f <= SE_R - SE_RND * SE_BEV - SE_COL_PAD) {
    return 0.;
  }
  if (h.f - SE_R >= (1. + SE_RND) * SE_BEV - SE_COL_PAD) {
    return 1.;
  }
  return seRamp((h.s + SE_COL_PAD) / SE_BEV, SE_RND).x;
}
