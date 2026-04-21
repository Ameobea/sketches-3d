precision highp float;

uniform float uTime;
uniform float uHeight;
uniform float uHorizonFadeStart;
uniform float uHorizonFadeEnd;

// Atmospheric tint — paint color is mixed toward uAtmoTintColor as -dir.y → 0,
// approximating distance reddening/darkening without a real optical-depth calc.
// Strength 0 disables the effect entirely.
uniform vec3 uAtmoTintColor;
uniform float uAtmoTintRange;
uniform float uAtmoTintStrength;

in vec3 vWorldPos;
out vec4 fragColor;
