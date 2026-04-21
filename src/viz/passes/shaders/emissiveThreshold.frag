uniform sampler2D tInput;
uniform float threshold;
uniform float smoothing;
// When > 0, selects the UE4-style soft-knee gate. The smoothstep path and the
// soft-knee path are mutually exclusive — scenes pick whichever shape reads better.
uniform float softKnee;
varying vec2 vUv;

void main() {
  vec4 color = texture2D(tInput, vUv);
  float luma = dot(color.rgb, vec3(0.2126, 0.7152, 0.0722));

  float contribution;
  if (softKnee > 0.0) {
    // UE4 BloomSetup soft-knee: quadratic ramp of width `2*softKnee` centered on
    // `threshold`, then linear (subtractive) above. C1-continuous, no hard cutoff.
    // The ratio form (max(quad, linear) / luma) asymptotes to 1 for bright pixels
    // and fades smoothly to 0 below `threshold - softKnee`, so small flickers in
    // low-luma regions produce tiny proportional contributions instead of binary pops.
    float soft = max(luma - (threshold - softKnee), 0.0);
    soft = min(soft, 2.0 * softKnee);
    soft = soft * soft * 0.25 / max(softKnee, 1e-5);
    float linear = max(luma - threshold, 0.0);
    contribution = max(soft, linear) / max(luma, 1e-5);
  } else {
    contribution = smoothstep(threshold - smoothing * 0.5, threshold + smoothing * 0.5, luma);
  }

  gl_FragColor = vec4(color.rgb * contribution, color.a * contribution);
}
