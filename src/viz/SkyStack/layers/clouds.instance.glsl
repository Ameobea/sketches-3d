uniform vec3 uHazeColor_$ID;
uniform vec3 uHazeHighColor_$ID;
uniform float uHazeIntensity_$ID;
uniform float uHazeCenter_$ID;
uniform float uHazeWidth_$ID;
uniform float uHazeSharpness_$ID;
uniform vec3 uHazeScale_$ID;
uniform vec3 uHazeSpeed_$ID;
uniform int uHazeOctaves_$ID;
uniform float uHazeLacunarity_$ID;
uniform float uHazeGain_$ID;
uniform float uHazeBias_$ID;
uniform float uHazePow_$ID;

// Returns (rgb, density). rgb is NOT premultiplied.
vec4 sampleHaze_$ID(vec3 dir, float elev) {
  float w = max(uHazeWidth_$ID, 1e-4);
  float shape = 1.0 - smoothstep(0.0, w, abs(elev - uHazeCenter_$ID));
  if (shape <= 0.0) {
    return vec4(0.0);
  }

  vec3 p = dir * uHazeScale_$ID + uHazeSpeed_$ID * uTime;
  float f = skyFbm(p, uHazeOctaves_$ID, uHazeLacunarity_$ID, uHazeGain_$ID);
  f = clamp(f + uHazeBias_$ID, 0.0, 1.0);
  f = pow(f, max(uHazePow_$ID, 1e-3));
  float edge = clamp(uHazeSharpness_$ID, 1e-3, 0.5);
  float density = smoothstep(0.5 - edge, 0.5 + edge, f);
  float a = clamp(shape * density * uHazeIntensity_$ID, 0.0, 1.0);

  // Oklab mix on the shaped-but-pre-threshold value so the color gradient
  // spans the full range of fBm values that contribute.
  vec3 hazeCol = oklabMix(uHazeColor_$ID, uHazeHighColor_$ID, clamp(f, 0.0, 1.0));
  return vec4(hazeCol, a);
}
