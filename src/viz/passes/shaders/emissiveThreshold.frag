uniform sampler2D tInput;
uniform float threshold;
uniform float smoothing;
varying vec2 vUv;

void main() {
  vec4 color = texture2D(tInput, vUv);
  float luma = dot(color.rgb, vec3(0.2126, 0.7152, 0.0722));
  float contribution = smoothstep(threshold - smoothing * 0.5, threshold + smoothing * 0.5, luma);
  gl_FragColor = vec4(color.rgb * contribution, color.a * contribution);
}
