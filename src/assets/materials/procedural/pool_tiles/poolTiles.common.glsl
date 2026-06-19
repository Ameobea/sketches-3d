// Small square pool/mosaic tiles: flat glossy tile faces flush at the base
// surface, joined by recessed grout channels along the cell boundaries. Each
// tile rim rolls down a rounded bevel into a flat grout valley — a chebyshev
// (square-grid) glazed-ceramic mosaic. Carving is zero across the tile face, so
// the marcher's first sample terminates over almost the whole surface. A glossy
// tile / matte grout roughness split plus valley AO give the wet liminal-pool
// read.

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

const float PT_GROUT_FADE_W = 8. * PT_EDGE_W; // footprint scale at which the grout grid dissolves

// Dominant-axis projection into 2D (Y→xz, X→zy, Z→xy), matching the other POM materials.
vec2 ptProjectUV(vec3 pos, vec3 axisNormal) {
  vec3 a = abs(axisNormal);
  if (a.y >= a.x && a.y >= a.z) {
    return pos.xz;
  } else if (a.x >= a.z) {
    return vec2(pos.z, pos.y);
  }
  return vec2(pos.x, pos.y);
}

// Distance to the nearest cell boundary; `cl` = signed cell-local coords.
float ptBoundaryDist(vec2 uv, out vec2 cl) {
  cl = (fract(uv / PT_CELL) - 0.5) * PT_CELL;
  return 0.5 * PT_CELL - max(abs(cl.x), abs(cl.y));
}

// Grout carve vs boundary distance `b`: full depth across the flat valley
// (b < PT_GROUT_HW), rounded bevel up to the flat face (b ≥ PT_EDGE_W → 0).
float ptGroutCarve(float b) {
  return PT_DEPTH * (1. - smoothstep(PT_GROUT_HW, PT_EDGE_W, b));
}

// AA'd grout-line coverage [0,1] for color/roughness/attenuation — the valley
// only; the rim stays tile-colored (its shading comes from the relief normal).
// Thin grid → dissolves to 0 as the footprint outgrows the line.
float ptValleyVis(float b, float aa) {
  float w = max(0.4 * PT_BEVEL_W, aa);
  float cov = 1. - smoothstep(PT_GROUT_HW - w, PT_GROUT_HW + w, b);
  return aaThinFeature(cov, aa, PT_GROUT_FADE_W);
}
