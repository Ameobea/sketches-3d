in float aSeed;

uniform float pTime;
uniform vec3 pPlayerPos;
uniform vec3 pBoxSize;
uniform vec3 pBoxCenter;
uniform float pBoxEdgeFade;
uniform float pLifetime;
uniform float pStretch;
uniform vec2 pPxRange;
uniform float pViewportHeight;
uniform vec3 pAmbient;
uniform vec3 pSunColor;
uniform sampler2D pSunShadowMap;
uniform mat4 pSunShadowMatrix;
uniform float pSunShadowOn;

out vec2 vUv;
out vec4 vColor;
out float vSeed;
out float vAge;
out vec4 vFog;
out float vViewZ;

//__CUSTOM_UNIFORMS__

float pHash(uint x) {
  x ^= x >> 16u;
  x *= 0x7feb352du;
  x ^= x >> 15u;
  x *= 0x846ca68bu;
  x ^= x >> 16u;
  return float(x) * (1.0 / 4294967296.0);
}
vec3 pHash3(uint x) { return vec3(pHash(x), pHash(x + 0x9e3779b9u), pHash(x + 0x3c6ef372u)); }

struct Particle {
  vec3 pos;
  float size;
  float rot;
  vec4 color;
};

//__FOG_SHADER__

//__MOTION__

#include <packing>
float pSunVisibility(vec3 worldPos) {
  vec4 sc = pSunShadowMatrix * vec4(worldPos, 1.0);
  vec3 c = (sc.xyz / sc.w) * 0.5 + 0.5;
  if (pSunShadowOn < 0.5 || any(lessThan(c, vec3(0.0))) || any(greaterThan(c, vec3(1.0)))) return 1.0;
  return step(c.z - 0.002, unpackRGBAToDepth(texture2D(pSunShadowMap, c.xy)));
}

// Metres of free space between `wpos` and the first surface a HeightField saw past its plane.
float pClearance(highp sampler2D map, mat4 m, float depth, float soften, vec3 wpos, uint id) {
  vec2 jitter = (vec2(pHash(id ^ 0x68bc21ebu), pHash(id ^ 0x02e5be93u)) - 0.5) * soften;
  vec4 c = m * vec4(wpos.x + jitter.x, wpos.y, wpos.z + jitter.y, 1.0);
  vec2 uv = c.xy * 0.5 + 0.5;
  if (any(lessThan(uv, vec2(0.0))) || any(greaterThan(uv, vec2(1.0)))) return 1e6;
  return texture(map, uv).r - (c.z * 0.5 + 0.5) * depth;
}

void main() {
  uint id = uint(aSeed);
  float seed = pHash(id);
  float age;
  uint cycle = 0u;
  if (pLifetime > 0.0) {
    float tt = pTime + seed * pLifetime;
    cycle = uint(tt / pLifetime);
    age = tt - float(cycle) * pLifetime;
  } else {
    age = pTime + seed * 1024.0;
  }
  uint cid = id * 7919u + cycle * 104729u;
  vec3 u = pHash3(cid);
  //__DISTRIBUTE__
  vec3 origin = (u - 0.5) * pBoxSize;
  vec4 params = vec4(pHash3(cid + 15485863u), pHash(cid + 32452843u));
  Particle p = motion(seed, age, origin, params);

  vec3 halfBox = pBoxSize * 0.5;
  vec3 rel = mod(p.pos - pBoxCenter + halfBox, pBoxSize) - halfBox;
  vec3 wpos = pBoxCenter + rel;
  vec3 edge = smoothstep(vec3(0.0), vec3(pBoxEdgeFade), halfBox - abs(rel));
  float alpha = p.color.a * edge.x * edge.y * edge.z;
  float size = p.size;
  float dist = distance(wpos, cameraPosition);
  //__GATES__

  vec3 rgb = p.color.rgb;
  //__LIGHT__

  vec4 viewPos = viewMatrix * vec4(wpos, 1.0);
  float px = size * projectionMatrix[1][1] * pViewportHeight * 0.5 / max(-viewPos.z, 1e-3);
  if (px > 0.0 && px < pPxRange.x) {
    float k = pPxRange.x / px;
    size *= k;
    alpha /= k * k;
  } else if (px > pPxRange.y) {
    size *= pPxRange.y / px;
  }
  if (alpha <= 0.002) size = 0.0;

  vec3 camRight = vec3(viewMatrix[0][0], viewMatrix[1][0], viewMatrix[2][0]);
  vec3 camUp = vec3(viewMatrix[0][1], viewMatrix[1][1], viewMatrix[2][1]);
  vec2 corner = position.xy;
  vec3 offset;
  //__ORIENT__

  vec4 clip = projectionMatrix * (viewMatrix * vec4(wpos + offset, 1.0));
  gl_Position = clip;
  vUv = uv;
  vSeed = seed;
  vAge = age;
  vColor = vec4(rgb, alpha);
  vViewZ = clip.w;
#ifdef HAS_FOG
  vFog = getFogEffect(wpos, cameraPosition, pPlayerPos, clip.z / clip.w * 0.5 + 0.5, pTime);
#else
  vFog = vec4(0.0);
#endif
}
