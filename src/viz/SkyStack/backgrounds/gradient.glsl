#define HORIZON_MODE_SOLID_$ID 0
#define HORIZON_MODE_MIRROR_$ID 1
#define HORIZON_MODE_EXTEND_$ID 2

uniform int uHorizonMode_$ID;
uniform vec3 uBelowColor_$ID;

#if BAND_COUNT_$ID > 0
uniform float uBandCenters_$ID[BAND_COUNT_$ID];
uniform float uBandWidths_$ID[BAND_COUNT_$ID];
uniform vec3 uBandColors_$ID[BAND_COUNT_$ID];
uniform float uBandIntensities_$ID[BAND_COUNT_$ID];
uniform float uBandFadeRates_$ID[BAND_COUNT_$ID];
uniform float uBandFadePhases_$ID[BAND_COUNT_$ID];
#endif

// GRADIENT_LUT_$ID is a precomputed Oklab-interpolated sample table baked at
// factory time. Adjacent entries are perceptually close, so the per-fragment
// RGB lerp below is visually indistinguishable from live Oklab math while
// costing zero cube roots.
vec3 sampleGradientLut_$ID(float t) {
  float f = clamp(t, 0.0, 1.0) * float(LUT_SIZE_$ID - 1);
  int i0 = int(f);
  int i1 = min(i0 + 1, LUT_SIZE_$ID - 1);
  float frac = f - float(i0);
  return mix(GRADIENT_LUT_$ID[i0], GRADIENT_LUT_$ID[i1], frac);
}

// Full gradient color at a given elevation, including horizon mode + the
// horizon-smoothstep between above- and below-horizon colors. `hb` is the
// compositor-scope horizonBlend. Bands are NOT added — call `evalBands_$ID()`
// separately if wanted.
vec3 evalGradient_$ID(float elev, float hb) {
  vec3 aboveColor = sampleGradientLut_$ID(clamp(elev, 0.0, 1.0));

  vec3 belowColor;
  if (uHorizonMode_$ID == HORIZON_MODE_MIRROR_$ID) {
    belowColor = sampleGradientLut_$ID(clamp(-elev, 0.0, 1.0));
  } else if (uHorizonMode_$ID == HORIZON_MODE_EXTEND_$ID) {
    belowColor = GRADIENT_LUT_$ID[0];
  } else {
    belowColor = uBelowColor_$ID;
  }

  return mix(belowColor, aboveColor, hb);
}

// Additive Gaussian-shaped bands, pole-damped to avoid zenith/nadir ringing.
vec3 evalBands_$ID(float elev, float cosElev) {
#if BAND_COUNT_$ID == 0
  return vec3(0.0);
#else
  vec3 sum = vec3(0.0);
  float poleDamp = smoothstep(0.0, 0.2, cosElev);
  for (int i = 0; i < BAND_COUNT_$ID; i++) {
    float w = max(uBandWidths_$ID[i], 1e-4);
    float shape = 1.0 - smoothstep(0.0, w, abs(elev - uBandCenters_$ID[i]));
    float fade = uBandFadeRates_$ID[i] == 0.0
      ? 1.0
      : (0.5 + 0.5 * sin(uTime * uBandFadeRates_$ID[i] + uBandFadePhases_$ID[i]));
    sum += uBandColors_$ID[i] * (shape * uBandIntensities_$ID[i] * fade * poleDamp);
  }
  return sum;
#endif
}
