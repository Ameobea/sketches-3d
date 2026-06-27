// Shared prelude for the grooved-plastic material: near-monochrome dark
// plastic, flat except for grate-like square fields of parallel carved slots
// arranged on a grid. Slots are aligned per square (fixed count, inset from
// every edge) and carving is zero outside the squares, so the marcher's first
// sample terminates over most of the surface.

const float GP_CELL  = 4.;  // square grid pitch
const float GP_SQ    = 1.7;  // slot-field half-size (Chebyshev)
const float GP_SLOTS = 10.;   // slots per square
const float GP_PITCH = 2. * GP_SQ / GP_SLOTS;

const float GP_HW       = 0.042; // slot floor half-width
const float GP_WALL     = 0.018; // slot wall ramp width
const float GP_END      = 0.;  // slot end inset from the square edge
const float GP_END_WALL = 0.06;  // slot end-wall ramp width
const float GP_END_OUT  = GP_SQ - GP_END;
const float GP_CARVE    = 0.8;   // marcher clamps carved depth at 0.8 = full pom.depth

const vec3 GP_BASE_COLOR   = vec3(0.106, 0.110, 0.117);
const vec3 GP_GROOVE_COLOR = vec3(0.012, 0.013, 0.014);

const float GP_AO_GROOVE     = 0.6;  // indirect mul at full carve
const float GP_DIRECT_GROOVE = 0.45; // direct mul at full carve; kills specular punch-through

// Cell-boundary seam: a shallow rounded chamfer on each cell's rim meeting at
// a thin, slightly deeper gap notch — a molded panel joint, not another slot.
const float GP_SEAM_W     = 0.12;  // chamfer width on each side of the boundary
const float GP_SEAM_DEPTH = 0.16;  // chamfer carve at the boundary (slots carve GP_CARVE)
const float GP_HAIR_HW    = 0.012; // gap notch half-width
const float GP_HAIR_WALL  = 0.025;
const float GP_HAIR_DEPTH = 0.14;  // extra carve of the gap notch
const float GP_SEAM_TINT  = 0.16;  // albedo toward groove color on the chamfer
const float GP_HAIR_TINT  = 0.5;   // …and on the gap line
const float GP_AO_SEAM    = 0.85;
const float GP_DIRECT_HAIR = 0.55;

// Dominant-axis projection into 2D (Y→xz, X→zy, Z→xy), matching the other POM materials.
vec2 gpProjectUV(vec3 pos, vec3 axisNormal) {
  vec3 a = abs(axisNormal);
  if (a.y >= a.x && a.y >= a.z) {
    return pos.xz;
  } else if (a.x >= a.z) {
    return vec2(pos.z, pos.y);
  }
  return vec2(pos.x, pos.y);
}

// Chebyshev distance from the cell center; `cl` = signed cell-local coords, `cellId` for parity.
float gpSquareDist(vec2 uv, out vec2 cl, out vec2 cellId) {
  vec2 c = uv / GP_CELL;
  cellId = floor(c);
  cl = (fract(c) - 0.5) * GP_CELL;
  return max(abs(cl.x), abs(cl.y));
}

// Slot direction alternates 90° per cell (checkerboard).
bool gpSlotsAlongX(vec2 cellId) {
  return mod(cellId.x + cellId.y, 2.) < 0.5;
}

// grid tier: the one per-cell datum is the slot orientation; the engine caches this across
// the march. GP_CELL must equal pom.cellPitch in materials.json (engine owns the decomposition).
struct GpCell {
  bool alongX;
};
GpCell gridComputeCell(vec2 cellId) {
  return GpCell(gpSlotsAlongX(cellId));
}

// Signed offset to the nearest slot centerline. Slot k of GP_SLOTS sits at
// cell-local (k + 0.5)*GP_PITCH - GP_SQ, so all slots land strictly inside the
// square with a half-pitch margin at both sides.
float gpSlotOffset(float l) {
  return (fract((l + GP_SQ) / GP_PITCH) - 0.5) * GP_PITCH;
}

// AA'd visual carve (slot coverage) shared by the color + attenuation slots;
// `cl` must already be inside the square.
float gpSlotCarve(vec2 cl, vec2 cellId, float aa) {
  bool alongX = gpSlotsAlongX(cellId);
  float g = abs(gpSlotOffset(alongX ? cl.y : cl.x));
  float a = abs(alongX ? cl.x : cl.y);
  float slot = aaSlot(g, GP_PITCH, GP_HW, GP_WALL, aa);
  float we = max(0.5 * GP_END_WALL, aa);
  float end = 1. - smoothstep(GP_END_OUT - 0.5 * GP_END_WALL - we, GP_END_OUT - 0.5 * GP_END_WALL + we, a);
  return slot * end;
}

// Seam carve vs distance-to-cell-boundary `b`; the gap notch lies inside the chamfer.
float gpSeamCarve(float b) {
  return GP_SEAM_DEPTH * clamp((GP_SEAM_W - b) / GP_SEAM_W, 0., 1.)
       + GP_HAIR_DEPTH * clamp((GP_HAIR_HW + GP_HAIR_WALL - b) / GP_HAIR_WALL, 0., 1.);
}

// AA'd seam visibility as (chamfer, gap) profiles, for the color + attenuation slots.
vec2 gpSeamVis(float b, float aa) {
  float mid = GP_HAIR_HW + 0.5 * GP_HAIR_WALL;
  float w = max(0.5 * GP_HAIR_WALL, aa);
  float chamfer = 1. - smoothstep(0., GP_SEAM_W, b);
  float hair = 1. - smoothstep(mid - w, mid + w, b);
  // Isolated lines at the cell boundary: dissolve as they go sub-pixel.
  return vec2(aaThinFeature(chamfer, aa, 2. * GP_SEAM_W), aaThinFeature(hair, aa, 2. * mid));
}
