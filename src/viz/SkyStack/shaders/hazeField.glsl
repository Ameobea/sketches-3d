// Shared haze-field sampler. One cloud band per material instance — the color
// sub-layer paints the haze into the tone-mapped buffer, the attenuator
// sub-layer uses the same density to dim emissive content behind the cloud.
// Both materials include this file so they agree pixel-for-pixel on density.
//
// Depends on prelude's `uTime`, `oklabMix`, and on `noise(vec3)` from
// noise.frag. Include noise.frag, prelude, then this file.

// MAX_HAZE_OCTAVES is emitted as a #define by the unified shader builder,
// sized to the maximum octave count actually used by any cloud band in this
// material. The per-cloud `uHazeXxxOctaves` uniform still gates the runtime
// loop count below MAX, but the loop bound is a literal constant so the
// driver can unroll.

uniform vec3 uHazeColor;
// Cloud color crossfades from uHazeColor (thin wisps) to uHazeHighColor (dense
// cores) as a function of the shaped fBm value. Set both the same for a
// single-hue cloud.
uniform vec3 uHazeHighColor;
uniform float uHazeIntensity;
uniform float uHazeCenter;
uniform float uHazeWidth;
uniform float uHazeSharpness;
uniform vec3 uHazeScale;
uniform vec3 uHazeSpeed;
uniform int uHazeOctaves;
uniform float uHazeLacunarity;
uniform float uHazeGain;
uniform float uHazeBias;
uniform float uHazePow;

// Amplitude-normalized fBm so the output stays in [0, 1] regardless of octave
// count / gain. Otherwise dialing octaves shifts overall coverage.
float skyFbm(vec3 x, int octaves, float lacunarity, float gain) {
  float v = 0.0;
  float a = 1.0;
  float norm = 0.0;
  vec3 shift = vec3(100.0);
  for (int i = 0; i < MAX_HAZE_OCTAVES; i++) {
    if (i >= octaves) {
      break;
    }
    v += a * noise(x);
    norm += a;
    x = x * lacunarity + shift;
    a *= gain;
  }
  return v / max(norm, 1e-6);
}

// Returns (rgb, density). Density in [0, 1] is the cloud coverage for alpha-
// over compositing (rgb is already the cloud color, NOT premultiplied).
vec4 sampleHaze(vec3 dir, float elev) {
  float w = max(uHazeWidth, 1e-4);
  float shape = 1.0 - smoothstep(0.0, w, abs(elev - uHazeCenter));
  if (shape <= 0.0) {
    return vec4(0.0);
  }

  vec3 p = dir * uHazeScale + uHazeSpeed * uTime;
  float f = skyFbm(p, uHazeOctaves, uHazeLacunarity, uHazeGain);
  f = clamp(f + uHazeBias, 0.0, 1.0);
  f = pow(f, max(uHazePow, 1e-3));
  float edge = clamp(uHazeSharpness, 1e-3, 0.5);
  float density = smoothstep(0.5 - edge, 0.5 + edge, f);
  float a = clamp(shape * density * uHazeIntensity, 0.0, 1.0);

  // Oklab mix on the shaped-but-pre-threshold value so the color gradient
  // spans the full range of fBm values that contribute, not just the edge.
  vec3 hazeCol = oklabMix(uHazeColor, uHazeHighColor, clamp(f, 0.0, 1.0));
  return vec4(hazeCol, a);
}
