// User-provided `paintGround` is prepended to this shader and must have signature:
//   vec4 paintGround(vec2 uv, vec2 uvDeriv, vec3 dir, float invDist)
//
// uv       — world-space XZ on the virtual ground plane (direction-space projection)
// uvDeriv  — per-axis length of screen-space derivatives of `uv`, in uv units per pixel
// dir      — view direction (ray from camera through fragment); dir.y < 0 below horizon
// invDist  — -dir.y / uHeight in [0, 1/uHeight]; 0 at horizon, max looking straight down
//
// Returns (rgb, alpha). Alpha is multiplied by the horizon fade before output.
// Noise helpers from `noise.frag` are available (hash, noise, fbm).

void main() {
  vec3 dir = normalize(vWorldPos - cameraPosition);

  // Clamp dir.y strictly negative so ray-plane intersection never blows up. Above-horizon
  // fragments still run the shader so dFdx/dFdy stay well-defined across 2x2 quads; they
  // produce alpha=0 via the horizon smoothstep below rather than discarding.
  float negDy = max(-dir.y, 1e-4);
  float t = uHeight / negDy;
  vec2 uv = dir.xz * t;

  // Screen-space derivatives of `uv` — feature paint code can use these for correct
  // per-pixel AA width (Bgolus trick) and for fading features past the Nyquist limit.
  vec2 uvDeriv = vec2(length(vec2(dFdx(uv.x), dFdy(uv.x))), length(vec2(dFdx(uv.y), dFdy(uv.y))));
  float invDist = -dir.y / uHeight;

  vec4 paint = paintGround(uv, uvDeriv, dir, invDist);

  // Horizon fade: 0 at/above horizon, ramping to 1 in the band below. Smoothstep over
  // negative inputs returns 0, so above-horizon fragments naturally get alpha=0.
  float horizonAlpha = smoothstep(uHorizonFadeStart, uHorizonFadeEnd, -dir.y);

  fragColor = vec4(paint.rgb, paint.a * horizonAlpha);
}
