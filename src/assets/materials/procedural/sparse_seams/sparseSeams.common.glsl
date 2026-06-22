// Large-scale sparse panel seams: panel_seams scaled up to cover hundreds/thousands of units,
// but each cell-boundary SEGMENT is independently present or absent so the surface reads as a few
// long seams rather than a complete grid. Presence is a near-binary on/off (it scales the chamfer
// depth) from low-frequency value-noise sampled ALONG each seam — slow variation gives long
// contiguous runs; per-line decorrelation + separate H/V seeds keep the two seam families
// independent. Runs end abruptly (crisp caps); this stays safeStep-safe because inside a seam band
// gridLateralDist is already 0 (fine stepping), so a run-end cap is just another finely-marched
// wall, while the flat panels the marcher strides over never contain a seam. Carve is zero over
// the panel interiors (the vast majority), where the marcher terminates on its first sample.
//
// L1 projectedField: the two axes are handled separately (so each seam knows its line index +
// run coordinate), unlike panel_seams' single Chebyshev distance. The relief normal uses only the
// across-boundary chamfer slope × presence; the along-seam term (incl. the run-end cap) is dropped.

#ifndef SS_SEAM_W
#define SS_SEAM_W 1.1 // chamfer run on each side of a boundary
#endif
#ifndef SS_SEAM_DEPTH
#define SS_SEAM_DEPTH 0.7 // chamfer carve at the boundary (fraction of pom.depth)
#endif
#ifndef SS_HAIR_HW
#define SS_HAIR_HW 0.06 // gap-notch half-width
#endif
#ifndef SS_HAIR_WALL
#define SS_HAIR_WALL 0.12 // gap-notch wall run
#endif
#ifndef SS_HAIR_DEPTH
#define SS_HAIR_DEPTH 0.3 // extra carve of the gap notch
#endif
#ifndef SS_DENSITY
#define SS_DENSITY 0.32 // approx fraction of seam segments present (lower = sparser)
#endif
#ifndef SS_RUN_FREQ
#define SS_RUN_FREQ 0.13 // noise frequency along a seam (cell units); smaller = longer runs
#endif
#ifndef SS_RUN_SOFT
#define SS_RUN_SOFT 0.002 // presence step softness (small = crisp on/off run ends)
#endif
#ifndef SS_LINE_SEP
#define SS_LINE_SEP 2.7 // noise separation between parallel lines (decorrelates neighbours)
#endif
#ifndef SS_BASE_COLOR
#define SS_BASE_COLOR vec3(0.12, 0.125, 0.13)
#endif
#ifndef SS_DARK_COLOR
#define SS_DARK_COLOR vec3(0.02, 0.021, 0.023)
#endif
#ifndef SS_SEAM_TINT
#define SS_SEAM_TINT 0.22 // albedo toward dark on the chamfer
#endif
#ifndef SS_HAIR_TINT
#define SS_HAIR_TINT 0.5 // …and on the gap line
#endif
#ifndef SS_AO_SEAM
#define SS_AO_SEAM 0.85 // indirect mul over the chamfer valley
#endif
#ifndef SS_DIRECT_HAIR
#define SS_DIRECT_HAIR 0.55 // direct mul in the gap line
#endif
#ifndef SS_SCRATCH_FREQ
#define SS_SCRATCH_FREQ 0.6 // across-streak scratch frequency
#endif
#ifndef SS_SCRATCH_ANISO
#define SS_SCRATCH_ANISO 0.06 // along-streak frequency factor (small = long streaks); streaks run along uv.y
#endif
#ifndef SS_SCRATCH_AMP
#define SS_SCRATCH_AMP 0.1 // scratch darkening amplitude
#endif
#ifndef SS_PLATE_W
#define SS_PLATE_W 20.0 // mosaic column width (world units)
#endif
#ifndef SS_PLATE_H
#define SS_PLATE_H 15.0 // mosaic base panel height; per-column height = 1..HSTEPS × this
#endif
#ifndef SS_PLATE_HSTEPS
#define SS_PLATE_HSTEPS 7 // per-column random panel height in base-H units (1..HSTEPS); avg span = H·(1+HSTEPS)/2
#endif
#ifndef SS_PLATE_VAR
#define SS_PLATE_VAR 0.1 // per-panel brightness amplitude
#endif
#ifndef SS_GROOVE_W
#define SS_GROOVE_W 0.1 // plate-edge groove half-width (world units)
#endif
#ifndef SS_GROOVE_DARK
#define SS_GROOVE_DARK 0. // albedo multiplier in a full groove
#endif
// POM seam pitch is an integer multiple of the plate grid (per axis) so seams land on plate/groove
// lines instead of clashing: vertical seams every NX columns, horizontal seams every NY plate-heights.
#ifndef SS_SEAM_NX
#define SS_SEAM_NX 2
#endif
#ifndef SS_SEAM_NY
#define SS_SEAM_NY 3
#endif
const float SS_CELL_X = float(SS_SEAM_NX) * SS_PLATE_W;
const float SS_CELL_Y = float(SS_SEAM_NY) * SS_PLATE_H;

const float SS_SEAM_FADE_W = 8. * SS_SEAM_W;

// Near-binary presence [0,1] of a seam segment. `along` (continuous, cell units) is the run
// coordinate → low-freq noise gives long runs; `line` is the parallel-line index, separated so
// neighbours are independent; `seed` distinguishes the H and V seam families. Present where the
// noise is below SS_DENSITY (so SS_DENSITY ≈ fraction present); SS_RUN_SOFT keeps a sliver of ramp
// at the ends so the marcher bracket stays conditioned.
float ssPresent(float along, float line, float seed) {
  float n = noise(vec2(along * SS_RUN_FREQ, line * SS_LINE_SEP + seed));
  return smoothstep(SS_DENSITY + SS_RUN_SOFT, SS_DENSITY - SS_RUN_SOFT, n);
}

// Chamfer + gap-notch carve vs boundary distance `b`, paired with its slope dcarve/db.
vec2 ssSeamCarveVS(float b) {
  vec2 ss = smoothstepVS(0., SS_SEAM_W, b);
  vec2 sh = smoothstepVS(SS_HAIR_HW, SS_HAIR_HW + SS_HAIR_WALL, b);
  return vec2(SS_SEAM_DEPTH * (1. - ss.x) + SS_HAIR_DEPTH * (1. - sh.x),
              -SS_SEAM_DEPTH * ss.y - SS_HAIR_DEPTH * sh.y);
}

// AA'd seam visibility as (chamfer, gap) coverage for the color + attenuation slots; isolated
// lines dissolve as they go sub-pixel (keyed to the joint width, not the hair's tiny width).
vec2 ssSeamVis(float b, float aa) {
  float mid = SS_HAIR_HW + 0.5 * SS_HAIR_WALL;
  float w = max(0.5 * SS_HAIR_WALL, aa);
  float chamfer = 1. - smoothstep(0., SS_SEAM_W, b);
  float hair = 1. - smoothstep(mid - w, mid + w, b);
  return vec2(aaThinFeature(chamfer, aa, SS_SEAM_FADE_W), aaThinFeature(hair, aa, SS_SEAM_FADE_W));
}

// Hull albedo weathering: a smooth anisotropic scratch layer (noise stretched along uv.y) ×
// a quantized panel-mosaic layer — fixed-width columns split into per-column random-height panels
// (height a random 1..HSTEPS multiple of SS_PLATE_H, phase-offset per column so edges don't align),
// each a sharp-edged brightness jitter. Recessed grooves (color-only) darken plate edges: column
// edges always, strip edges only where the adjacent strip's shade differs. Each term dissolves to
// its mean by footprint so distant plating reads flat, not noisy.
vec3 ssDeckTint(vec3 base, vec2 uv, float aa) {
  float scratch = 1. - SS_SCRATCH_AMP * (1. - noise(vec2(uv.x * SS_SCRATCH_FREQ, uv.y * SS_SCRATCH_FREQ * SS_SCRATCH_ANISO)));
  scratch = mix(scratch, 1., fadeToMeanFactor(aa, 1. / SS_SCRATCH_FREQ));

  float c = floor(uv.x / SS_PLATE_W);
  float rh = SS_PLATE_H * floor(1. + hash(c + 19.3) * float(SS_PLATE_HSTEPS));
  float off = floor(hash(c + 41.7) * float(SS_PLATE_HSTEPS)) * SS_PLATE_H;
  float ly = fract((uv.y + off) / rh);
  float row = floor((uv.y + off) / rh);
  float panelRaw = 1. + SS_PLATE_VAR * (2. * hash(vec2(c, row) + 7.1) - 1.);
  float nbright = 1. + SS_PLATE_VAR * (2. * hash(vec2(c, row + (ly < 0.5 ? -1. : 1.)) + 7.1) - 1.);

  float aaw = max(aa, 1e-3);
  // float gV = 1. - smoothstep(SS_GROOVE_W - aaw, SS_GROOVE_W + aaw, (0.5 - abs(fract(uv.x / SS_PLATE_W) - 0.5)) * SS_PLATE_W);
  float gV = 0.;
  float gH = 1. - smoothstep(SS_GROOVE_W - aaw, SS_GROOVE_W + aaw, (0.5 - abs(ly - 0.5)) * rh);
  gH *= smoothstep(0., SS_PLATE_VAR, abs(panelRaw - nbright));
  float groove = aaThinFeature(max(gV, gH), aa, 8. * SS_GROOVE_W);

  float panel = mix(panelRaw, 1., fadeToMeanFactor(aa, min(SS_PLATE_W, SS_PLATE_H)));
  return base * scratch * panel * mix(1., SS_GROOVE_DARK, groove);
}

// Per-fragment seam state: boundary distances + cell-local signs (for the gradient) + the present
// presence of the nearest vertical/horizontal seam.
struct SsSeam {
  float bx;
  float by;
  float clx;
  float cly;
  float pv;
  float ph;
};

SsSeam ssEval(vec2 uv) {
  vec2 pitch = vec2(SS_CELL_X, SS_CELL_Y);
  vec2 cl = (fract(uv / pitch) - 0.5) * pitch;
  float lineX = floor(uv.x / SS_CELL_X + 0.5);
  float lineY = floor(uv.y / SS_CELL_Y + 0.5);
  return SsSeam(
    0.5 * SS_CELL_X - abs(cl.x),
    0.5 * SS_CELL_Y - abs(cl.y),
    cl.x, cl.y,
    ssPresent(uv.y / SS_CELL_Y, lineX, 0.),   // vertical seam runs along y
    ssPresent(uv.x / SS_CELL_X, lineY, 50.)); // horizontal seam runs along x
}

// safeStep lateral distance to the nearest boundary band (presence-agnostic, so conservative).
float gridLateralDist(vec2 uv) {
  vec2 pitch = vec2(SS_CELL_X, SS_CELL_Y);
  vec2 cl = (fract(uv / pitch) - 0.5) * pitch;
  return max(0., min(0.5 * SS_CELL_X - abs(cl.x), 0.5 * SS_CELL_Y - abs(cl.y)) - SS_SEAM_W);
}
