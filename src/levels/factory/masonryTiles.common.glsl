struct Masonry {
  float dentZone;    // 1 inside the tile bowl, 0 in the groove
  float grooveZone;  // 1 in the groove, 0 inside the tile
  float bowl;        // bowl profile in normalized tile coords
  vec2  cellId;      // (col, row) for stable per-brick hashing
};

// Cheap sin-free 1D hash (Dave Hoskins)
float masonryHash11(float p) {
  p = fract(p * 0.1031);
  p *= p + 33.33;
  p *= p + p;
  return fract(p);
}

const vec2 MASONRY_CELL = vec2(3.8, 2.6);
const vec2 MASONRY_GROOVE = vec2(0.16, 0.16);
const vec2 MASONRY_TILE_HALF = vec2(1.82, 1.22); // 0.5 * (CELL - GROOVE)
const float MASONRY_CORNER_RADIUS = 0.01;
const float MASONRY_BEVEL = 0.15;
const float MASONRY_DENT_DEPTH = 0.4;
const float MASONRY_GROOVE_DEPTH = 1.0;

struct MasonryCore {
  float dentZone;
  float grooveZone;
  float bowl;
  float row;
  float staggerX;
};

MasonryCore evalMasonryCore(vec3 pos, vec3 axisNormal, float viewDist) {
  vec3 absN = abs(axisNormal);
  vec2 uv;
  if (absN.y >= absN.x && absN.y >= absN.z) {
    uv = pos.xz;
  } else if (absN.x >= absN.z) {
    uv = vec2(pos.z, pos.y);
  } else {
    uv = vec2(pos.x, pos.y);
  }

  float row = floor(uv.y / MASONRY_CELL.y + 0.5);
  float rowHash = masonryHash11(row);
  vec2 uvStagger = vec2(uv.x - rowHash * MASONRY_CELL.x, uv.y);
  vec2 cellLocal = (fract(uvStagger / MASONRY_CELL + 0.5) - 0.5) * MASONRY_CELL;

  // rounded-box SDF
  vec2 q = abs(cellLocal) - MASONRY_TILE_HALF + MASONRY_CORNER_RADIUS;
  float sdf = length(max(q, 0.)) + min(max(q.x, q.y), 0.) - MASONRY_CORNER_RADIUS;

  float band = max(MASONRY_BEVEL, viewDist * 0.001);

  MasonryCore c;
  c.dentZone = 1. - smoothstep(-band, 0., sdf);
  c.grooveZone = smoothstep(0., band, sdf);

  vec2 tileLocal = cellLocal / MASONRY_TILE_HALF;
  vec2 bowlXY = max(vec2(0.), 1. - tileLocal * tileLocal);
  c.bowl = bowlXY.x * bowlXY.y;

  c.row = row;
  c.staggerX = uvStagger.x;
  return c;
}

Masonry evalMasonry(vec3 pos, vec3 axisNormal, float viewDist) {
  MasonryCore c = evalMasonryCore(pos, axisNormal, viewDist);
  Masonry m;
  m.dentZone = c.dentZone;
  m.grooveZone = c.grooveZone;
  m.bowl = c.bowl;
  m.cellId = vec2(floor(c.staggerX / MASONRY_CELL.x + 0.5), c.row);
  return m;
}
