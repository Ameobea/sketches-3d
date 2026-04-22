// Shared gradient + bands evaluator. Consumed by the gradient layer's main
// shader and by other layers that need to sample the sky color at a given
// elevation (e.g. building silhouettes, which darken the gradient).
//
// Relies on prelude-provided `oklabMix` and `uTime`. Do not declare a `main()`
// here — include this alongside a layer entry shader that does.
//
// `STOP_COUNT` and `BAND_COUNT` are emitted as #defines by the unified shader
// builder based on the actual configured count. They drive both the uniform-
// array sizes (sized to the exact count, no slack) and the loop bounds (which
// are literal constants the driver can unroll). At least one stop is required
// — the SkyStack constructor enforces stops.length >= 1.

#define HORIZON_MODE_SOLID  0
#define HORIZON_MODE_MIRROR 1
#define HORIZON_MODE_EXTEND 2

uniform float uStopPositions[STOP_COUNT];
uniform vec3 uStopColors[STOP_COUNT];

uniform int uHorizonMode;
uniform vec3 uBelowColor;
// uHorizonBlend is declared in skyUnified.prelude.frag — shared with layers
// that don't include this file (e.g. clouds).

#if BAND_COUNT > 0
uniform float uBandCenters[BAND_COUNT];
uniform float uBandWidths[BAND_COUNT];
uniform vec3 uBandColors[BAND_COUNT];
uniform float uBandIntensities[BAND_COUNT];
uniform float uBandFadeRates[BAND_COUNT];
uniform float uBandFadePhases[BAND_COUNT];
#endif

// Gradient-stop interpolation in Oklab space. `t` is in [0, 1] along the
// authored stop positions. STOP_COUNT >= 1 is guaranteed by the constructor.
vec3 evalGradientStops(float t) {
  if (t <= uStopPositions[0]) {
    return uStopColors[0];
  }
  for (int i = 1; i < STOP_COUNT; i++) {
    float p1 = uStopPositions[i];
    if (t <= p1) {
      float p0 = uStopPositions[i - 1];
      float f = clamp((t - p0) / max(p1 - p0, 1e-6), 0.0, 1.0);
      return oklabMix(uStopColors[i - 1], uStopColors[i], f);
    }
  }
  return uStopColors[STOP_COUNT - 1];
}

// Full gradient color at a given elevation, including horizon mode handling
// and the horizon-blend smoothstep between above- and below-horizon colors.
// Bands are NOT added — call `evalBands()` separately if they're wanted.
vec3 evalGradient(float elev) {
  float tAbove = clamp(elev, 0.0, 1.0);
  vec3 aboveColor = evalGradientStops(tAbove);

  vec3 belowColor;
  if (uHorizonMode == HORIZON_MODE_MIRROR) {
    belowColor = evalGradientStops(clamp(-elev, 0.0, 1.0));
  } else if (uHorizonMode == HORIZON_MODE_EXTEND) {
    belowColor = evalGradientStops(0.0);
  } else {
    belowColor = uBelowColor;
  }

  float hb = smoothstep(-uHorizonBlend, uHorizonBlend, elev);
  return mix(belowColor, aboveColor, hb);
}

// Additive bands — soft Gaussian-shaped bumps at configured elevations,
// damped toward the poles to avoid ringing artifacts at the zenith/nadir.
vec3 evalBands(float elev, float cosElev) {
#if BAND_COUNT == 0
  return vec3(0.0);
#else
  vec3 sum = vec3(0.0);
  float poleDamp = smoothstep(0.0, 0.2, cosElev);
  for (int i = 0; i < BAND_COUNT; i++) {
    float w = max(uBandWidths[i], 1e-4);
    float shape = 1.0 - smoothstep(0.0, w, abs(elev - uBandCenters[i]));
    float fade = uBandFadeRates[i] == 0.0
      ? 1.0
      : (0.5 + 0.5 * sin(uTime * uBandFadeRates[i] + uBandFadePhases[i]));
    sum += uBandColors[i] * (shape * uBandIntensities[i] * fade * poleDamp);
  }
  return sum;
#endif
}
