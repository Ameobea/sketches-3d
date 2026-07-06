// Relief normal: ∇carve = (dcarve/dd)·∇d, with ∇d from a 2-tap central difference
// on the shape SDF (uniform across shapes, cf. pit_grid) and analytic for the
// 1-D groove. The branch whose carve wins the height max() supplies the gradient.
vec3 getPomNormal(vec3 pos, vec3 N, float depth, float t, float aa_) {
  vec2 p = ssCellLocal(ssProjectUV(pos, N));
  vec2 aa = ssAA();

  float dS = ssShapeSdf(p);
  vec2 cS = ssCarveVS(dS, SS_DEPTH);
  float carve = cS.x;
  vec2 gradUV = vec2(0.);
  if (abs(dS) < SS_WALL_HW) {
    float e = 0.5 * SS_WALL_HW;
    vec2 gd = vec2(ssShapeSdf(p + vec2(e, 0.)) - ssShapeSdf(p - vec2(e, 0.)),
                   ssShapeSdf(p + vec2(0., e)) - ssShapeSdf(p - vec2(0., e))) / (2. * e);
    gradUV = cS.y * gd * reliefAAFade(max(aa.x, aa.y), SS_WALL_HW);
  }

#if SS_GROOVE
  float dG = ssGrooveSdf(p.y);
  vec2 cG = ssCarveVS(dG, SS_GROOVE_DEPTH);
  if (cG.x > carve) {
    carve = cG.x;
    gradUV = abs(dG) < SS_WALL_HW
      ? vec2(0., cG.y * sign(abs(p.y) - SS_GROOVE_POS) * sign(p.y)) * reliefAAFade(aa.y, SS_WALL_HW)
      : vec2(0.);
  }
#endif

#if SS_UV_MODE == 1
  vec3 gw = depth * (gradUV.x * uvFrameT + gradUV.y * uvFrameB);
#else
  vec3 na = abs(N);
  vec3 gradW;
  if (na.y >= na.x && na.y >= na.z) {
    gradW = vec3(gradUV.x, 0., gradUV.y);
  } else if (na.x >= na.z) {
    gradW = vec3(0., gradUV.y, gradUV.x);
  } else {
    gradW = vec3(gradUV.x, gradUV.y, 0.);
  }
  vec3 gw = depth * gradW;
#endif
  return normalize(N + (gw - dot(gw, N) * N));
}
