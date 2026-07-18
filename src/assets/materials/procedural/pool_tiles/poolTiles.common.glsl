// Small square pool/mosaic tiles: flat glossy tile faces flush at the base
// surface, joined by recessed grout channels along the cell boundaries. Each
// tile rim rolls down a rounded bevel into a flat grout valley — a chebyshev
// (square-grid) glazed-ceramic mosaic. Carving is zero across the tile face, so
// the marcher's first sample terminates over almost the whole surface. A glossy
// tile / matte grout roughness split plus valley AO give the wet liminal-pool
// read. AA is anisotropic: every slot filters the two grout-line families
// against a per-axis pattern-space footprint from patAA().

// Overridable via `shaders.constants`, for swept/unwrapped meshes where world
// projection smears. UV mode reads the base-surface vUv, so the carve is constant
// along the view ray — no parallax slide; the relief reads as surface-painted.

const float PT_CELL = 1.0; // tile pitch

const float PT_GROUT_HW = 0.022; // grout valley half-width (flat bottom)
const float PT_BEVEL_W  = 0.045; // rounded rim run from valley edge up to the flat face
const float PT_EDGE_W   = PT_GROUT_HW + PT_BEVEL_W; // carve nonzero only within this of a boundary
const float PT_DEPTH    = 0.85;  // grout recess (fraction of pom.depth)

const vec3 PT_TILE_COLOR  = vec3(0.66, 0.64, 0.52); // warm cream glaze
const vec3 PT_GROUT_COLOR = vec3(0.14, 0.135, 0.09); // muted olive grout

const float PT_TILE_ROUGH  = 0.03; // wet glazed face
const float PT_GROUT_ROUGH = 0.8;  // porous matte grout (clearcoat adds the wet sheen on top)

const float PT_AO_GROUT     = 0.55; // indirect mul in the valley
const float PT_DIRECT_GROUT = 0.7;  // direct mul in the valley

const float PT_GROUT_DUTY = 2. * PT_GROUT_HW / PT_CELL; // one line family's area mean

// Signed cell-local coords `cl` + per-axis distance to the nearest cell boundary.
vec2 ptBoundaryDist(vec2 uv, out vec2 cl) {
  cl = (fract(uv / PT_CELL) - 0.5) * PT_CELL;
  return 0.5 * PT_CELL - abs(cl);
}

// Grout carve vs boundary distance `b` (= min axis of ptBoundaryDist): full depth across
// the flat valley (b < PT_GROUT_HW), rounded bevel up to the flat face (b ≥ PT_EDGE_W → 0).
float ptGroutCarve(float b) {
  return PT_DEPTH * (1. - smoothstep(PT_GROUT_HW, PT_EDGE_W, b));
}

// Box-filtered coverage of one grout-line family: |footprint ∩ line| / footprint.
// Integral-conserving, so sub-pixel lines dim instead of shimmering, and hands off
// continuously to the duty-cycle mean once the footprint outgrows the cell.
float ptLineVis(float b, float aa) {
  float w = max(aa, 0.4 * PT_BEVEL_W);
  float cov = clamp((min(b + w, PT_GROUT_HW) - max(b - w, -PT_GROUT_HW)) / (2. * w), 0., 1.);
  return fadeToMean(cov, PT_GROUT_DUTY, aa, PT_CELL);
}

// AA'd grout coverage for color/roughness/attenuation: union of the two line families,
// each filtered by its own footprint axis.
float ptValleyVis(vec2 bd, vec2 aa) {
  float vx = ptLineVis(bd.x, aa.x);
  float vy = ptLineVis(bd.y, aa.y);
  return vx + vy - vx * vy;
}

// Relief collapse keyed to the bevel width on the carving (nearest-boundary) axis —
// the denser screen-space axis flattens first.
float ptReliefFade(vec2 bd) {
  vec2 aa = patAA();
  return reliefAAFade(bd.x <= bd.y ? aa.x : aa.y, PT_BEVEL_W);
}
