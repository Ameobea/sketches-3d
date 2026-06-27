// Drainage-gutter trench material: infinite trench strips (periodic across
// one axis, unbounded along the other) bridged by slats, with deep dark gaps
// carved between them and a solid rail along each trench edge. Carving is
// zero outside the trenches, so the marcher's first sample terminates over
// most of the surface.

const bool GT_ALONG_X = true; // trenches run along the projected-UV x axis

const float GT_PITCH      = 6.;    // spacing between trench centerlines
const float GT_HALF_W     = 0.9;   // trench half-width
const float GT_SLAT_PITCH = 0.5;   // slat repeat along the trench
const float GT_GAP_HW     = 0.13;  // gap floor half-width (slat thickness = pitch − 2·(hw+wall))
const float GT_WALL       = 0.018; // gap wall ramp width
const float GT_END        = 0.1;   // gap end inset from the trench edge (the rail)
const float GT_END_WALL   = 0.05;  // gap end-wall ramp width
const float GT_END_OUT    = GT_HALF_W - GT_END;
const float GT_CARVE      = 0.8;   // marcher clamps carved depth at 0.8 = full pom.depth

const vec3 GT_BASE_COLOR = vec3(0.106, 0.110, 0.117);
const vec3 GT_VOID_COLOR = vec3(0.006, 0.007, 0.008); // reads as a hole, darker than the grate slots

const float GT_AO_VOID     = 0.45;
const float GT_DIRECT_VOID = 0.3;

// Signed offset to the nearest trench centerline (across-axis).
float gtTrenchOffset(vec2 uv) {
  float v = GT_ALONG_X ? uv.y : uv.x;
  return (fract(v / GT_PITCH + 0.5) - 0.5) * GT_PITCH;
}

// Signed offset to the nearest gap centerline (along-axis).
float gtGapOffset(vec2 uv) {
  float u = GT_ALONG_X ? uv.x : uv.y;
  return (fract(u / GT_SLAT_PITCH + 0.5) - 0.5) * GT_SLAT_PITCH;
}

// safeStep lateral distance to the nearest height-varying wall — the gap-wall ramp
// (along-axis) ∪ the trench-end-wall ramp (across-axis). 0 inside a ramp band; the
// marcher strides flats at this distance and steps fine through the bands.
float gridLateralDist(vec2 uv) {
  float dv = abs(gtTrenchOffset(uv));
  float g = abs(gtGapOffset(uv));
  float dGap = max(GT_GAP_HW - g, g - (GT_GAP_HW + GT_WALL));
  float dEnd = abs(dv - (GT_END_OUT - 0.5 * GT_END_WALL)) - 0.5 * GT_END_WALL;
  return max(0., min(dGap, dEnd));
}

// AA'd visual carve (gap coverage) for the color + attenuation slots; `dv`
// must already be inside the trench.
float gtVisCarve(vec2 uv, float dv, float aa) {
  float g = abs(gtGapOffset(uv));
  float gap = aaSlot(g, GT_SLAT_PITCH, GT_GAP_HW, GT_WALL, aa);
  float we = max(0.5 * GT_END_WALL, aa);
  float end = 1. - smoothstep(GT_END_OUT - 0.5 * GT_END_WALL - we, GT_END_OUT - 0.5 * GT_END_WALL + we, dv);
  return gap * end;
}

// --- Tier-A analytic intersection (pom-capability-ladder-plan.md Phase 4) -----------------
// Closed-form first crossing of s·NdotV = depth·carve(line(s)) for the LINEARIZED field
// (grateTrench.height.glsl): with linearstep walls, carve = gap(g)·end(dv) is piecewise
// quadratic in s, so each segment between feature breakpoints solves exactly. Returns -1 to
// fall back to safeStep when the ray spans more than one slat cell or trench (grazing). Keep
// the breakpoint/quadratic logic in sync with scripts/pomMarcherHarness/grateTrench.mjs.
float _gtSolveSeg(float A, float B, float C, float lo, float hi) {
  float a0 = max(lo, 0.) - 1e-7, b0 = hi + 1e-7;
  if (abs(A) < 1e-12) {
    if (abs(B) < 1e-15) {
      return -1.;
    }
    float s = -C / B;
    return (s >= a0 && s <= b0) ? max(s, 0.) : -1.;
  }
  float disc = B * B - 4. * A * C;
  if (disc < 0.) {
    return -1.;
  }
  float sq = sqrt(disc);
  float r1 = (-B - sq) / (2. * A), r2 = (-B + sq) / (2. * A);
  float rmin = min(r1, r2), rmax = max(r1, r2);
  if (rmin >= a0 && rmin <= b0) {
    return max(rmin, 0.);
  }
  if (rmax >= a0 && rmax <= b0) {
    return max(rmax, 0.);
  }
  return -1.;
}

float gridAnalyticHit(vec2 uv0, vec2 duv, float NdotV, float depth) {
  float marchLen = depth / max(NdotV, 1e-3);
  float u0 = GT_ALONG_X ? uv0.x : uv0.y, du = GT_ALONG_X ? duv.x : duv.y;  // along-axis (gaps)
  float v0 = GT_ALONG_X ? uv0.y : uv0.x, dvv = GT_ALONG_X ? duv.y : duv.x; // across-axis (trenches)
  if (floor(u0 / GT_SLAT_PITCH + 0.5) != floor((u0 + du * marchLen) / GT_SLAT_PITCH + 0.5)) {
    return -1.;
  }
  if (floor(v0 / GT_PITCH + 0.5) != floor((v0 + dvv * marchLen) / GT_PITCH + 0.5)) {
    return -1.;
  }

  float gOff0 = gtGapOffset(uv0), vOff0 = gtTrenchOffset(uv0);
  if (abs(gOff0) >= GT_GAP_HW + GT_WALL || abs(vOff0) >= GT_END_OUT) {
    return 0.; // entry already on the surface (slat top / rail)
  }

  float bp[14];
  int n = 0;
  bp[n++] = 0.;
  bp[n++] = marchLen;
  if (abs(du) > 1e-12) {
    float gw = GT_GAP_HW + GT_WALL;
    float ts[5] = float[5](0., GT_GAP_HW, -GT_GAP_HW, gw, -gw);
    for (int i = 0; i < 5; i++) {
      float s = (ts[i] - gOff0) / du;
      if (s > 1e-9 && s < marchLen) {
        bp[n++] = s;
      }
    }
  }
  if (abs(dvv) > 1e-12) {
    float e1 = GT_END_OUT - GT_END_WALL;
    float te[5] = float[5](0., e1, -e1, GT_END_OUT, -GT_END_OUT);
    for (int i = 0; i < 5; i++) {
      float s = (te[i] - vOff0) / dvv;
      if (s > 1e-9 && s < marchLen) {
        bp[n++] = s;
      }
    }
  }
  for (int i = 1; i < n; i++) {
    float key = bp[i];
    int j = i - 1;
    for (; j >= 0 && bp[j] > key; j--) {
      bp[j + 1] = bp[j];
    }
    bp[j + 1] = key;
  }

  float k = depth * GT_CARVE;
  for (int i = 0; i + 1 < n; i++) {
    float a = bp[i], b = bp[i + 1];
    if (b - a < 1e-9) {
      continue;
    }
    float m = 0.5 * (a + b);
    float ag = 1., bg = 0.;
    float gm = abs(gOff0 + du * m);
    if (gm >= GT_GAP_HW + GT_WALL) {
      ag = 0.;
    } else if (gm > GT_GAP_HW) {
      float sg = sign(gOff0 + du * m);
      ag = (GT_GAP_HW + GT_WALL - sg * gOff0) / GT_WALL;
      bg = -sg * du / GT_WALL;
    }
    float ae = 1., be = 0.;
    float dm = abs(vOff0 + dvv * m);
    if (dm >= GT_END_OUT) {
      ae = 0.;
    } else if (dm > GT_END_OUT - GT_END_WALL) {
      float sv = sign(vOff0 + dvv * m);
      ae = (GT_END_OUT - sv * vOff0) / GT_END_WALL;
      be = -sv * dvv / GT_END_WALL;
    }
    float root = _gtSolveSeg(k * bg * be, k * (ag * be + bg * ae) - NdotV, k * ag * ae, a, b);
    if (root >= 0.) {
      return root;
    }
  }
  return marchLen;
}
