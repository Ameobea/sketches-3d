// Prelude for the unified SkyStack fragment shader. Declares the two MRT
// output slots + shared uniforms, defines the geometry helpers (skyViewDir,
// skyCoords, discardIfOccluded), Oklab color math, and the front-to-back
// compositor (accumulators + `accumulate()`).
//
// MRT layout:
//   location 0 → tone-mapped path (blitted into composer inputBuffer).
//   location 1 → emissive path (blitted into emissiveRT, bypass-tone-map).
//
// Compositor model: layers run in front-to-back order, each emitting one
// `accumulate(color, emissive, alpha, emissiveAlpha)` call (color and the
// alpha-blend channel are PRE-MULTIPLIED). Each layer's contribution is
// weighted by the remaining `(1 - accumAlpha)` so a layer that outputs
// alpha=1 fully occludes everything behind it. The compose stage wraps each
// layer in `if (accumAlpha < SKY_SATURATION_ALPHA) { ... }` so saturation
// short-circuits the rest of the stack — that's where the "any opaque layer
// auto-blocks the back" behavior comes from. No layer needs to know about
// any other layer's existence.

precision highp float;

uniform float uTime;
uniform float uHorizonOffset;
uniform float uHorizonBlend;
uniform mat4 uProjectionMatrixInverse;
uniform mat4 uCameraWorldMatrix;
uniform sampler2D uSceneDepth;

in vec2 vUv;
layout (location = 0) out vec4 oColor;
layout (location = 1) out vec4 oEmissive;

const float PI = 3.141592653589793;
const float TWO_PI = 6.283185307179586;
const float HALF_PI = 1.5707963267948966;

vec3 skyViewDir() {
  vec4 ndc = vec4(vUv * 2.0 - 1.0, 1.0, 1.0);
  vec4 viewPos = uProjectionMatrixInverse * ndc;
  viewPos /= viewPos.w;
  return normalize((uCameraWorldMatrix * vec4(viewPos.xyz, 0.0)).xyz);
}

void skyCoords(vec3 dir, out float elev, out float azimuth, out float cosElev) {
  float dy = clamp(dir.y, -1.0, 1.0);
  elev = asin(dy) / HALF_PI - uHorizonOffset;
  cosElev = sqrt(max(1.0 - dy * dy, 0.0));
  azimuth = atan(dir.z, dir.x);
}

void discardIfOccluded() {
  float depth = texture(uSceneDepth, vUv).r;
  if (depth < 0.9999)
    discard;
}

// Oklab perceptually-uniform color mixing.
// Input/output: linear RGB (no gamma). Ref: https://bottosson.github.io/posts/oklab/
vec3 rgbToOklab(vec3 c) {
  float l = 0.4122214708 * c.r + 0.5363325363 * c.g + 0.0514459929 * c.b;
  float m = 0.2119034982 * c.r + 0.6806995451 * c.g + 0.1073969566 * c.b;
  float s = 0.0883024619 * c.r + 0.2817188376 * c.g + 0.6299787005 * c.b;
  float l_ = pow(max(l, 0.0), 1.0 / 3.0);
  float m_ = pow(max(m, 0.0), 1.0 / 3.0);
  float s_ = pow(max(s, 0.0), 1.0 / 3.0);
  return vec3(0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_, 1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_, 0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_);
}
vec3 oklabToRgb(vec3 c) {
  float l_ = c.x + 0.3963377774 * c.y + 0.2158037573 * c.z;
  float m_ = c.x - 0.1055613458 * c.y - 0.0638541728 * c.z;
  float s_ = c.x - 0.0894841775 * c.y - 1.2914855480 * c.z;
  return vec3(4.0767416621 * (l_ * l_ * l_) - 3.3077115913 * (m_ * m_ * m_) + 0.2309699292 * (s_ * s_ * s_), -1.2684380046 * (l_ * l_ * l_) + 2.6097574011 * (m_ * m_ * m_) - 0.3413193965 * (s_ * s_ * s_), -0.0041960863 * (l_ * l_ * l_) - 0.7034186147 * (m_ * m_ * m_) + 1.6956082560 * (s_ * s_ * s_));
}
vec3 oklabMix(vec3 a, vec3 b, float t) {
  return oklabToRgb(mix(rgbToOklab(a), rgbToOklab(b), t));
}

// === Front-to-back compositor ===
//
// File-scope globals — per-fragment state, default-initialized to zero each
// invocation. Layer bodies update these via `accumulate()`.

vec3 accumSkyColor = vec3(0.0);
vec3 accumEmissive = vec3(0.0);
float accumAlpha = 0.0;
float accumEmissiveAlpha = 0.0;

// Saturation cutoff — once accumAlpha exceeds this, subsequent layer bodies
// are skipped (the contribution would be < 0.001 weight anyway). 0.999 leaves
// a small "wedge" of the back layer mathematically present; visually
// indistinguishable, but it's a single tunable knob if anyone needs it.
#define SKY_SATURATION_ALPHA 0.999

// `color` and `emissive` are PRE-multiplied by their per-fragment blend
// weight at the call site (e.g. clouds pass `haze.rgb * haze.a`). The
// compositor weighting `(1 - accumAlpha)` is applied here, so the call site
// only needs to think about the layer's own blend math.
void accumulate(vec3 color, vec3 emissive, float alpha, float emissiveAlpha) {
  float w = 1.0 - accumAlpha;
  accumSkyColor      += w * color;
  accumEmissive      += w * emissive;
  accumAlpha         += w * alpha;
  accumEmissiveAlpha += w * emissiveAlpha;
}
