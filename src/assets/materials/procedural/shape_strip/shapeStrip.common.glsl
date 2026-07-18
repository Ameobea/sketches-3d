// Strip-based pit_grid variant: infinite horizontal rows (repeating along pattern
// v) of shaped pits carved into a flat top surface — one run of shapes per row,
// centered on the row's centerline, with optional thin grooves running the full
// span along each row's top and bottom edges. Shapes are the pit_grid SDF set
// (square / diamond / triangle / trapezoid) with the same smoothed-lip carve.
// UV mode (PAT_UV_MODE) reads mesh UVs for rail_sweep-style trim; the default
// SS_ROW_PITCH of 1 tiles seamlessly across the normalized v-wrap. NB: with
// SS_STAGGER the pattern's v period doubles, so a seamless wrap needs an even
// number of rows per wrap.

#ifndef SS_SHAPE
#define SS_SHAPE 0 // 0 square, 1 diamond, 2 equilateral triangle, 3 trapezoid
#endif
#ifndef SS_STAGGER
#define SS_STAGGER 0 // 1 = offset alternate rows by half the cell pitch
#endif
#ifndef SS_GROOVE
#define SS_GROOVE 1 // 1 = horizontal groove along each row edge, running the full span
#endif
#ifndef SS_ROW_PITCH
#define SS_ROW_PITCH 1.0 // row spacing (pattern v period)
#endif
#ifndef SS_CELL
#define SS_CELL 0.5 // shape repeat along u
#endif
#ifndef SS_RADIUS
#define SS_RADIUS 0.16 // shape half-size
#endif
#ifndef SS_WALL_HW
#define SS_WALL_HW 0.05 // half-width of the smoothed pit-wall band
#endif
#ifndef SS_DEPTH
#define SS_DEPTH 0.85 // pit floor carve (fraction of pom.depth)
#endif
#ifndef SS_TRAP_TOP
#define SS_TRAP_TOP 0.65 // trapezoid top/bottom width ratio (SS_SHAPE == 3)
#endif
#ifndef SS_GROOVE_POS
#define SS_GROOVE_POS 0.32 // groove centerline distance from the row centerline
#endif
#ifndef SS_GROOVE_HW
#define SS_GROOVE_HW 0.02 // groove half-width
#endif
#ifndef SS_GROOVE_DEPTH
#define SS_GROOVE_DEPTH 0.45 // groove carve (fraction of pom.depth)
#endif
#ifndef SS_TOP_COLOR
#define SS_TOP_COLOR vec3(0.30, 0.31, 0.33)
#endif
#ifndef SS_PIT_COLOR
#define SS_PIT_COLOR vec3(0.05, 0.052, 0.058)
#endif
#ifndef SS_DUTY
#define SS_DUTY 0.26 // approx recessed area fraction: the distance-dissolve tone
#endif
#ifndef SS_AO
#define SS_AO 0.5 // indirect mul at full pit depth
#endif

const float SS_DUTY_U = min(1., 2. * SS_RADIUS / SS_CELL); // approx shape coverage along a row band

// Row/cell-local coords: x folded to the nearest shape center in the row's run,
// y signed offset from the row centerline in [-pitch/2, pitch/2].
vec2 ssCellLocal(vec2 p) {
  float row = floor(p.y / SS_ROW_PITCH);
  float u = p.x;
#if SS_STAGGER
  u -= 0.5 * SS_CELL * mod(row, 2.);
#endif
  return vec2((fract(u / SS_CELL) - 0.5) * SS_CELL, p.y - (row + 0.5) * SS_ROW_PITCH);
}

// Exact (1-Lipschitz) shape SDFs, negative inside; ported from pit_grid (iq SDF set).
float ssSquare(vec2 p, float r) {
  vec2 q = abs(p) - r;
  return length(max(q, 0.)) + min(max(q.x, q.y), 0.);
}
float ssDiamond(vec2 p, float r) {
  return (abs(p.x) + abs(p.y) - r) * 0.70710678;
}
float ssTriangle(vec2 p, float r) {
  const float k = 1.7320508;
  p.x = abs(p.x) - r;
  p.y = p.y + r / k;
  if (p.x + k * p.y > 0.) {
    p = vec2(p.x - k * p.y, -k * p.x - p.y) * 0.5;
  }
  p.x -= clamp(p.x, -2. * r, 0.);
  return -length(p) * sign(p.y);
}
float ssTrapezoid(vec2 p, float r1, float r2, float he) {
  vec2 k1 = vec2(r2, he), k2 = vec2(r2 - r1, 2. * he);
  p.x = abs(p.x);
  vec2 ca = vec2(p.x - min(p.x, (p.y < 0.) ? r1 : r2), abs(p.y) - he);
  vec2 cb = p - k1 + k2 * clamp(dot(k1 - p, k2) / dot(k2, k2), 0., 1.);
  float s = (cb.x < 0. && ca.y < 0.) ? -1. : 1.;
  return s * sqrt(min(dot(ca, ca), dot(cb, cb)));
}

float ssShapeSdf(vec2 p) {
#if SS_SHAPE == 1
  return ssDiamond(p, SS_RADIUS);
#elif SS_SHAPE == 2
  return ssTriangle(p, SS_RADIUS);
#elif SS_SHAPE == 3
  return ssTrapezoid(p, SS_RADIUS, SS_RADIUS * SS_TRAP_TOP, SS_RADIUS);
#else
  return ssSquare(p, SS_RADIUS);
#endif
}

float ssGrooveSdf(float lv) {
  return abs(abs(lv) - SS_GROOVE_POS) - SS_GROOVE_HW;
}

// Carve vs signed SDF d: full floor inside (d < -HW), 0 on top (d > +HW), one
// smoothstep wall rounding both lips. .x = carve, .y = dcarve/dd.
vec2 ssCarveVS(float d, float depth) {
  float t = clamp((d + SS_WALL_HW) / (2. * SS_WALL_HW), 0., 1.);
  return vec2(depth * (1. - t * t * (3. - 2. * t)), -depth * 6. * t * (1. - t) / (2. * SS_WALL_HW));
}

// AA'd recess coverage (pits ∪ grooves) shared by color/attenuation, dissolved
// anisotropically: shapes collapse along u to a row-band tone (gated by the band
// mask so the rows stay banded while only aa.x is large), grooves box-filter on
// the v footprint via aaSlot, then the whole row dissolves to the duty mean.
float ssCov(vec2 p, vec2 aa) {
  float cov = 1. - aaStep(0., ssShapeSdf(p), max(aa.x, aa.y));
  float band = 1. - aaStep(0., abs(p.y) - SS_RADIUS, aa.y);
  cov = fadeToMean(cov, band * SS_DUTY_U, aa.x, SS_CELL);
#if SS_GROOVE
  cov = max(cov, aaSlot(abs(abs(p.y) - SS_GROOVE_POS), SS_ROW_PITCH, SS_GROOVE_HW, 2. * SS_WALL_HW, aa.y));
#endif
  return fadeToMean(cov, SS_DUTY, aa.y, SS_ROW_PITCH);
}
