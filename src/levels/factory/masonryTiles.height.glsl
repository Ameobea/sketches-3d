// Procedural masonry-tile heightfield (rounded variant).
//
// Each cell holds a rectangular tile (~1.78:1, wider than tall) with a smooth
// concave cos-bowl dent in the center. Tiles have rounded corners and a soft
// bevel where they meet the surrounding groove — both via a rounded-box SDF
// + two smoothstep "zones." The whole height function is continuous: the
// only sharp feature in the previous version was the tile/groove cliff, and
// that's now a smooth bevel slope. Eliminates the cliff-aliasing the user
// reported at the brick/grout boundary.
//
// Rows are horizontally staggered by a per-row hash for a randomized
// masonry look.

float getPomHeight(vec3 pos, vec3 normal, float curTimeSeconds) {
  vec3 absN = abs(normal);
  vec2 uv;
  if (absN.y >= absN.x && absN.y >= absN.z) {
    uv = pos.xz;
  } else if (absN.x >= absN.z) {
    uv = vec2(pos.z, pos.y);
  } else {
    uv = vec2(pos.x, pos.y);
  }

  const vec2  CELL = vec2(3.2, 1.8);        // ~2× previous scale, 1.78:1.
  const vec2  GROOVE = vec2(0.16, 0.16);    // total groove between cells.
  const vec2  TILE_HALF = 0.5 * (CELL - GROOVE);
  const float CORNER = 0.08;                // SDF corner radius.
  const float BEVEL = 0.15;                 // tile/groove slope width.

  const float GROOVE_DEPTH = 0.65;          // deepest carve.
  const float DENT_DEPTH = 0.4;             // bowl carve at tile center.

  // Per-row masonry stagger.
  float row = floor(uv.y / CELL.y + 0.5);
  float rowHash = fract(sin(row * 12.9898 + 78.233) * 43758.5453);
  vec2 uvStagger = vec2(uv.x - rowHash * CELL.x, uv.y);

  // Cell-local coords centered on the tile.
  vec2 cellLocal = (fract(uvStagger / CELL + 0.5) - 0.5) * CELL;

  // Rounded-box SDF: positive in groove, negative inside tile, zero at rim.
  vec2 q = abs(cellLocal) - TILE_HALF + CORNER;
  float sdf = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - CORNER;

  // AA-widened slope. BEVEL sets the close-range bevel width in world units;
  // the screen-pixel footprint takes over at distance so sub-pixel features
  // can't alias. Uses vWorldPos (per-fragment constant) not the per-sample
  // march `pos`, so the band is identical across march iterations.
  float band = max(BEVEL, distance(cameraPosition, vWorldPos) * 0.001);

  // Two zone smoothsteps meeting at sdf=0 (the tile rim, h=0). Inside: dent
  // grows toward the bowl center. Outside: groove grows toward GROOVE_DEPTH.
  float dentZone = 1.0 - smoothstep(-band, 0.0, sdf);
  float grooveZone = smoothstep(0.0, band, sdf);

  // Cos bowl in normalized tile coords.
  vec2 tileLocal = cellLocal / TILE_HALF;
  float bowl = max(0.0, cos(tileLocal.x * 1.5707963) * cos(tileLocal.y * 1.5707963));

  return bowl * DENT_DEPTH * dentZone + GROOVE_DEPTH * grooveZone;
}
