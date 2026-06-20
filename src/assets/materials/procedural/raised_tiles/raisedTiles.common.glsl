// Raised concrete tiles: a square grid where each tile is a flat plateau at its
// own height. Adjacent tiles are *flush* — no grout groove. Where two tiles share
// a height they meet seamlessly (a faint painted seam only); where heights differ
// they meet at the midpoint height via a narrow C1 wall, giving a clean step with
// no raymarch-breaking discontinuity. Per-tile height is the single creative knob
// (rtTileHeight01): hash quantized to tiers by default, with continuous/ripple/
// terrace alternatives commented inline.
//
// Cell topology is isolated in rtCellField + the neighbour lookups; swapping it
// for a hex distance field yields hex tiles with the rest unchanged.

const float RT_CELL      = 1.0;
const float RT_WALL_W    = 0.02;  // run of the step wall on each side of a shared edge (tight = crisp edges)
const float RT_SEAM_HW   = 0.01;  // painted hairline-seam half-width (no geometry)
const float RT_TILE_TOP  = 0.0;   // carve of the tallest tile (0 = flush with the base surface)
const float RT_TILE_SPAN = 0.5;   // tile-to-tile height spread (carve units)
const float RT_LEVELS    = 5.0;   // discrete height tiers (1 = continuous)

const vec3  RT_CONCRETE   = vec3(0.31, 0.295, 0.265); // base albedo (recolour freely)
const vec3  RT_SEAM_COLOR = vec3(0.05, 0.047, 0.042);
const float RT_SEAM_DARK  = 0.5;   // seam-line darkening strength
const float RT_TINT_AMP   = 0.10;  // per-tile albedo value jitter
const float RT_MOTTLE_AMP = 0.16;  // large-patch stain contrast
const float RT_BASE_ROUGH = 0.9;

const float RT_AO_REACH   = 0.18;  // contact-AO reach into a recessed tile from a taller edge
const float RT_AO_RECESS  = 0.6;   // indirect mul at full contact AO
const vec2  RT_LIGHT_UV   = vec2(-0.3304, 0.9438); // fake key in face space (from above, slight lean)
const float RT_SHADOW_REACH  = 1.8; // cell-units of shadow per unit height diff (× light grazing)
const float RT_SHADOW_DARKEN = 0.3; // direct mul in full self-shadow

const float RT_FADE_PERIOD = 0.27; // footprint at which seam/AO/shadow detail dissolves

// Must match pom.lodFadeStart / lodFadeRange in build.mjs: the distance window
// over which the engine retracts the relief to flat. Past it the surface is flat,
// so the height-derived cues (per-tile tint, AO, self-shadow) must retract too —
// else distant tiles read as flat, differently-shaded squares.
const float RT_POM_FADE_START = 16.0;
const float RT_POM_FADE_RANGE = 40.0;

// 0 = full relief (near) → 1 = relief fully retracted (past the POM cutoff).
float rtPomFade(float distanceToCamera) {
  return smoothstep(RT_POM_FADE_START, RT_POM_FADE_START + RT_POM_FADE_RANGE, distanceToCamera);
}

// Dominant-axis projection into 2D (Y→xz, X→zy, Z→xy), matching the other POM materials.
vec2 rtProjectUV(vec3 pos, vec3 axisNormal) {
  vec3 a = abs(axisNormal);
  if (a.y >= a.x && a.y >= a.z) { return pos.xz; }
  else if (a.x >= a.z)         { return vec2(pos.z, pos.y); }
  return vec2(pos.x, pos.y);
}

// Inverse of rtProjectUV: map a 2D tangent-plane vector back to world.
vec3 rtUVtoWorld(vec2 g, vec3 axisNormal) {
  vec3 a = abs(axisNormal);
  if (a.y >= a.x && a.y >= a.z) { return vec3(g.x, 0., g.y); }
  else if (a.x >= a.z)         { return vec3(0., g.y, g.x); }
  return vec3(g.x, g.y, 0.);
}

// Square cell topology. cl = signed cell-local coords; edgeDir = unit step toward
// the nearest edge (also the integer offset to the neighbour across it). Returns
// the edge distance b: 0 on an edge → CELL/2 at the centre.
float rtCellField(vec2 uv, out vec2 cellId, out vec2 cl, out vec2 edgeDir) {
  cellId = floor(uv / RT_CELL);
  cl = (fract(uv / RT_CELL) - 0.5) * RT_CELL;
  edgeDir = abs(cl.x) >= abs(cl.y) ? vec2(sign(cl.x), 0.) : vec2(0., sign(cl.y));
  return 0.5 * RT_CELL - max(abs(cl.x), abs(cl.y));
}

// `u_heightPhase`: CPU-accumulated monotonic time phase (gated rate) used as a
// pseudo-3rd noise dimension — each tile value-noises between integer phase steps,
// holding its tier between bursts and reshuffling during them.
const vec2 RT_TIME_OFFSET = vec2(31.41, 27.18);

// ===== creative knob: per-tile height in [0,1], 1 = tallest (flush) ==========
float rtTileHeight01(vec2 cellId) {
  vec2 seed = cellId + 0.5;
  float zi = floor(u_heightPhase);
  float h = mix(
    hash(seed + RT_TIME_OFFSET * zi),
    hash(seed + RT_TIME_OFFSET * (zi + 1.0)),
    smoothstep(0.0, 1.0, u_heightPhase - zi)
  );
  // return floor(h * RT_LEVELS) / (RT_LEVELS - 1.0); // random, quantized to tiers (default)
  return h;
  // return hash(cellId + 0.5);                    // continuous random
  // return cos(cellId.x * 0.6) * 0.5 + 0.5;       // horizontal ripple
  // return fract((cellId.x + cellId.y) * 0.2);    // diagonal terraces
  // return 0.5 + 0.5 * sin(cellId.x * 0.4) * cos(cellId.y * 0.4); // egg-crate
}
// =============================================================================

float rtPlateauCarve(float h01) {
  return RT_TILE_TOP + (1.0 - h01) * RT_TILE_SPAN;
}

float rtTileCarve(vec2 cellId) {
  return rtPlateauCarve(rtTileHeight01(cellId));
}

// Step wall: from this tile's plateau (b ≥ WALL_W) down/up to the midpoint of the
// two tiles at the shared edge (b = 0). Symmetric, so both tiles meet flush there.
float rtWallCarve(float b, float cThis, float cNb) {
  return mix(0.5 * (cThis + cNb), cThis, smoothstep(0.0, RT_WALL_W, b));
}
