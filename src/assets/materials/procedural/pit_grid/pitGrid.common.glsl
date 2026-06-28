// Infinite grid of pits: a flat top surface carved by a shaped hole centered in
// each cell, with smoothed top + floor lips. The lattice is a plain (optionally
// rectangular, optionally row-staggered) square grid; the hole SHAPE is just an
// SDF in cell-local space (square / diamond / triangle / trapezoid), so it has
// nothing to do with the lattice. Authored at L1 (projectedField + safeStep):
// the pattern is identical in every cell with no per-cell hashing, so the grid
// tier's only extra — a per-cell cache — would hold nothing; safeStep strides
// the flat padding via gridLateralDist just as well. Holes sit at cell centers,
// and the fold is to [-pitch/2, pitch/2], so the in-cell SDF is the globally
// nearest-hole distance — a valid 1-Lipschitz lateral distance for the marcher.

#ifndef PG_SHAPE
#define PG_SHAPE 0 // 0 square, 1 diamond, 2 equilateral triangle, 3 trapezoid
#endif
#ifndef PG_STAGGER
#define PG_STAGGER 0 // 1 = offset alternate rows by half the x-pitch (brick layout)
#endif
#ifndef PG_PITCH
#define PG_PITCH vec2(2.0) // cell size (x, y); rectangular cells allowed
#endif
#ifndef PG_RADIUS
#define PG_RADIUS 0.6 // hole half-size; per-axis padding = PG_PITCH/2 - PG_RADIUS
#endif
#ifndef PG_WALL_HW
#define PG_WALL_HW 0.12 // half-width of the smoothed wall band (larger = gentler, longer wall)
#endif
#ifndef PG_DEPTH
#define PG_DEPTH 0.85 // floor carve as a fraction of pom.depth
#endif
#ifndef PG_TRAP_TOP
#define PG_TRAP_TOP 0.65 // trapezoid top/bottom width ratio (PG_SHAPE == 3)
#endif
#ifndef PG_TOP_COLOR
#define PG_TOP_COLOR vec3(0.30, 0.31, 0.33)
#endif
#ifndef PG_PIT_COLOR
#define PG_PIT_COLOR vec3(0.05, 0.052, 0.058)
#endif
#ifndef PG_DUTY
#define PG_DUTY 0.22 // approx hole area fraction: the tone the pattern dissolves to at distance
#endif
#ifndef PG_AO
#define PG_AO 0.5 // indirect mul at full pit depth
#endif

const float PG_FADE_PERIOD = min(PG_PITCH.x, PG_PITCH.y);

// Signed cell-local coords (offset to this cell's hole center), in [-pitch/2, pitch/2].
vec2 pgCellLocal(vec2 uv) {
  vec2 u = uv;
#if PG_STAGGER
  u.x -= 0.5 * PG_PITCH.x * mod(floor(uv.y / PG_PITCH.y), 2.0);
#endif
  return (fract(u / PG_PITCH) - 0.5) * PG_PITCH;
}

// Exact (1-Lipschitz) hole SDFs in cell-local space; negative inside the hole. (iq SDF set.)
float pgSquare(vec2 p, float r) {
  vec2 q = abs(p) - r;
  return length(max(q, 0.)) + min(max(q.x, q.y), 0.);
}
float pgDiamond(vec2 p, float r) {
  return (abs(p.x) + abs(p.y) - r) * 0.70710678;
}
float pgTriangle(vec2 p, float r) {
  const float k = 1.7320508;
  p.x = abs(p.x) - r;
  p.y = p.y + r / k;
  if (p.x + k * p.y > 0.) {
    p = vec2(p.x - k * p.y, -k * p.x - p.y) * 0.5;
  }
  p.x -= clamp(p.x, -2. * r, 0.);
  return -length(p) * sign(p.y);
}
float pgTrapezoid(vec2 p, float r1, float r2, float he) {
  vec2 k1 = vec2(r2, he), k2 = vec2(r2 - r1, 2. * he);
  p.x = abs(p.x);
  vec2 ca = vec2(p.x - min(p.x, (p.y < 0.) ? r1 : r2), abs(p.y) - he);
  vec2 cb = p - k1 + k2 * clamp(dot(k1 - p, k2) / dot(k2, k2), 0., 1.);
  float s = (cb.x < 0. && ca.y < 0.) ? -1. : 1.;
  return s * sqrt(min(dot(ca, ca), dot(cb, cb)));
}

float pgHoleSdf(vec2 p) {
#if PG_SHAPE == 1
  return pgDiamond(p, PG_RADIUS);
#elif PG_SHAPE == 2
  return pgTriangle(p, PG_RADIUS);
#elif PG_SHAPE == 3
  return pgTrapezoid(p, PG_RADIUS, PG_RADIUS * PG_TRAP_TOP, PG_RADIUS);
#else
  return pgSquare(p, PG_RADIUS);
#endif
}

// Carve vs signed hole-SDF d: full floor inside (d < -HW), 0 on top (d > +HW), one smoothstep
// wall rounding both lips. .x = carve, .y = dcarve/dd (shared by height + normal).
vec2 pgCarveVS(float d) {
  vec2 ss = smoothstepVS(-PG_WALL_HW, PG_WALL_HW, d);
  return vec2(PG_DEPTH * (1. - ss.x), -PG_DEPTH * ss.y);
}

// safeStep lateral distance: distance outside the wall band (|d| > HW is flat top or flat floor).
// Staggered rows can place the nearer hole in the adjacent row, so min with it to stay conservative.
float gridLateralDist(vec2 uv) {
  float d = pgHoleSdf(pgCellLocal(uv));
#if PG_STAGGER
  float cy = (fract(uv.y / PG_PITCH.y) - 0.5) * PG_PITCH.y;
  float ny = uv.y + sign(cy) * PG_PITCH.y;
  vec2 nu = vec2(uv.x - 0.5 * PG_PITCH.x * mod(floor(ny / PG_PITCH.y), 2.0), ny);
  d = min(d, pgHoleSdf((fract(nu / PG_PITCH) - 0.5) * PG_PITCH));
#endif
  return max(0., abs(d) - PG_WALL_HW);
}
