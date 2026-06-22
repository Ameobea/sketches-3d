// Chevron-strip material: parallel zig-zag ridges separated by carved crevices. The relief is a
// 1-D profile in a sheared coordinate w = across + chTri(along): level sets of w are the zig-zags,
// bands of w are the strips, floor(w/pitch) is the strip index (alternating albedo, carried through
// the POM hit). Each crevice is a flat floor, a near-vertical wall, then a gentle bevel rolling off
// only the ridge's top edge. Distances live in w-space; |∇w| = CH_G converts to the UV distance the
// marcher strides by. cf. grateTrench.common.glsl (1-D trench) + triangleGrid.common.glsl (hitType).

const bool CH_ALONG_X = true; // zig-zags run along projected-UV x; strips repeat across y

// Overridable tunables: each is an `#ifndef` default that a per-instance `shaders.constants` entry
// replaces (extend the library material and set e.g. { "CH_AMP": { "type": "float", "value": 2.2 } }).
#ifndef CH_RUN
#define CH_RUN 6.0
#endif
#ifndef CH_AMP
#define CH_AMP 1.5
#endif
#ifndef CH_WPITCH
#define CH_WPITCH 5.0
#endif
#ifndef CH_FLOOR_HW
#define CH_FLOOR_HW 0.45
#endif
#ifndef CH_WALL_W
#define CH_WALL_W 0.10
#endif
#ifndef CH_BEVEL_W
#define CH_BEVEL_W 0.10
#endif
#ifndef CH_CARVE
#define CH_CARVE 0.8
#endif
#ifndef CH_BEVEL_DROP
#define CH_BEVEL_DROP 0.25
#endif
#ifndef CH_WALL_DARKEN
#define CH_WALL_DARKEN 0.4
#endif
#ifndef CH_COLOR_A
#define CH_COLOR_A vec3(0.27, 0.30, 0.34)
#endif
#ifndef CH_COLOR_B
#define CH_COLOR_B vec3(0.40, 0.19, 0.15)
#endif
#ifndef CH_FLOOR_COLOR
#define CH_FLOOR_COLOR vec3(0.018, 0.020, 0.023)
#endif

// Derived from the tunables above; recompute automatically when those are overridden.
const float CH_SLOPE = 4. * CH_AMP / CH_RUN;           // |d(chTri)/d(along)|
const float CH_G     = sqrt(1. + CH_SLOPE * CH_SLOPE); // |∇w|: w-space → UV distance scale
// Crevice cross-section, w-space, out from the centerline:
//   [0, FLOOR_HW) flat floor · [.., WALL_END) near-vertical wall · [.., BEVEL_END) top-edge bevel · ridge top
const float CH_WALL_END    = CH_FLOOR_HW + CH_WALL_W;
const float CH_BEVEL_END   = CH_WALL_END + CH_BEVEL_W;
const float CH_WALL_DROP   = CH_CARVE - CH_BEVEL_DROP;  // wall removes the carve the bevel doesn't
const float CH_FADE_PERIOD = 2.0 * CH_BEVEL_END;        // w-space footprint at which crevice bands dissolve to ridge

const float CH_BAND_PAD = 0.04; // insets the floor/wall color bands so POM hit imprecision can't bleed them

// Fake crevice cast-shadow + AO, art-directed (cf. triangleGrid.common.glsl).
const vec3  CH_SHADOW_LIGHT_DIR = normalize(vec3(70., 12., -40.));
const float CH_SHADOW_REACH     = 1.4; // w-space reach of the shadow across the trench at the floor
const float CH_SHADOW_PENUMBRA  = 0.35;
const float CH_SHADOW_DARKEN    = 0.05;
const float CH_SHADOW_WALL_LIFT = 0.4; // reach kept at the rim (cd=0), climbing to full at the floor
const float CH_AO_DEPTH      = 0.6;
const float CH_AO_WALL       = 0.5;
const float CH_AO_WALL_RANGE = 0.35;

float chTri(float u) {
  return CH_AMP * (1. - 4. * abs(fract(u / CH_RUN) - 0.5));
}
float chSlopeSign(float u) {
  return fract(u / CH_RUN) < 0.5 ? 1. : -1.;
}

// Lean per-step field for the marcher's height/lateral paths: signed offset to the nearest crevice
// centerline, w-space. The full hitType frame (gridComputeHit) is evaluated only once, at the hit.
float chWOff(vec2 uv) {
  float u = CH_ALONG_X ? uv.x : uv.y;
  float a = CH_ALONG_X ? uv.y : uv.x;
  float w = a + chTri(u);
  return (fract(w / CH_WPITCH + 0.5) - 0.5) * CH_WPITCH;
}

// ∇w in UV (un-normalized; |∇w| = CH_G). The local zig segment has slope su·CH_SLOPE.
vec2 chGradW(float su) {
  return CH_ALONG_X ? vec2(su * CH_SLOPE, 1.) : vec2(1., su * CH_SLOPE);
}

// Shared at-hit frame (hitType): one warp eval reused by color/attenuation/normal.
//   aw = |across-offset to crevice centerline| (w-space, the profile coord) · off = its sign (which wall)
//   su = local zig segment slope sign (for ∇w) · idx = strip index (alternating albedo)
struct ChHit { float aw; float off; float su; float idx; };
ChHit gridComputeHit(vec2 uv) {
  float u = CH_ALONG_X ? uv.x : uv.y;
  float a = CH_ALONG_X ? uv.y : uv.x;
  float w = a + chTri(u);
  float wOff = (fract(w / CH_WPITCH + 0.5) - 0.5) * CH_WPITCH;
  return ChHit(abs(wOff), sign(wOff), chSlopeSign(u), floor(w / CH_WPITCH));
}

// Carve profile vs w-space across-distance `aw`, paired with dcarve/daw so height and normal share
// one definition. .x = carve (CH_CARVE on the floor → 0 on the top), .y = slope (≤0 in the ramps).
vec2 chCarveVS(float aw) {
  vec2 sw = smoothstepVS(CH_FLOOR_HW, CH_WALL_END, aw);
  vec2 sb = smoothstepVS(CH_WALL_END, CH_BEVEL_END, aw);
  return vec2(CH_CARVE - CH_WALL_DROP * sw.x - CH_BEVEL_DROP * sb.x,
              -CH_WALL_DROP * sw.y - CH_BEVEL_DROP * sb.y);
}

// safeStep lateral distance to the nearest height-varying band (wall ∪ bevel), /CH_G into UV so it's
// a conservative (1-Lipschitz) stride. Flat floor + ridge top read 0 carve → first-sample early-out.
float gridLateralDist(vec2 uv) {
  float aw = abs(chWOff(uv));
  return max(0., max(CH_FLOOR_HW - aw, aw - CH_BEVEL_END)) / CH_G;
}

// Analytic crevice cast-shadow: the up-light wall shades across the trench. `sOff` = signed w-space
// across position, `su` selects the local segment's across-axis (so the two arms of a chevron shade
// opposite walls), `cd` = carved-depth fraction (the floor reaches farther into shadow than the rim).
// Reach to the up-light rim / the light's across-component; mirrors triPitShadowFromEdges for one trench.
float chCreviceShadow(float sOff, float su, float cd, vec3 worldN) {
  if (dot(worldN, CH_SHADOW_LIGHT_DIR) <= 0.) {
    return 0.;
  }
  vec2 Lraw = domProject(CH_SHADOW_LIGHT_DIR, domAxis(worldN));
  float Llen = length(Lraw);
  if (Llen < 1e-4) {
    return 0.;
  }
  float La = dot(Lraw, chGradW(su) / CH_G) / Llen; // unit projected light · unit across-axis
  float reach;
  if (La > 1e-3) {
    reach = (CH_BEVEL_END - sOff) / La;
  } else if (La < -1e-3) {
    reach = (CH_BEVEL_END + sOff) / -La;
  } else {
    return 0.; // light runs along the trench
  }
  float shadowLen = mix(CH_SHADOW_WALL_LIFT, 1., cd) * CH_SHADOW_REACH;
  float r = reach / max(shadowLen, 1e-4);
  return 1. - smoothstep(1. - max(CH_SHADOW_PENUMBRA, 1e-3), 1., r);
}
