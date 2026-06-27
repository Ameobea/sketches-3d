// POM height field: gap + border stay flat (0), the interior carves inward over
// [TRI_BORDER_END, TRI_WALL_END]. Targets 0.8 (not 1) on the floor because the
// marcher clamps carved depth to 0.8 before scaling by pom.depth.
float gridHeight(vec2 uv, float t) {
  float ed = triEdgeDist(uv);
  return TRI_FLOOR_DEPTH * smoothstep(TRI_BORDER_END, TRI_WALL_END, ed);
}
