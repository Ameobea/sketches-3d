// Dual Kawase downsample — 5 taps with half-texel bilinear trick.
// Ref: https://blog.frost.kiwi/dual-kawase/
//
// Each diagonal tap sits at the corner where 4 source texels meet, so the
// GPU's bilinear unit blends all four for free.  5 texture fetches effectively
// sample a 4×4 region with a smooth tent-like weight distribution:
//
//   1  2  1
//   2  4  2   / 16
//   1  2  1
//
// The center tap gets weight 4 (dominant), diagonals get weight 1 each
// (but each one covers 4 texels via bilinear → effective weight 1 per texel
// in the corners).  Total weight = 4 + 4×1 = 8.

precision highp float;

uniform sampler2D tInput;
uniform vec2 uHalfTexel;

varying vec2 vUv;

void main() {
  vec4 sum  = texture2D(tInput, vUv) * 4.0;
  sum += texture2D(tInput, vUv + vec2(-uHalfTexel.x,  uHalfTexel.y));
  sum += texture2D(tInput, vUv + vec2( uHalfTexel.x,  uHalfTexel.y));
  sum += texture2D(tInput, vUv + vec2(-uHalfTexel.x, -uHalfTexel.y));
  sum += texture2D(tInput, vUv + vec2( uHalfTexel.x, -uHalfTexel.y));
  gl_FragColor = sum * 0.125;
}
