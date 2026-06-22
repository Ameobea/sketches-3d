// Shared prelude for the triangle-grid material: tunable constants + helpers,
// emitted before the color/attenuation/POM slots that call into it.

const float TRI_EDGE = 8.0;

// Bands by perpendicular edge-distance, walking from a shared edge inward:
//   [0, GAP_HALF) seam · [.., BORDER_END) border · [.., WALL_END) carved wall · [WALL_END, inradius] floor
const float TRI_GAP_HALF      = 0.15;
const float TRI_BORDER_WIDTH  = 0.35;
const float TRI_WALL_WIDTH    = 0.30;
const float TRI_FLOOR_DEPTH   = 0.8;  // [0,1]; the POM marcher clamps carved depth to 0.8, so 0.8 = full floor
const float TRI_WALL_BAND_PAD = 0.04; // insets the wall *color* band so POM hit imprecision can't bleed it out
const float TRI_BORDER_END = TRI_GAP_HALF + TRI_BORDER_WIDTH;
const float TRI_WALL_END   = TRI_BORDER_END + TRI_WALL_WIDTH;
const float TRI_FADE_PERIOD = 2.0 * TRI_WALL_END; // footprint at which the grid dissolves to fill; raise = persist longer

const vec3  TRI_BG_COLOR     = vec3(0.55, 0.57, 0.57); // light gray
const vec3  TRI_BORDER_COLOR = vec3(0.015, 0.017, 0.017); // dark charcoal
const vec3  TRI_FILL_COLOR   = vec3(0.03, 0.20, 0.18); // teal
const float TRI_WALL_DARKEN  = 0.35; // pit-wall color = fill * this

const float TRI_SQRT3 = 1.7320508075688772;

// Edge-line family normals, 120° apart; the tiling edges are their level sets.
const vec2 TRI_N0 = vec2(0., 1.);
const vec2 TRI_N1 = vec2(-0.8660254037844386, -0.5);
const vec2 TRI_N2 = vec2(0.8660254037844386, -0.5);

// Fake pit cast-shadow (analytic). Art-directed extent, not physical: the pits
// are shallow + wide, so a true cast shadow wouldn't reach the floor.
const vec3  TRI_SHADOW_LIGHT_DIR = normalize(vec3(70., 10., -45.)); // direction only
const float TRI_SHADOW_REACH     = 2.;
const float TRI_SHADOW_PENUMBRA  = 0.3; // soft-edge fraction of the reach
const float TRI_SHADOW_DARKEN    = 0.03;
const float TRI_SHADOW_WALL_LIFT = 0.4; // reach kept at the wall top (cd=0), climbing the diagonal to the rim

// Fake AO (analytic, light-independent).
const float TRI_AO_DEPTH      = 0.75; // pit-floor brightness from depth AO
const float TRI_AO_WALL       = 0.55; // brightness at the wall/floor crease
const float TRI_AO_WALL_RANGE = 0.5;
const float TRI_AO_CORNER     = 0.45; // smooth-min radius for corner darkening / rounding

// Distance to the nearest tiling edge: min over the 3 line families (spaced by
// triangle height H), each a centered sawtooth. 0 on an edge → inradius H/3 at the incenter.
float triEdgeDist(vec2 uv) {
  float H = TRI_EDGE * 0.5 * TRI_SQRT3;
  vec3 p = vec3(dot(uv, TRI_N0), dot(uv, TRI_N1), dot(uv, TRI_N2)) / H;
  vec3 d = abs(fract(p + 0.5) - 0.5) * H;
  return min(d.x, min(d.y, d.z));
}

// safeStep lateral distance to the nearest height-varying region — the carve ramp lives in the
// edge-distance band [TRI_BORDER_END, TRI_WALL_END]; flat top below it, flat floor above. 0 in the
// band. triEdgeDist is 1-Lipschitz, so this is a valid lateral distance.
float gridLateralDist(vec2 uv) {
  float ed = triEdgeDist(uv);
  return max(0., max(TRI_BORDER_END - ed, ed - TRI_WALL_END));
}

// As triEdgeDist, but per-family (x/y/z) plus each family's sawtooth sign, so the
// inward edge normal reconstructs as `sgns[i] * TRI_Ni`.
vec3 triEdgeDist3(vec2 uv, out vec3 sgns) {
  float H = TRI_EDGE * 0.5 * TRI_SQRT3;
  vec3 p = vec3(dot(uv, TRI_N0), dot(uv, TRI_N1), dot(uv, TRI_N2)) / H;
  vec3 f = fract(p + 0.5) - 0.5;
  sgns = sign(f);
  return abs(f) * H;
}

// As triEdgeDist, plus the unit gradient (into the triangle). For a distance-to-
// parallel-lines field the gradient is just the signed family normal — no derivative work.
float triEdgeDistGrad(vec2 uv, out vec2 gradDir) {
  vec3 sgns;
  vec3 d = triEdgeDist3(uv, sgns);
  if (d.x <= d.y && d.x <= d.z) {
    gradDir = sgns.x * TRI_N0;
    return d.x;
  } else if (d.y <= d.z) {
    gradDir = sgns.y * TRI_N1;
    return d.y;
  }
  gradDir = sgns.z * TRI_N2;
  return d.z;
}

// Shared at-hit frame (hitType): one triEdgeDist3 eval, reused by the color/attenuation/normal
// slots so they don't each re-project and re-decompose. ed/gradDir/AO/shadow all derive from (d, sgns).
struct TriHit { vec3 d; vec3 sgns; };
TriHit gridComputeHit(vec2 uv) {
  vec3 sgns;
  vec3 d = triEdgeDist3(uv, sgns);
  return TriHit(d, sgns);
}

// Polynomial smooth-minimum (Inigo Quilez); `k` is the rounding radius.
float triSmin(float a, float b, float k) {
  float h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
  return mix(b, a, h) - k * h * (1.0 - h);
}

// Analytic cast-shadow for the whole pit. For each rim edge that lies up-light,
// the reach along the projected light to that rim is d_i/a_i; nearest wins. The
// length scales with carved depth `cd` so the boundary climbs the walls
// diagonally and the top surface (cd=0) excludes itself; the up-light wall also
// self-excludes (a<0). Umbra to (1-PENUMBRA) of the reach, then a soft edge.
float triPitShadowFromEdges(vec3 d, vec3 sgns, float cd, vec3 worldN) {
  if (dot(worldN, TRI_SHADOW_LIGHT_DIR) <= 0.0) {
    return 0.0;
  }
  vec2 Lraw = domProject(TRI_SHADOW_LIGHT_DIR, domAxis(worldN));
  float Llen = length(Lraw);
  if (Llen < 1e-4) {
    return 0.0; // light straight overhead
  }
  vec2 Luv = Lraw / Llen;
  vec2 m0 = sgns.x * TRI_N0;
  vec2 m1 = sgns.y * TRI_N1;
  vec2 m2 = sgns.z * TRI_N2;
  float reach = 1e9;
  float a0 = -dot(Luv, m0); if (a0 > 1e-3) { reach = min(reach, d.x / a0); }
  float a1 = -dot(Luv, m1); if (a1 > 1e-3) { reach = min(reach, d.y / a1); }
  float a2 = -dot(Luv, m2); if (a2 > 1e-3) { reach = min(reach, d.z / a2); }
  float shadowLen = mix(TRI_SHADOW_WALL_LIFT, 1.0, cd) * TRI_SHADOW_REACH;
  float r = reach / max(shadowLen, 1e-4);
  float pen = max(TRI_SHADOW_PENUMBRA, 1e-3);
  return 1.0 - smoothstep(1.0 - pen, 1.0, r);
}
