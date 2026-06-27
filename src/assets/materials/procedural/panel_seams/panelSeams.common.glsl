// Standalone chamfered panel-seam material: flat dark plastic panels joined
// along cell boundaries by a molded joint — a shallow rounded chamfer on each
// panel's rim meeting at a thin, slightly deeper gap notch. Carving is zero
// away from the seams, so the marcher's first sample terminates over almost
// the whole surface.

const float PS_CELL = 5.;

const float PS_SEAM_W     = 0.16;  // chamfer width on each side of the boundary
const float PS_SEAM_DEPTH = 0.18;  // chamfer carve at the boundary (0.8 = full pom.depth)
const float PS_HAIR_HW    = 0.012; // gap notch half-width
const float PS_HAIR_WALL  = 0.025;
const float PS_HAIR_DEPTH = 0.14;  // extra carve of the gap notch
const float PS_SEAM_FADE_W = 8. * PS_SEAM_W; // footprint scale at which seams dissolve; raise = persist longer

const vec3 PS_BASE_COLOR = vec3(0.106, 0.110, 0.117);
const vec3 PS_DARK_COLOR = vec3(0.012, 0.013, 0.014);

const float PS_SEAM_TINT   = 0.16; // albedo toward dark color on the chamfer
const float PS_HAIR_TINT   = 0.5;  // …and on the gap line
const float PS_AO_SEAM     = 0.85;
const float PS_DIRECT_HAIR = 0.55;

// Dominant-axis projection lives in proceduralMaterialGrid.glsl (domProject/domAxis/domUnproject).

// Distance to the nearest cell boundary; `cl` = signed cell-local coords (for
// the gradient direction).
float psBoundaryDist(vec2 uv, out vec2 cl) {
  cl = gridCellLocal(uv, PS_CELL);
  return 0.5 * PS_CELL - max(abs(cl.x), abs(cl.y));
}

// Seam carve vs boundary distance `b` (the gap notch lies inside the chamfer) paired with its
// slope dcarve/db, so height and normal share one definition. .x = carve, .y = dcarve/db.
vec2 psSeamCarveVS(float b) {
  vec2 ss = smoothstepVS(0., PS_SEAM_W, b);
  vec2 sh = smoothstepVS(PS_HAIR_HW, PS_HAIR_HW + PS_HAIR_WALL, b);
  return vec2(PS_SEAM_DEPTH * (1. - ss.x) + PS_HAIR_DEPTH * (1. - sh.x),
              -PS_SEAM_DEPTH * ss.y - PS_HAIR_DEPTH * sh.y);
}

// AA'd seam visibility as (chamfer, gap) profiles, for the color + attenuation slots.
vec2 psSeamVis(float b, float aa) {
  float mid = PS_HAIR_HW + 0.5 * PS_HAIR_WALL;
  float w = max(0.5 * PS_HAIR_WALL, aa);
  float chamfer = 1. - smoothstep(0., PS_SEAM_W, b);
  float hair = 1. - smoothstep(mid - w, mid + w, b);
  // Isolated lines at the cell boundary: dissolve as they go sub-pixel. Keyed to
  // the joint width (PS_SEAM_FADE_W), not the hair's own tiny width, so the dark
  // seam line persists as long as the joint is resolvable instead of popping out.
  return vec2(aaThinFeature(chamfer, aa, PS_SEAM_FADE_W), aaThinFeature(hair, aa, PS_SEAM_FADE_W));
}
