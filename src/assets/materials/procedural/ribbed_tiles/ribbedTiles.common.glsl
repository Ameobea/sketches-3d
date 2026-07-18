// Ribbed concrete tiles: a square tile grid where checkerboard-alternating tiles
// carry a field of fine horizontal grooves and the rest stay smooth, split by
// thin recessed joints. Carving is zero outside grooves/joints, so the marcher
// first-sample-terminates over most of the surface.

const float RB_TILE     = 4.;   // tile pitch; must equal pom.cellPitch
const float RB_FIELD    = 1.86;  // groove-field half-size (Chebyshev)
const float RB_GROOVES  = 20.;
const float RB_PITCH    = 2. * RB_FIELD / RB_GROOVES;
const float RB_HW       = 0.026; // groove floor half-width
const float RB_WALL     = 0.02;  // groove wall ramp width
const float RB_END_WALL = 0.1;   // groove end ramp width
const float RB_CARVE    = 0.8;   // marcher clamps carved depth at 0.8 = full pom.depth

const float RB_SEAM_HW    = 0.014; // joint floor half-width
const float RB_SEAM_WALL  = 0.04;
const float RB_SEAM_W     = RB_SEAM_HW + RB_SEAM_WALL;
const float RB_SEAM_DEPTH = 0.4;

const vec3  RB_BASE_COLOR    = vec3(0.265, 0.285, 0.315);
const vec3  RB_SEAM_COLOR    = vec3(0.10, 0.108, 0.12);
const float RB_TINT_AMP      = 0.09; // per-tile albedo value jitter
const float RB_GROOVE_DARKEN = 0.8; // groove-floor albedo multiplier

const float RB_AO_GROOVE     = 0.62; // indirect mul at full carve
const float RB_DIRECT_GROOVE = 0.7;  // direct mul at full carve; kills specular punch-through
const float RB_AO_SEAM       = 0.62;
const float RB_DIRECT_SEAM   = 0.6;

// Chebyshev distance from the tile center; `cl` = signed cell-local coords.
float rbCellDist(vec2 uv, out vec2 cl, out vec2 cellId) {
  vec2 c = uv / RB_TILE;
  cellId = floor(c);
  cl = (fract(c) - 0.5) * RB_TILE;
  return max(abs(cl.x), abs(cl.y));
}

bool rbRibbed(vec2 cellId) {
  return mod(cellId.x + cellId.y, 2.) < 0.5;
}

// grid tier: the one per-cell datum is the ribbed flag; the engine caches it across
// the march. RB_TILE must equal pom.cellPitch (engine owns the decomposition).
struct RbCell {
  bool ribbed;
};
RbCell gridComputeCell(vec2 cellId) {
  return RbCell(rbRibbed(cellId));
}

// Signed offset to the nearest groove centerline. Grooves are horizontal lines
// strictly inside the field with a half-pitch margin at both y ends.
float rbGrooveOffset(float y) {
  return (fract((y + RB_FIELD) / RB_PITCH) - 0.5) * RB_PITCH;
}

// AA'd visual groove coverage shared by the color + attenuation slots;
// `cl` must already be inside the field.
float rbGrooveCarve(vec2 cl, float aa) {
  float g = abs(rbGrooveOffset(cl.y));
  float slot = aaSlot(g, RB_PITCH, RB_HW, RB_WALL, aa);
  float we = max(0.5 * RB_END_WALL, aa);
  float mid = RB_FIELD - 0.5 * RB_END_WALL;
  float end = 1. - smoothstep(mid - we, mid + we, abs(cl.x));
  return slot * end;
}

// Joint carve vs distance-to-tile-boundary `b`.
float rbSeamCarve(float b) {
  return RB_SEAM_DEPTH * (1. - smoothstep(RB_SEAM_HW, RB_SEAM_W, b));
}

const float RB_SEAM_AA_SOFT = 0.4; // joint AA softness: < 1 fades later (a little shimmer traded for reach)

vec2 rbChebDir(vec2 cl) {
  return abs(cl.x) >= abs(cl.y) ? vec2(sign(cl.x), 0.) : vec2(0., sign(cl.y));
}

// Anisotropy-aware footprint across the tile joint, so a joint viewed along its
// own length keeps its darkening until the true width goes sub-pixel; the POM
// relief still fades on the isotropic footprint. Cached per fragment (color +
// attenuation both hit it on joint-band fragments).
float rbSeamAACache = -1.;
float rbSeamAA(vec2 cl) {
  if (rbSeamAACache < 0.) {
    rbSeamAACache = max(RB_SEAM_AA_SOFT * aaPatternDirFootprint(rbChebDir(cl)), 1e-4);
  }
  return rbSeamAACache;
}

// AA'd joint visibility for the color + attenuation slots; dissolves as the
// line goes sub-pixel.
float rbSeamVis(float b, float aa) {
  return aaLine(b, RB_SEAM_HW, RB_SEAM_WALL, aa);
}
