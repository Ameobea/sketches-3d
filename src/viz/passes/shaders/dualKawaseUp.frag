// Dual Kawase upsample — 8 taps in a cross+diagonal pattern.
// Ref: https://blog.frost.kiwi/dual-kawase/
//
// Axis-aligned taps at ±1 texel (weight 1) and diagonal taps at ±0.5 texel
// (weight 2, bilinear blends 4 texels each).  Total weight = 4×1 + 4×2 = 12.
//
// The result is blended with the corresponding downsample level:
//   output = mix(downsampleAtThisLevel, upsampledFromBelow, uRadius)
// This controls how much of the blur from deeper (wider) levels propagates
// up.  Higher radius = broader glow; lower = tighter, more per-feature.

precision highp float;

uniform sampler2D tInput;
uniform sampler2D tDownsample;
uniform vec2 uHalfTexel;
uniform float uRadius;

varying vec2 vUv;

void main() {
  vec2 hp = uHalfTexel;

  vec4 sum  = texture2D(tInput, vUv + vec2(-hp.x * 2.0,  0.0));
  sum += texture2D(tInput, vUv + vec2(-hp.x,  hp.y)) * 2.0;
  sum += texture2D(tInput, vUv + vec2( 0.0,   hp.y * 2.0));
  sum += texture2D(tInput, vUv + vec2( hp.x,  hp.y)) * 2.0;
  sum += texture2D(tInput, vUv + vec2( hp.x * 2.0,  0.0));
  sum += texture2D(tInput, vUv + vec2( hp.x, -hp.y)) * 2.0;
  sum += texture2D(tInput, vUv + vec2( 0.0,  -hp.y * 2.0));
  sum += texture2D(tInput, vUv + vec2(-hp.x, -hp.y)) * 2.0;
  vec4 upsampled = sum / 12.0;

  vec4 base = texture2D(tDownsample, vUv);
  gl_FragColor = mix(base, upsampled, uRadius);
}
