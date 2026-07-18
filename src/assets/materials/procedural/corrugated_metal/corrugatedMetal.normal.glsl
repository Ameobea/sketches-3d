// Relief normal: groove-wall gradient (mirrors cmCarve — keep in sync) plus the
// shading-only crest crowning, mapped to world via the UV frame (UV mode) or the
// dominant axis of N (world mode). The crown adds carve CM_CROWN·(1+cos(2π·off/P))/2 —
// max at the groove, 0 at mid-rib — so ribs read as gently rolled sheet.
vec3 getPomNormal(vec3 pos, vec3 N, float depth, float t, float aa) {
  float s = patProjectUV(pos, N).x;
  bool lap;
  float off = cmGroove(s, lap);
  vec3 gp = cmParams(lap);

  vec2 aa2 = patAA();
  float dGroove = -gp.z * smoothstepVS(gp.x, gp.x + gp.y, abs(off)).y * sign(off);
  float dCrown = -CM_CROWN * 3.1416 / CM_PITCH * sin(6.2832 * off / CM_PITCH);
  float ds = dGroove * reliefAAFade(aa2.x, CM_WALL) + dCrown * reliefAAFade(aa2.x, 0.5 * CM_PITCH);

  vec3 gw = depth * patGradToWorld(vec2(ds, 0.), N);
  return normalize(N + (gw - dot(gw, N) * N));
}
