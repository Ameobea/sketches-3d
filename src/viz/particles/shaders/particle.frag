in vec2 vUv;
in vec4 vColor;
in float vSeed;
in float vAge;
in vec4 vFog;
in float vViewZ;
#ifdef SOFT_DEPTH
flat in float vSoftRadius;
#endif

layout(location = 0) out vec4 outColor;
#ifdef HAS_EMISSIVE_RT
layout(location = 1) out vec4 outEmissive;
#endif

uniform float pTime;
#ifdef SOFT_DEPTH
uniform highp sampler2D pSceneDepth;
uniform vec2 pNearFar;
#endif

//__CUSTOM_UNIFORMS__

//__SPRITE__

void main() {
  float a = vColor.a;
#ifdef SOFT_DEPTH
  #ifdef RENDER_SCALE_INV
  ivec2 depthTexel = ivec2(gl_FragCoord.xy * RENDER_SCALE_INV);
  #else
  ivec2 depthTexel = ivec2(gl_FragCoord.xy);
  #endif
  float sceneDepth = texelFetch(pSceneDepth, depthTexel, 0).r;
  float sceneZ = pNearFar.x * pNearFar.y / (pNearFar.y - sceneDepth * (pNearFar.y - pNearFar.x));
  vec2 d = vUv * 2.0 - 1.0;
  float h = vSoftRadius * sqrt(max(1.0 - dot(d, d), 1e-3));
  a *= clamp((min(sceneZ, vViewZ + h) - max(vViewZ - h, pNearFar.x)) / (2.0 * h), 0.0, 1.0);
  if (a <= 0.002) discard;
#endif
  vec4 s = sprite(vUv, vSeed, vAge);
  a = clamp(a * s.a, 0.0, 1.0);
  if (a <= 0.002) discard;
  vec3 rgb = s.rgb * vColor.rgb;
#ifdef OUTPUT_EMISSIVE
  outColor = vec4(0.0, 0.0, 0.0, a);
  outEmissive = vec4(rgb * a, a);
#else
  #ifdef HAS_FOG
  rgb = mix(rgb, vFog.rgb, vFog.a);
  #endif
  outColor = vec4(rgb * a, a);
  #ifdef HAS_EMISSIVE_RT
  outEmissive = vec4(0.0);
  #endif
#endif
}
