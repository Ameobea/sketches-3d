uniform sampler2D emissiveBuffer;
uniform sampler2D emissiveDepthBuffer;
uniform mat4 projectionMatrixInverse;
uniform mat4 cameraWorldMatrix;
uniform vec3 fogCameraPos;
uniform vec3 fogPlayerPos;
uniform float curTimeSeconds;
varying vec2 vUv;

vec3 reconstructWorldPos(float depth, vec2 uv) {
  vec4 ndc = vec4(uv * 2.0 - 1.0, depth * 2.0 - 1.0, 1.0);
  vec4 viewPos = projectionMatrixInverse * ndc;
  viewPos /= viewPos.w;
  return (cameraWorldMatrix * viewPos).xyz;
}

void main() {
  vec4 emissive = texture2D(emissiveBuffer, vUv);
  float depth = texture2D(emissiveDepthBuffer, vUv).r;
  vec3 worldPos = reconstructWorldPos(depth, vUv);
  vec4 fogResult = getFogEffect(worldPos, fogCameraPos, fogPlayerPos, depth, curTimeSeconds);
  // Blend the fog color into RGB in linear space. Alpha is attenuated by (1 - fogFactor) so
  // fully-fogged pixels disappear from both the emissive composite AND the bloom source,
  // which eliminates bright halo sheen where bloom from un-fogged emissive overlaps close
  // geometry. Halo/non-mesh pixels pick up the main-scene depth (pre-blitted into emissiveRT),
  // so they fog consistently with the scene behind them.
  emissive.rgb = mix(emissive.rgb, fogResult.rgb, fogResult.a);
  emissive.a *= (1.0 - fogResult.a);
  gl_FragColor = emissive;
}
