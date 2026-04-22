// Ground sampler for the unified SkyStack shader. The caller must have already
// prepended the user paintShader source, which defines:
//
//   vec4 paintGround(vec2 uv, vec2 uvDeriv, vec3 dir, float invDist);
//
// This helper reproduces the old standalone GroundPlane mesh's shading: ray-
// plane intersection on y=0 to derive UV, analytic screen-space derivatives,
// atmospheric tint fade, and a horizon smoothstep on alpha. The output is
// written into the emissive channel only — the unified shader's `skyColor`
// for below-horizon fragments still comes from the gradient's HorizonMode
// (SolidBelow in the factory), matching what the old pipeline produced (the
// old GroundPlane mesh was registered as an emissive-bypass mesh, so its
// paint went only to emissiveRT, not to the tone-mapped color buffer).

uniform float uGroundHeight;
uniform float uGroundHorizonFadeStart;
uniform float uGroundHorizonFadeEnd;
uniform vec3 uGroundAtmoTintColor;
uniform float uGroundAtmoTintRange;
uniform float uGroundAtmoTintStrength;

// Out: groundEmissive = (tinted paint rgb, horizon-faded alpha). rgb is the
// non-premul paint color (matching the old GroundPlane output); the unified
// main() adds this to emissiveRGB/Alpha so FinalPass's
// `mix(scene, emissive.rgb, emissive.a)` composite reproduces the old
// emissive-bypass behavior pixel-for-pixel.
void sampleGround(vec3 dir, out vec4 groundEmissive) {
  // Above-horizon fragments contribute nothing (horizon-fade alpha is 0). Skip
  // the SDF/paint work for them, but keep a small epsilon margin so 2x2 quads
  // that straddle the horizon still have valid dFdx/dFdy for the fragments that
  // do contribute. The margin is roughly a couple of pixels of dir.y at typical
  // FOVs — enough that any fragment within a quad of a contributing one also
  // takes the full path.
  if (dir.y > 0.01) {
    groundEmissive = vec4(0.0);
    return;
  }

  // Clamp dir.y strictly negative so ray-plane intersection never blows up.
  // Fragments inside the epsilon band above the horizon still take this path
  // for derivative-quad coherence; their horizonAlpha is 0 so output is 0.
  float negDy = max(-dir.y, 1e-4);
  float t = uGroundHeight / negDy;
  vec2 uv = dir.xz * t;

  vec2 uvDeriv = vec2(
    length(vec2(dFdx(uv.x), dFdy(uv.x))),
    length(vec2(dFdx(uv.y), dFdy(uv.y)))
  );
  float invDist = -dir.y / uGroundHeight;

  vec4 paint = paintGround(uv, uvDeriv, dir, invDist);

  // Atmospheric tint — strengthens as -dir.y approaches 0. Strength 0 disables.
  float atmoT = (1.0 - smoothstep(0.0, uGroundAtmoTintRange, -dir.y)) * uGroundAtmoTintStrength;
  vec3 tinted = mix(paint.rgb, uGroundAtmoTintColor, clamp(atmoT, 0.0, 1.0));

  // Horizon fade — 0 at/above horizon, ramps to 1 below. Above-horizon
  // fragments get alpha=0 automatically (smoothstep on negative input is 0).
  float horizonAlpha = smoothstep(uGroundHorizonFadeStart, uGroundHorizonFadeEnd, -dir.y);

  groundEmissive = vec4(tinted, paint.a * horizonAlpha);
}
