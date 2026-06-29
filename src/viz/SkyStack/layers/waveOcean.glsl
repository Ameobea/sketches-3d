// Stylized abstract ocean: the world below the horizon is a single-valued
// heightfield of infinite Z-slices ("waveforms"). Each slice runs along X with
// a sinusoidal hump profile; neighbouring slices slide against each other
// (even slices phase += sin t, odd += cos t) over a low-frequency world-space
// swell (base) and amplitude (wavefront) field. No overhangs — the only
// discontinuities are the vertical walls where one slice steps above its
// neighbour, resolved by an exact per-boundary plane test.
//
// The trace is two-phase: phase A skips the empty air down to the smooth
// `base+amp` upper envelope with only the low-frequency slope bound (cheap, no
// walls); phase B does the fine hump march inside the thin actual band. This
// keeps the expensive high-slope stepping confined to ~2*amp of world height.

// ---- Structure (world units) ----
const float SLICE_W_$ID = 49.; // Z-width of one waveform slice
const float WAVE_FREQ_$ID = 0.11; // along-slice hump spatial freq (rad/unit) → λ≈57
const float OCEAN_Y_$ID = -140.; // world-Y of the mean surface plane

// ---- Amplitudes (signed half-heights) ----
const float BASE_AMP_$ID = 32.;  // low-freq swell deviation about the mean
const float WAVE_AMP_MAX_$ID = 14.; // per-slice hump half-amplitude (tall regions)
const float WAVE_AMP_MIN_$ID = 3.; // hump half-amplitude (flat regions)

// ---- Animation ----
const float PHASE_SPEED_$ID = 0.85; // rad/s of the sin/cos slide
const float PHASE_SWING_$ID = 2.8; // how far each slice slides (rad)
const float BASE_DRIFT_$ID = 0.46; // swell evolution speed
const float AMP_DRIFT_$ID = 0.29; // wavefront evolution speed

// Low-freq field spatial frequencies (rad/unit). LOWFREQ_K bounds the steepest
// of these for the phase-A Lipschitz march; keep it ≥ every coefficient used.
const float LOWFREQ_K_$ID = 0.011;

// Phase A hands off to phase B once within this vertical distance of the upper
// envelope. The Lipschitz step approaches the envelope geometrically, so a
// strict `<= 0` test would never trip; phase B still starts above the true
// surface (H ≤ envelope) so no crossing is skipped.
const float ENTER_EPS_$ID = 0.75;

// ---- Appearance ----
const vec3  OCEAN_DEEP_$ID = vec3(0.012, 0.075, 0.17);
const vec3  OCEAN_BRIGHT_$ID = vec3(0.04, 0.42, 0.52);
const vec3  LIGHT_DIR_$ID = vec3(0.3015, 0.8616, 0.4070); // normalize(0.35,1.,0.47)
const float LIGHT_INT_$ID = 0.85;
const float AMBIENT_$ID = 0.32;
const float WALL_DARKEN_$ID = 0.55; // side-face brightness multiplier
const float NORMAL_EPS_$ID = 0.45; // world-space finite-diff step for normals

// ---- Anisotropic glint (sun-glitter-style glare that follows the horizon) ----
// A pale highlight that sparkles on light-facing wave faces, concentrated near
// the horizon and peaked toward a world azimuth (GLINT_DIR) with an
// omnidirectional floor (GLINT_BASE). World-anchored, so it stays at its
// bearing as the camera turns.
const vec2  GLINT_DIR_$ID = vec2(0.5, 0.866); // world azimuth of the glare (normalized)
const float GLINT_BASE_$ID = 0.3; // omnidirectional floor (0 = pure wedge)
const float GLINT_WIDTH_$ID = 0.2; // azAlign threshold for the peak (lower = broader)
const float GLINT_HOR0_$ID = 0.015; // negDy where glint is strongest (near horizon)
const float GLINT_HOR1_$ID = 0.5; // negDy where glint fades out (looking down)
const float GLINT_SHARP_$ID = 8.; // wave-face specular sharpness
const float GLINT_INTENSITY_$ID = 1.3;
const vec3  GLINT_COLOR_$ID = vec3(0.2, 0.4, 0.688);

// ---- LoD / fade ----
// Distance term: px = world-space size of one screen pixel at the entry dist
// (scaled by OCEAN_LOD_BIAS, higher on low quality → fades nearer).
const float AMP_FADE0_$ID  = 1.; // humps begin flattening
const float AMP_FADE1_$ID  = 2.; // humps gone
const float BASE_FADE0_$ID = 22.; // swell begins flattening
const float BASE_FADE1_$ID = 70.; // swell gone → flat plane, march skipped
// Grazing term (resolution-independent): fade by view elevation (negDy = -dir.y).
const float GRAZE_AMP_LO_$ID = 0.012; // humps gone
const float GRAZE_AMP_HI_$ID = 0.054; // humps full
const float GRAZE_BASE_LO_$ID = 0.025;
const float GRAZE_BASE_HI_$ID = 0.08;
const float FOG_START_$ID = 1400.; // entry distance where alpha fade begins
const float FOG_END_$ID = 9000.; // entry distance where fully transparent

float baseField_$ID(vec2 p) {
  float t = uTime * BASE_DRIFT_$ID;
  float a = sin(dot(p, vec2(0.6, 0.8)) * 0.0085 + t * 1.3);
  float b = cos(dot(p, vec2(-0.7, 0.5)) * 0.006 - t);
  return BASE_AMP_$ID * 0.5 * (a + b);
}

float ampField_$ID(vec2 p) {
  float t = uTime * AMP_DRIFT_$ID;
  float w = 0.5 + 0.5 * cos(dot(p, vec2(0.8, -0.6)) * 0.007 + t);
  return mix(WAVE_AMP_MIN_$ID, WAVE_AMP_MAX_$ID, w);
}

float slicePhase_$ID(float iz) {
  float osc = mod(iz, 2.) < 0.5 ? sin(uTime * PHASE_SPEED_$ID) : cos(uTime * PHASE_SPEED_$ID);
  osc = sign(osc) * pow(abs(osc), 3.);
  return hash(iz * 0.317 + 0.5) * TWO_PI + osc * PHASE_SWING_$ID;
}

// Signed surface height above the mean plane at world XZ for the given slice
// phase. lodAmp/lodBase fade the hump / swell contributions at distance.
float oceanHeight_$ID(vec2 p, float phase, float lodAmp, float lodBase) {
  return baseField_$ID(p) * lodBase + ampField_$ID(p) * lodAmp * sin(p.x * WAVE_FREQ_$ID + phase);
}

struct OceanHit_$ID {
  bool hit;
  float t;
  vec3 normal;
  vec2 p;
  float iz;
  float phase;
  bool isWall;
  int steps;
};

OceanHit_$ID traceOcean_$ID(vec3 ro, vec3 rd, float negDy, float lenHoriz, float lodAmp, float lodBase, float bandHalf) {
  OceanHit_$ID r;
  r.hit = false;
  r.isWall = false;
  r.steps = MAX_OCEAN_STEPS_$ID;

  float tExit = (ro.y - OCEAN_Y_$ID + bandHalf) / negDy;
  float t = max((ro.y - OCEAN_Y_$ID - bandHalf) / negDy, 0.);

  float lowSlope = (BASE_AMP_$ID * lodBase + WAVE_AMP_MAX_$ID * lodAmp) * LOWFREQ_K_$ID * lenHoriz;
  float invA = 1. / (negDy + lowSlope + 1e-4);

  // Phase A: skip empty air down to the smooth upper envelope (base + amp).
  // If the budget runs out here (grazing the envelope on a swell back-slope) we
  // fall through to phase B / the bisection fallback instead of returning a flat
  // miss — otherwise those pixels read as flat avg-color blobs.
  int i = 0;
  for (; i < MAX_OCEAN_STEPS_$ID; i++) {
    vec2 p = ro.xz + rd.xz * t;
    float fU = (ro.y - negDy * t) - (OCEAN_Y_$ID + baseField_$ID(p) * lodBase + ampField_$ID(p) * lodAmp);
    if (fU <= ENTER_EPS_$ID) {
      break;
    }
    t += fU * invA;
    if (t >= tExit) {
      r.steps = i;
      return r;
    }
  }

  // Phase B: fine hump march inside the band, resolving Z-slice walls exactly.
  float humpSlope = WAVE_AMP_MAX_$ID * lodAmp * WAVE_FREQ_$ID * lenHoriz;
  float invB = 1. / (negDy + humpSlope + lowSlope + 1e-4);

  float iz = floor((ro.z + rd.z * t) / SLICE_W_$ID);
  float phase = slicePhase_$ID(iz);
  float zStep = rd.z >= 0. ? 1. : -1.;
  float nextBoundZ = (iz + (zStep > 0. ? 1. : 0.)) * SLICE_W_$ID;
  float tNextZ = abs(rd.z) > 1e-6 ? (nextBoundZ - ro.z) / rd.z : 1e30;

  for (; i < MAX_OCEAN_STEPS_$ID; i++) {
    if (t >= tExit) {
      break;
    }
    vec2 p = ro.xz + rd.xz * t;
    float f = (ro.y - negDy * t) - (OCEAN_Y_$ID + oceanHeight_$ID(p, phase, lodAmp, lodBase));
    if (f <= 0.01) {
      r.hit = true;
      r.t = t;
      r.p = p;
      r.iz = iz;
      r.phase = phase;
      r.steps = i;
      return r;
    }

    float tCand = t + f * invB;
    if (tCand >= tNextZ) {
      t = tNextZ;
      iz += zStep;
      phase = slicePhase_$ID(iz);
      vec2 pb = ro.xz + rd.xz * t;
      float surfNew = OCEAN_Y_$ID + oceanHeight_$ID(pb, phase, lodAmp, lodBase);
      if ((ro.y - negDy * t) <= surfNew) {
        r.hit = true;
        r.isWall = true;
        r.t = t;
        r.p = pb;
        r.iz = iz;
        r.phase = phase;
        r.normal = vec3(0., 0., -zStep);
        r.steps = i;
        return r;
      }
      nextBoundZ += zStep * SLICE_W_$ID;
      tNextZ = (nextBoundZ - ro.z) / rd.z;
    } else {
      t = tCand;
    }
  }

  // Budget/band exhausted on a grazing back-slope: the conservative march kept
  // f >= 0 the whole way and tExit sits below the lower envelope (f < 0), so
  // [t, tExit] brackets the first crossing. Bisect onto the real surface so
  // these regions shade as ocean instead of the wrong (too-high) stuck position.
  float ta = t;
  float tb = tExit;
  for (int k = 0; k < 8; k++) {
    float tm = 0.5 * (ta + tb);
    vec2 pm = ro.xz + rd.xz * tm;
    float fm = (ro.y - negDy * tm) - (OCEAN_Y_$ID + oceanHeight_$ID(pm, phase, lodAmp, lodBase));
    if (fm > 0.) {
      ta = tm;
    } else {
      tb = tm;
    }
  }
  r.hit = true;
  r.t = ta;
  r.p = ro.xz + rd.xz * ta;
  r.iz = iz;
  r.phase = phase;
  r.steps = MAX_OCEAN_STEPS_$ID;
  return r;
}

// TODO: I'd like to make this noise radial and isotropic wrt. the origin.
//
// So instead of just plain patches of color like this, I'd like it to be sort of long bands that are ~parallel with
// the horizon.  Should probably also fade out with distance close to horizon (although I think that's already getting
// handled elsewhere in the code)
vec3 oceanBaseColor_$ID(vec2 pos) {
  float t = noise(pos * 0.002);
  t = t * t;
  return mix(OCEAN_DEEP_$ID, OCEAN_BRIGHT_$ID, t);
}

#if DEBUG_OCEAN_MODE == 1
vec3 oceanHeat_$ID(int value) {
  float tt = clamp(float(value) / float(MAX_OCEAN_STEPS_$ID), 0., 1.);
  vec3 c0 = vec3(0.05, 0., 0.4);
  vec3 c1 = vec3(0., 0.7, 0.9);
  vec3 c2 = vec3(0.95, 0.95, 0.);
  vec3 c3 = vec3(1., 0.1, 0.);
  vec3 a = mix(c0, c1, smoothstep(0., 0.33, tt));
  vec3 b = mix(a, c2, smoothstep(0.33, 0.66, tt));
  return mix(b, c3, smoothstep(0.66, 1., tt));
}
#endif

void sampleWaveOcean_$ID(vec3 dir, out vec3 outColor, out float outAlpha) {
  outColor = vec3(0.);
  outAlpha = 0.;

  if (dir.y > -0.001) {
    return;
  }

  vec3 ro = uCameraWorldMatrix[3].xyz;
  float negDy = max(-dir.y, 1e-4);
  float meanDepth = ro.y - OCEAN_Y_$ID;
  if (meanDepth < 1.) {
    return;
  }

  float tMean = meanDepth / negDy;

  float fogT = smoothstep(FOG_START_$ID, FOG_END_$ID, tMean);
  float alpha = 1. - fogT;
  if (alpha < 0.002) {
    return;
  }

  // LoD: distance footprint × grazing-elevation term.
  float angularPx = 2. * abs(uProjectionMatrixInverse[1][1]) / float(textureSize(uSceneDepth, 0).y);
  float px = angularPx * tMean * float(OCEAN_LOD_BIAS_$ID);
  float lodAmp = (1. - smoothstep(AMP_FADE0_$ID, AMP_FADE1_$ID, px)) * smoothstep(GRAZE_AMP_LO_$ID, GRAZE_AMP_HI_$ID, negDy);
  float lodBase = (1. - smoothstep(BASE_FADE0_$ID, BASE_FADE1_$ID, px)) * smoothstep(GRAZE_BASE_LO_$ID, GRAZE_BASE_HI_$ID, negDy);

  float avgLight = AMBIENT_$ID + max(LIGHT_DIR_$ID.y, 0.) * LIGHT_INT_$ID;
  vec3 avgColor = mix(OCEAN_DEEP_$ID, OCEAN_BRIGHT_$ID, 0.35) * avgLight;

  // Fully flattened — skip the march, shade the bare plane.
  if (lodBase < 0.01) {
    outColor = avgColor * alpha;
    outAlpha = alpha;
    return;
  }

  float lenHoriz = length(dir.xz);
  float bandHalf = BASE_AMP_$ID * lodBase + WAVE_AMP_MAX_$ID * lodAmp + 0.5;

  OceanHit_$ID hit = traceOcean_$ID(ro, dir, negDy, lenHoriz, lodAmp, lodBase, bandHalf);

#if DEBUG_OCEAN_MODE == 1
  outColor = oceanHeat_$ID(hit.steps) * alpha;
  outAlpha = alpha;
  return;
#endif

  if (!hit.hit) {
    outColor = avgColor * alpha;
    outAlpha = alpha;
    return;
  }

  vec3 normal = hit.normal;
  if (!hit.isWall) {
    float hC = oceanHeight_$ID(hit.p, hit.phase, lodAmp, lodBase);
    float hX = oceanHeight_$ID(hit.p + vec2(NORMAL_EPS_$ID, 0.), hit.phase, lodAmp, lodBase);
    float hZ = oceanHeight_$ID(hit.p + vec2(0., NORMAL_EPS_$ID), hit.phase, lodAmp, lodBase);
    normal = normalize(vec3(-(hX - hC), NORMAL_EPS_$ID, -(hZ - hC)));
  }

  // sharpen the normal a bit to make the glint more concentrated on the light-facing wave faces
  //
  // Not sure if this is correct, but it seems to work well enough
  // normal = normalize(pow(normal, vec3(1. / 1.5)));
  // have to preserve signs
  normal = sign(normal) * pow(abs(normal), vec3(16.));

  float ndl = max(dot(normal, LIGHT_DIR_$ID), 0.);
  float light = AMBIENT_$ID + ndl * LIGHT_INT_$ID;
  vec3 col = oceanBaseColor_$ID(hit.p) * light;
  if (hit.isWall) {
    col *= WALL_DARKEN_$ID;
  }

  // Blend toward flat average as the swell fades out (kills slice-color aliasing).
  col = mix(col, avgColor, 1. - lodBase);

  // Anisotropic glint — pale glare on light-facing wave faces, near the horizon,
  // peaked toward GLINT_DIR with an omnidirectional floor.
  vec2 viewAz = lenHoriz > 1e-4 ? dir.xz / lenHoriz : vec2(1., 0.);
  float azFactor = mix(GLINT_BASE_$ID, 1., smoothstep(GLINT_WIDTH_$ID, 1., dot(viewAz, GLINT_DIR_$ID)));
  float horizonBand = 1. - smoothstep(GLINT_HOR0_$ID, GLINT_HOR1_$ID, negDy);
  float faceSpec = pow(max(dot(normal, LIGHT_DIR_$ID), 0.), GLINT_SHARP_$ID);
  col += GLINT_COLOR_$ID * (GLINT_INTENSITY_$ID * azFactor * horizonBand * faceSpec);

  outColor = col * alpha;
  outAlpha = alpha;
}
