uniform float uGroundHeight_$ID;
uniform float uGroundHorizonFadeStart_$ID;
uniform float uGroundHorizonFadeEnd_$ID;
uniform vec3 uGroundAtmoTintColor_$ID;
uniform float uGroundAtmoTintRange_$ID;
uniform float uGroundAtmoTintStrength_$ID;

// paintGround_$ID is defined by the user-provided paint shader source (with
// `$ID` resolved to the layer's id), so each ground instance can ship its own
// paint function without collisions.
//   vec4 paintGround_$ID(vec2 uv, vec2 uvDeriv, vec3 dir, float invDist);
//
// Output is written to the emissive channel only (bypass tone mapping); the
// caller multiplies by horizon fade and atmospheric tint.
void sampleGround_$ID(vec3 dir, out vec4 groundEmissive) {
  if (dir.y > 0.01) {
    groundEmissive = vec4(0.0);
    return;
  }

  float negDy = max(-dir.y, 1e-4);
  float t = uGroundHeight_$ID / negDy;
  vec2 uv = dir.xz * t;

  vec2 uvDeriv = vec2(
    length(vec2(dFdx(uv.x), dFdy(uv.x))),
    length(vec2(dFdx(uv.y), dFdy(uv.y)))
  );
  float invDist = -dir.y / uGroundHeight_$ID;

  vec4 paint = paintGround_$ID(uv, uvDeriv, dir, invDist);

  float atmoT = (1.0 - smoothstep(0.0, uGroundAtmoTintRange_$ID, -dir.y)) * uGroundAtmoTintStrength_$ID;
  vec3 tinted = mix(paint.rgb, uGroundAtmoTintColor_$ID, clamp(atmoT, 0.0, 1.0));

  float horizonAlpha = smoothstep(uGroundHorizonFadeStart_$ID, uGroundHorizonFadeEnd_$ID, -dir.y);

  groundEmissive = vec4(tinted, paint.a * horizonAlpha);
}
