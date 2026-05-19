struct Masonry {
  float dentZone;    // 1 inside the tile bowl, 0 in the groove
  float grooveZone;  // 1 in the groove, 0 inside the tile
  float bowl;        // cos-bowl profile in normalized tile coords
  vec2  cellId;      // (col, row) for stable per-brick hashing
};

Masonry evalMasonry(vec3 pos, vec3 axisNormal, float viewDist) {
  vec3 absN = abs(axisNormal);
  vec2 uv;
  if (absN.y >= absN.x && absN.y >= absN.z) {
    uv = pos.xz;
  } else if (absN.x >= absN.z) {
    uv = vec2(pos.z, pos.y);
  } else {
    uv = vec2(pos.x, pos.y);
  }

  const vec2 CELL = vec2(3.8, 2.6);
  const vec2 GROOVE = vec2(0.16, 0.16);
  const vec2 TILE_HALF = 0.5 * (CELL - GROOVE);
  const float CORNER_RADIUS = 0.01;
  const float BEVEL = 0.15;

  float row = floor(uv.y / CELL.y + 0.5);
  float rowHash = fract(sin(row * 12.9898 + 78.233) * 43758.5453);
  vec2 uvStagger = vec2(uv.x - rowHash * CELL.x, uv.y);
  vec2 cellLocal = (fract(uvStagger / CELL + 0.5) - 0.5) * CELL;

  // rounded-box SDF
  vec2 q = abs(cellLocal) - TILE_HALF + CORNER_RADIUS;
  float sdf = length(max(q, 0.)) + min(max(q.x, q.y), 0.) - CORNER_RADIUS;

  float band = max(BEVEL, viewDist * 0.001);

  Masonry m;
  m.dentZone = 1. - smoothstep(-band, 0., sdf);
  m.grooveZone = smoothstep(0., band, sdf);

  vec2 tileLocal = cellLocal / TILE_HALF;
  m.bowl = max(0., cos(tileLocal.x * 1.5707963) * cos(tileLocal.y * 1.5707963));
  m.cellId = vec2(floor(uvStagger.x / CELL.x + 0.5), row);
  return m;
}
