uniform sampler2D inputBuffer;
#ifdef HAS_EMISSIVE_BUFFER
uniform sampler2D emissiveBuffer;
#endif
#ifdef HAS_EMISSIVE_BLOOM
uniform sampler2D emissiveBloomBuffer;
uniform float bloomIntensity;
#endif
uniform float gammaExponent;
varying vec2 vUv;

#if defined(HAS_FOG) || defined(SKY_BYPASS_TONEMAP)
uniform sampler2D depthBuffer;
#endif
#ifdef HAS_FOG
uniform mat4 projectionMatrixInverse;
uniform mat4 cameraWorldMatrix;
uniform vec3 fogCameraPos;
uniform vec3 fogPlayerPos;
uniform float curTimeSeconds;
#endif

#include <common>
#include <tonemapping_pars_fragment>

vec3 linearToSRGB(vec3 value) {
  return mix(
    value * 12.92,
    pow(clamp(value, 0.0, 1.0), vec3(1.0 / 2.4)) * 1.055 - 0.055,
    step(vec3(0.0031308), value)
  );
}

// Interleaved Gradient Noise (Jimenez et al. 2014) — blue-noise-like spectral
// properties, purely a function of screen position.
float ign(vec2 p) {
  return fract(52.9829189 * fract(dot(p, vec2(0.06711056, 0.00583715))));
}

// TPDF dither: sum of two decorrelated IGN samples gives a triangular
// distribution over [-1, 1], which has zero mean and lower quantization bias
// than rectangular (white-noise) dithering.
float tpdfDither(vec2 seed) {
  float a = ign(seed);
  float b = ign(seed + vec2(1.7269, 2.3891));
  return a + b - 1.0;
}

#ifdef HAS_FOG
// Reconstructs the world-space position of the fragment from the depth buffer.
// depth is the raw depth buffer value in [0, 1]; a value at or near 1.0 indicates
// the far plane (sky / no geometry) and should be handled in getFogEffect accordingly.
vec3 reconstructWorldPos(float depth, vec2 uv) {
  vec4 ndc = vec4(uv * 2.0 - 1.0, depth * 2.0 - 1.0, 1.0);
  vec4 viewPos = projectionMatrixInverse * ndc;
  viewPos /= viewPos.w;
  return (cameraWorldMatrix * viewPos).xyz;
}
#endif

void main() {
  vec4 color = texture2D(inputBuffer, vUv);

  // Sky-bypass detection: fragments at the far plane (depth ≈ 1) are treated as
  // sky / no geometry and skip the tone-mapping stage entirely, preserving the
  // color authored by the sky material. All downstream passes (sRGB encode,
  // emissive composite, bloom, gamma, dither) still run.
  bool bypassToneMap = false;
  #ifdef SKY_BYPASS_TONEMAP
    bypassToneMap = texture2D(depthBuffer, vUv).r >= 0.9999;
  #endif

  // Fog blend runs in linear space, before tone mapping, so the fog color is
  // authored in linear and participates correctly in the tone-mapping curve.
  #ifdef HAS_FOG
  {
    float depth = texture2D(depthBuffer, vUv).r;
    vec3 worldPos = reconstructWorldPos(depth, vUv);
    // getFogEffect is defined by the injected fogShader string.
    // Signature: vec4 getFogEffect(vec3 worldPos, vec3 cameraPos, vec3 playerPos,
    //                              float depth, float curTimeSeconds)
    // Returns vec4(fogColor.rgb, fogFactor) where fogFactor=0 is clear, 1 is full fog.
    vec4 fogResult = getFogEffect(worldPos, fogCameraPos, fogPlayerPos, depth, curTimeSeconds);
    color.rgb = mix(color.rgb, fogResult.rgb, fogResult.a);
  }
  #endif

  if (!bypassToneMap) {
    #if defined(TONE_MAPPING_ACES)
      color.rgb = ACESFilmicToneMapping(color.rgb);
    #elif defined(TONE_MAPPING_CINEON)
      color.rgb = CineonToneMapping(color.rgb);
    #elif defined(TONE_MAPPING_REINHARD)
      color.rgb = ReinhardToneMapping(color.rgb);
    #elif defined(TONE_MAPPING_AGX)
      color.rgb = AgXToneMapping(color.rgb);
    #elif defined(TONE_MAPPING_NEUTRAL)
      color.rgb = NeutralToneMapping(color.rgb);
    #else
      // 'none': apply exposure and hard-clamp
      color.rgb = min(color.rgb * toneMappingExposure, vec3(1.0));
    #endif
  }

  color.rgb = linearToSRGB(color.rgb);

#ifdef HAS_EMISSIVE_BUFFER
{
  vec4 emissive = texture2D(emissiveBuffer, vUv);
  // Composite emissive without AgX — preserves vivid saturated colors.
  vec3 emissiveSRGB = linearToSRGB(emissive.rgb);
  color.rgb = mix(color.rgb, emissiveSRGB, emissive.a);
}
#endif

#ifdef HAS_EMISSIVE_BLOOM
{
  // Additive bloom glow from the MipmapBlur of the emissive buffer.
  vec4 bloom = texture2D(emissiveBloomBuffer, vUv);
  color.rgb += linearToSRGB(bloom.rgb) * bloomIntensity;
}
#endif

  // User gamma adjustment in sRGB space. gammaExponent = 1/gamma, so 1.0 is identity.
  // Clamp before pow to avoid undefined behavior on values >1 from additive bloom.
  color.rgb = pow(clamp(color.rgb, 0.0, 1.0), vec3(gammaExponent));

  // Per-channel TPDF dither in sRGB space.
  // Decorrelate R/G/B by using different seed offsets so noise appears as
  // desaturated grain rather than coloured shimmer.
  vec2 coord = gl_FragCoord.xy;
  vec3 noise = vec3(
    tpdfDither(coord),
    tpdfDither(coord + vec2(47.3, 31.7)),
    tpdfDither(coord + vec2(97.1, 53.9))
  );

  // avoid dithering almost completely black regions to avoid obvious artifacts when
  // looking at large completely or very nearly black areas
  //
  // I feel like this might just work out to a step function, but idk
  float luma = dot(color.rgb, vec3(0.2126, 0.7152, 0.0722));
  float amplitude = (1.0 / 255.0) * smoothstep(0.0, 0.005, luma);

  color.rgb += noise * amplitude;

  gl_FragColor = color;
}
