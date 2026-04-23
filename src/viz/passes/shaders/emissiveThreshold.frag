uniform sampler2D tInput;
varying vec2 vUv;

#ifdef HAS_THRESHOLD
uniform float threshold;
uniform float smoothing;
// When > 0, selects the UE4-style soft-knee gate.
uniform float softKnee;
#endif

#ifdef HAS_FOG
uniform sampler2D depthBuffer;
uniform mat4 projectionMatrixInverse;
uniform mat4 cameraWorldMatrix;
uniform vec3 fogCameraPos;
uniform vec3 fogPlayerPos;
uniform float curTimeSeconds;

vec3 reconstructWorldPos(float depth, vec2 uv) {
  vec4 ndc = vec4(uv * 2.0 - 1.0, depth * 2.0 - 1.0, 1.0);
  vec4 viewPos = projectionMatrixInverse * ndc;
  viewPos /= viewPos.w;
  return (cameraWorldMatrix * viewPos).xyz;
}
#endif

void main() {
  vec4 color = texture2D(tInput, vUv);

#ifdef HAS_FOG
  if (color.a > 0.0) {
    float depth = texture2D(depthBuffer, vUv).r;
    vec3 worldPos = reconstructWorldPos(depth, vUv);
    vec4 fogResult = getFogEffect(worldPos, fogCameraPos, fogPlayerPos, depth, curTimeSeconds);
    color.rgb = mix(color.rgb, fogResult.rgb, fogResult.a);
    color.a *= (1.0 - fogResult.a);
  }
#endif

#ifdef HAS_THRESHOLD
  float luma = dot(color.rgb, vec3(0.2126, 0.7152, 0.0722));

  float contribution;
  if (softKnee > 0.0) {
    // UE4-style soft-knee
    float soft = max(luma - (threshold - softKnee), 0.0);
    soft = min(soft, 2.0 * softKnee);
    soft = soft * soft * 0.25 / max(softKnee, 1e-5);
    float linear = max(luma - threshold, 0.0);
    contribution = max(soft, linear) / max(luma, 1e-5);
  } else {
    contribution = smoothstep(threshold - smoothing * 0.5, threshold + smoothing * 0.5, luma);
  }

  color *= contribution;
#endif

  gl_FragColor = color;
}
