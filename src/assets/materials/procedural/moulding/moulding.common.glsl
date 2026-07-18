// Fluted decorative moulding: infinite horizontal rows (repeating along pattern v)
// of vertical pill-shaped flutes, read as trim strips. Within a row: capsule-SDF
// flutes rise from a carved valley floor; flat rails run along the row's top and
// bottom edges, arriving while the flutes' rounded caps still have height so the
// caps emerge through them as scallops; rows are separated by a deeper gap. Carve
// and its gradient come from one function (mdCarveG) so the height and normal
// slots can't drift apart. UV mode (PAT_UV_MODE) reads mesh UVs for rail_sweep-style
// trim: with the default MD_ROW_PITCH of 1 the rows tile seamlessly across the
// normalized v-wrap. Like pool_tiles UV mode, the carve is then constant along the
// view ray — relief reads as surface-painted, shaded by the relief normal.

#ifndef MD_ROW_PITCH
#define MD_ROW_PITCH 1.0 // row spacing (pattern v period)
#endif
#ifndef MD_FLUTE_PITCH
#define MD_FLUTE_PITCH 0.18 // flute repeat along u
#endif
#ifndef MD_FLUTE_R
#define MD_FLUTE_R 0.45 // flute half-width, fraction of MD_FLUTE_PITCH
#endif
#ifndef MD_FLUTE_HL
#define MD_FLUTE_HL 0.22 // straight half-length of the flute capsule (v units)
#endif
#ifndef MD_RAIL_HL
#define MD_RAIL_HL 0.42 // rail band outer edge; beyond it the inter-row gap begins
#endif
#ifndef MD_FLUTE_CARVE
#define MD_FLUTE_CARVE 0.55 // valley depth between flutes (fraction of pom.depth)
#endif
#ifndef MD_RAIL_CARVE
#define MD_RAIL_CARVE 0.22 // rail plateau depth: below flute crests, above valleys
#endif
#ifndef MD_GAP_CARVE
#define MD_GAP_CARVE 0.95 // inter-row gap depth
#endif
#ifndef MD_COLOR
#define MD_COLOR vec3(0.52, 0.38, 0.22)
#endif
#ifndef MD_COLOR_DEEP
#define MD_COLOR_DEEP vec3(0.26, 0.17, 0.09) // valley/gap albedo (finish pooling in recesses)
#endif
#ifndef MD_GRAIN_SCALE
#define MD_GRAIN_SCALE vec2(0.9, 9.0) // wood-grain fbm freq (low u = streaks run along the trim)
#endif
#ifndef MD_GRAIN_AMP
#define MD_GRAIN_AMP 0.16
#endif
#ifndef MD_ROUGH_TOP
#define MD_ROUGH_TOP 0.6
#endif
#ifndef MD_ROUGH_DEEP
#define MD_ROUGH_DEEP 0.8
#endif
#ifndef MD_AO_DEEP
#define MD_AO_DEEP 0.5 // indirect mul at full recess
#endif
#ifndef MD_DIRECT_DEEP
#define MD_DIRECT_DEEP 0.8 // direct mul at full recess
#endif

const float MD_R = MD_FLUTE_R * MD_FLUTE_PITCH; // flute half-width in pattern units
const float MD_CAP_R0 = MD_FLUTE_HL + 0.55 * MD_R; // rail ramp: starts late enough that the valley…
const float MD_CAP_R1 = MD_FLUTE_HL + 1.05 * MD_R; // …wedges reach the cap tips (scalloped rail edge)
const float MD_GAP_RAMP = 0.05 * MD_ROW_PITCH;
// UV mode: noise-cylinder radius matching the grain's v frequency, so the wrap arc
// length equals the planar domain span (grain density preserved, seam-free).
const float MD_GRAIN_CYL_R = PAT_UV_SCALE.y * MD_GRAIN_SCALE.y / 6.2832;

// smoothstep value + slope (the grid-tier lib isn't included for L0 materials).
vec2 mdSSVS(float e0, float e1, float x) {
  float t = clamp((x - e0) / (e1 - e0), 0., 1.);
  return vec2(t * t * (3. - 2. * t), 6. * t * (1. - t) / (e1 - e0));
}

// Per-fragment flute relief fade, lazily cached in a global so the march doesn't
// recompute it per step. Keyed tight (half the flute half-width, cf. grooved_plastic
// keying to its wall ramp): the relief normal dies while the color coverage still
// resolves the pattern, and the cov fades below carry it the rest of the way.
float mdKeep = -1.;
float mdKeepF() {
  if (mdKeep < 0.) {
    mdKeep = reliefAAFade(patAA().x, 0.5 * MD_R);
  }
  return mdKeep;
}

// Carve only — the marcher's hot path, no gradient math. Base profile across the
// row (valley floor → rail → gap) composed with the capsule flutes via min —
// wherever a flute cap rises above the base it wins, which is what makes the caps
// emerge through the rail ramp as scallops. `keep` fades the flutes (the finest
// repeat) toward the base profile so distant rows keep their band structure
// without flute moiré; pass 1 for the sharp field.
float mdCarve(vec2 p, float keep) {
  float alv = abs((fract(p.y / MD_ROW_PITCH) - 0.5) * MD_ROW_PITCH);
  float lu = (fract(p.x / MD_FLUTE_PITCH) - 0.5) * MD_FLUTE_PITCH;
  float rail = mix(MD_FLUTE_CARVE, MD_RAIL_CARVE, smoothstep(MD_CAP_R0, MD_CAP_R1, alv));
  float base = mix(rail, MD_GAP_CARVE, smoothstep(MD_RAIL_HL, MD_RAIL_HL + MD_GAP_RAMP, alv));
  float qy = max(alv - MD_FLUTE_HL, 0.);
  float d2 = lu * lu + qy * qy;
  if (d2 < MD_R * MD_R) {
    float fluteC = MD_FLUTE_CARVE * (1. - sqrt(MD_R * MD_R - d2) / MD_R);
    return min(base, mix(base, fluteC, keep));
  }
  return base;
}

// Pattern-space carve gradient (∂u, ∂v) for the relief normal — evaluated once per
// fragment, so it can afford the slope math. Flute gradient fades with mdKeepF()
// (matching the carve), the base ramps with the v footprint against their own width.
vec2 mdCarveGrad(vec2 p) {
  float lv = (fract(p.y / MD_ROW_PITCH) - 0.5) * MD_ROW_PITCH;
  float alv = abs(lv), sv = sign(lv);
  float lu = (fract(p.x / MD_FLUTE_PITCH) - 0.5) * MD_FLUTE_PITCH;

  vec2 r1 = mdSSVS(MD_CAP_R0, MD_CAP_R1, alv);
  vec2 r2 = mdSSVS(MD_RAIL_HL, MD_RAIL_HL + MD_GAP_RAMP, alv);
  float rail = mix(MD_FLUTE_CARVE, MD_RAIL_CARVE, r1.x);
  float base = mix(rail, MD_GAP_CARVE, r2.x);
  float dBase = (MD_RAIL_CARVE - MD_FLUTE_CARVE) * r1.y * (1. - r2.x) + (MD_GAP_CARVE - rail) * r2.y;

  float keep = mdKeepF();
  float qy = max(alv - MD_FLUTE_HL, 0.);
  float d2 = lu * lu + qy * qy;
  if (d2 < MD_R * MD_R && keep > 0.) {
    float d = sqrt(d2);
    float h = sqrt(MD_R * MD_R - d2); // circular flute cross-section, height h/R
    float fluteC = mix(base, MD_FLUTE_CARVE * (1. - h / MD_R), keep);
    if (fluteC < base) {
      float dcdd = keep * MD_FLUTE_CARVE * d / (MD_R * max(h, 0.15 * MD_R)); // clamp the near-vertical rim
      vec2 gd = d > 1e-5 ? vec2(lu, qy * sv) / d : vec2(0.);
      return dcdd * gd;
    }
  }
  return vec2(0., dBase * sv * reliefAAFade(patAA().y, MD_GAP_RAMP));
}

// AA'd recess coverage [0,1] shared by color/roughness/attenuation, from the SHARP
// carve (decoupled from the relief fade, so tone converges to the true means with
// no pop when the relief flattens): crests 0 → full-depth recesses 1, dissolving
// to the area mean as the footprint outgrows the flutes, then the row.
float mdCov(vec2 p, vec2 aa) {
  float cov = smoothstep(0., MD_FLUTE_CARVE, mdCarve(p, 1.));
  cov = fadeToMean(cov, 0.55, aa.x, MD_R);
  return fadeToMean(cov, 0.58, aa.y, MD_ROW_PITCH);
}
