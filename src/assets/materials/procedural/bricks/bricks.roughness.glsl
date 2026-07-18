// Matte concrete with rougher porous mortar; soot deposits push roughness up.
// Specular is the most alias-sensitive term, so the joint mix filters against a
// doubled footprint and settles on the area-mean earlier than color does.
float getCustomRoughness(vec3 pos, vec3 normal, float baseRoughness, float curTimeSeconds, SceneCtx ctx) {
  vec2 uv = patProjectUV(pos, vWorldNormal);
  vec2 brickId, cl;
  vec2 bd = brCellField(uv, brickId, cl);
  float r = mix(BR_BLOCK_ROUGH, BR_MORTAR_ROUGH, brJointVis(bd, 2. * patAA()));
  return mix(r, BR_SOOT_ROUGH, brSoot(uv));
}
