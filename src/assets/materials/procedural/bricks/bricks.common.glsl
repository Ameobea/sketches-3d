// Sooty cinderblock wall: flat block faces flush at the base surface, laid in a
// running bond and joined by recessed mortar joints with rounded rims. Carving is
// zero across the block face, so the marcher's first sample terminates over almost
// the whole surface. Grime is layered procedurally in the color/roughness slots:
// broad thresholded-fbm soot patches, block-scale staining, fine aggregate grain
// (footprint-faded), and per-brick tint jitter. AA mirrors pool_tiles: every slot
// filters the two joint-line families against a per-axis pattern-space footprint.

// Overridable knobs (#ifndef defaults, replaceable per-instance via shaders.constants).
// NB: the grime/grain fbm domains aren't wrap-periodic, so BR_UV_MODE on a closed
// sweep will show a soot seam at the v-wrap (cf. moulding's cylinder-embedded grain
// for the fix pattern if that use case matters).
#ifndef BR_UV_MODE
#define BR_UV_MODE 0 // 0 = world-space dominant-axis projection, 1 = mesh-UV pattern space
#endif
#ifndef BR_UV_SCALE
#define BR_UV_SCALE vec2(1.0) // pattern units per vUv unit (BR_UV_MODE == 1)
#endif
#ifndef BR_CELL
#define BR_CELL vec2(2.0, 1.0) // brick pitch (width, height)
#endif
#ifndef BR_OFFSET
#define BR_OFFSET 0.5 // per-row x offset, fraction of brick width (running bond)
#endif
#ifndef BR_JOINT_HW
#define BR_JOINT_HW 0.02 // mortar joint half-width (flat bottom)
#endif
#ifndef BR_BEVEL_W
#define BR_BEVEL_W 0.03 // rounded rim run from joint edge up to the flat face
#endif
#ifndef BR_DEPTH
#define BR_DEPTH 0.85 // joint recess (fraction of pom.depth)
#endif
#ifndef BR_BLOCK_COLOR
#define BR_BLOCK_COLOR vec3(0.25, 0.285, 0.26)
#endif
#ifndef BR_MORTAR_COLOR
#define BR_MORTAR_COLOR vec3(0.20, 0.21, 0.19)
#endif
#ifndef BR_SOOT_COLOR
#define BR_SOOT_COLOR vec3(0.045, 0.06, 0.065)
#endif
#ifndef BR_SOOT_SCALE
#define BR_SOOT_SCALE vec2(0.22, 0.13) // soot fbm freq per axis (y < x → vertical smear)
#endif
#ifndef BR_SOOT_LO
#define BR_SOOT_LO 0.5 // grime-field threshold where soot begins ...
#endif
#ifndef BR_SOOT_HI
#define BR_SOOT_HI 0.70 // ... and saturates
#endif
#ifndef BR_SOOT_AMT
#define BR_SOOT_AMT 0.85 // max soot opacity
#endif
#ifndef BR_MOTTLE_AMP
#define BR_MOTTLE_AMP 0.22 // block-scale stain contrast
#endif
#ifndef BR_GRAIN_SCALE
#define BR_GRAIN_SCALE 20.0 // fine aggregate-speckle freq
#endif
#ifndef BR_GRAIN_AMP
#define BR_GRAIN_AMP 0.15
#endif
#ifndef BR_TINT_AMP
#define BR_TINT_AMP 0.13 // per-brick value jitter
#endif
#ifndef BR_BLOCK_ROUGH
#define BR_BLOCK_ROUGH 0.88
#endif
#ifndef BR_MORTAR_ROUGH
#define BR_MORTAR_ROUGH 0.95
#endif
#ifndef BR_SOOT_ROUGH
#define BR_SOOT_ROUGH 0.96
#endif
#ifndef BR_AO_JOINT
#define BR_AO_JOINT 0.55 // indirect mul in the joint valley
#endif
#ifndef BR_DIRECT_JOINT
#define BR_DIRECT_JOINT 0.7 // direct mul in the joint valley
#endif

const float BR_EDGE_W = BR_JOINT_HW + BR_BEVEL_W; // carve nonzero only within this of a boundary
const vec2 BR_DUTY = 2. * BR_JOINT_HW / BR_CELL; // per-family joint area means

// Pattern-space projection: dominant-axis into 2D (Y→xz, X→zy, Z→xy) like the other
// POM materials, or scaled mesh UV under BR_UV_MODE.
vec2 brProjectUV(vec3 pos, vec3 axisNormal) {
#if BR_UV_MODE == 1
  return vUv * BR_UV_SCALE;
#else
  vec3 a = abs(axisNormal);
  if (a.y >= a.x && a.y >= a.z) {
    return pos.xz;
  } else if (a.x >= a.z) {
    return vec2(pos.z, pos.y);
  }
  return vec2(pos.x, pos.y);
#endif
}

// Per-axis pixel footprint in pattern units.
vec2 brAA() {
#if BR_UV_MODE == 1
  return aaUvFootprint * BR_UV_SCALE;
#else
  return vec2(aaWorldFootprint);
#endif
}

// Running-bond cell: brickId for hashing, signed cell-local coords `cl`, and the
// per-axis distance to the nearest joint centerline.
vec2 brCellField(vec2 uv, out vec2 brickId, out vec2 cl) {
  vec2 g = uv / BR_CELL;
  g.x += BR_OFFSET * floor(g.y);
  brickId = floor(g);
  cl = (fract(g) - 0.5) * BR_CELL;
  return 0.5 * BR_CELL - abs(cl);
}

// Joint carve vs boundary distance `b` (= min axis): full depth across the flat
// mortar bottom (b < BR_JOINT_HW), rounded bevel up to the flat face (b ≥ BR_EDGE_W → 0).
float brJointCarve(float b) {
  return BR_DEPTH * (1. - smoothstep(BR_JOINT_HW, BR_EDGE_W, b));
}

// Box-filtered coverage of one joint-line family: |footprint ∩ joint| / footprint,
// handing off to the family's duty-cycle mean once the footprint outgrows the cell.
float brLineVis(float b, float aa, float duty, float period) {
  float w = max(aa, 0.4 * BR_BEVEL_W);
  float cov = clamp((min(b + w, BR_JOINT_HW) - max(b - w, -BR_JOINT_HW)) / (2. * w), 0., 1.);
  return fadeToMean(cov, duty, aa, period);
}

// AA'd mortar coverage: union of the two joint families, each filtered by its own
// footprint axis (the families have different duty cycles on rectangular bricks).
float brJointVis(vec2 bd, vec2 aa) {
  float vx = brLineVis(bd.x, aa.x, BR_DUTY.x, BR_CELL.x);
  float vy = brLineVis(bd.y, aa.y, BR_DUTY.y, BR_CELL.y);
  return vx + vy - vx * vy;
}

// Relief collapse keyed to the bevel width on the carving (nearest-joint) axis.
float brReliefFade(vec2 bd) {
  vec2 aa = brAA();
  return reliefAAFade(bd.x <= bd.y ? aa.x : aa.y, BR_BEVEL_W);
}

// Sooty grime coverage [0, BR_SOOT_AMT]: a broad fbm places the blotches, two
// finer fbms roughen their edges and mottle their interiors, and the threshold
// hardens the sum into ragged patches. Low-frequency relative to the pixel
// footprint at any distance where it's visible, so no filtering of its own.
float brSoot(vec2 uv) {
  float f = 0.62 * fbm(uv * BR_SOOT_SCALE) + 0.30 * fbm(uv * BR_SOOT_SCALE * 4.3 + 7.7) +
            0.18 * fbm(uv * BR_SOOT_SCALE * 13. + 31.);
  return BR_SOOT_AMT * smoothstep(BR_SOOT_LO, BR_SOOT_HI, f);
}
