precision highp float;

#define MAX_STOPS 8
#define MAX_BANDS 4
#define MAX_HAZES 2
#define MAX_HAZE_OCTAVES 6

#define HORIZON_MODE_SOLID  0
#define HORIZON_MODE_MIRROR 1
#define HORIZON_MODE_EXTEND 2

uniform float uTime;

// --- Gradient -------------------------------------------------------------
uniform int uStopCount;
uniform float uStopPositions[MAX_STOPS];
uniform vec3 uStopColors[MAX_STOPS];

// --- Horizon --------------------------------------------------------------
uniform float uHorizonOffset;
uniform int uHorizonMode;
uniform vec3 uBelowColor;
uniform float uHorizonBlend;

// --- Solid-color bands (additive, time-fading) ----------------------------
uniform int uBandCount;
uniform float uBandCenters[MAX_BANDS];
uniform float uBandWidths[MAX_BANDS];
uniform vec3 uBandColors[MAX_BANDS];
uniform float uBandIntensities[MAX_BANDS];
uniform float uBandFadeRates[MAX_BANDS];
uniform float uBandFadePhases[MAX_BANDS];

// --- Noise-driven haze bands (alpha-over) ---------------------------------
uniform int uHazeCount;
uniform vec3 uHazeColors[MAX_HAZES];
// High-density color: cloud color crossfades from uHazeColors (thin wisps) to
// uHazeHighColors (dense cores) as a function of the shaped fBm value. When
// the layer doesn't opt in, both are set to the same color and the mix is a
// no-op.
uniform vec3 uHazeHighColors[MAX_HAZES];
uniform float uHazeIntensities[MAX_HAZES];
uniform float uHazeCenters[MAX_HAZES];
uniform float uHazeWidths[MAX_HAZES];
uniform float uHazeSharpness[MAX_HAZES];
uniform vec3 uHazeScales[MAX_HAZES];
uniform vec3 uHazeSpeeds[MAX_HAZES];
// Low-frequency elevation warp to break up circular contours at high elevations.
// Noise is sampled on a unit circle for seamless azimuth continuity.
uniform float uHazeWarp[MAX_HAZES];
uniform float uHazeWarpScale[MAX_HAZES];
uniform float uHazeWarpSpeed[MAX_HAZES];
// Per-layer fBm shaping knobs. `skyFbm` loops up to MAX_HAZE_OCTAVES with an
// early break on uHazeOctaves[i]. bias/pow are applied to the raw fBm output
// before the smoothstep threshold — `pow` in particular is what turns the same
// noise field into crisp cumulus vs. soft haze.
uniform int uHazeOctaves[MAX_HAZES];
uniform float uHazeLacunarity[MAX_HAZES];
uniform float uHazeGain[MAX_HAZES];
uniform float uHazeBias[MAX_HAZES];
uniform float uHazePow[MAX_HAZES];

// --- Starfield (additive) -------------------------------------------------
uniform float uStarIntensity;
uniform vec3 uStarColor;
uniform float uStarDensity;
uniform float uStarThreshold;
uniform float uStarSize;
uniform float uStarTwinkleSpeed;
uniform float uStarMinElev;

in vec3 vWorldPos;
out vec4 fragColor;

const float PI = 3.141592653589793;
const float TWO_PI = 6.283185307179586;
const float HALF_PI = 1.5707963267948966;

// --- Oklab color mixing (perceptually uniform) ----------------------------
vec3 rgbToOklab(vec3 c) {
  float l = 0.4122214708 * c.r + 0.5363325363 * c.g + 0.0514459929 * c.b;
  float m = 0.2119034982 * c.r + 0.6806995451 * c.g + 0.1073969566 * c.b;
  float s = 0.0883024619 * c.r + 0.2817188376 * c.g + 0.6299787005 * c.b;
  float l_ = pow(max(l, 0.0), 1.0 / 3.0);
  float m_ = pow(max(m, 0.0), 1.0 / 3.0);
  float s_ = pow(max(s, 0.0), 1.0 / 3.0);
  return vec3(0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_, 1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_, 0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_);
}
vec3 oklabToRgb(vec3 c) {
  float l_ = c.x + 0.3963377774 * c.y + 0.2158037573 * c.z;
  float m_ = c.x - 0.1055613458 * c.y - 0.0638541728 * c.z;
  float s_ = c.x - 0.0894841775 * c.y - 1.2914855480 * c.z;
  return vec3(4.0767416621 * (l_ * l_ * l_) - 3.3077115913 * (m_ * m_ * m_) + 0.2309699292 * (s_ * s_ * s_), -1.2684380046 * (l_ * l_ * l_) + 2.6097574011 * (m_ * m_ * m_) - 0.3413193965 * (s_ * s_ * s_), -0.0041960863 * (l_ * l_ * l_) - 0.7034186147 * (m_ * m_ * m_) + 1.6956082560 * (s_ * s_ * s_));
}
vec3 oklabMix(vec3 a, vec3 b, float t) {
  return oklabToRgb(mix(rgbToOklab(a), rgbToOklab(b), t));
}

// --- Base gradient --------------------------------------------------------
vec3 evalGradient(float t) {
  if (uStopCount <= 0) {
    return vec3(0.0);
  }
  if (t <= uStopPositions[0]) {
    return uStopColors[0];
  }
  for (int i = 1; i < MAX_STOPS; i++) {
    if (i >= uStopCount) {
      break;
    }
    float p1 = uStopPositions[i];
    if (t <= p1) {
      float p0 = uStopPositions[i - 1];
      float f = clamp((t - p0) / max(p1 - p0, 1e-6), 0.0, 1.0);
      return oklabMix(uStopColors[i - 1], uStopColors[i], f);
    }
  }
  return uStopColors[uStopCount - 1];
}

// --- Solid bands (existing additive feature) ------------------------------
vec3 evalBands(float elev, float cosElev) {
  vec3 sum = vec3(0.0);
  float poleDamp = smoothstep(0.0, 0.2, cosElev);
  for (int i = 0; i < MAX_BANDS; i++) {
    if (i >= uBandCount) {
      break;
    }
    float w = max(uBandWidths[i], 1e-4);
    float shape = 1.0 - smoothstep(0.0, w, abs(elev - uBandCenters[i]));
    float fade = uBandFadeRates[i] == 0.0 ? 1.0 : (0.5 + 0.5 * sin(uTime * uBandFadeRates[i] + uBandFadePhases[i]));
    sum += uBandColors[i] * (shape * uBandIntensities[i] * fade * poleDamp);
  }
  return sum;
}

// Configurable fBm for haze layers. Parameterizes octave count, lacunarity,
// and gain (all fixed in the default `fbm` from noise.frag). Amplitude is
// normalized so output remains in [0, 1] regardless of octave count / gain —
// so dialing octaves up/down doesn't shift the overall density.
float skyFbm(vec3 x, int octaves, float lacunarity, float gain) {
  float v = 0.0;
  float a = 1.0;
  float norm = 0.0;
  vec3 shift = vec3(100.0);
  for (int i = 0; i < MAX_HAZE_OCTAVES; i++) {
    if (i >= octaves) {
      break;
    }
    v += a * noise(x);
    norm += a;
    x = x * lacunarity + shift;
    a *= gain;
  }
  return v / max(norm, 1e-6);
}

// --- Canvas layer: noise-driven haze bands --------------------------------
//
// Each haze layer is an elevation-centered band whose *density* is driven by
// anisotropic 3D value noise sampled in direction space. For horizontal
// streaking, pass a large y-component in `uHazeScales[i]` and a smaller
// x/z component (noise varies quickly across elevation but slowly across
// azimuth). `uHazeSpeeds[i]` drifts the noise over time.
//
// Returns (rgb, alpha) for straight alpha-over compositing onto the gradient.
vec4 evalHaze(float elev, float azimuth, vec3 dir) {
  vec3 outColor = vec3(0.0);
  float outAlpha = 0.0;
  float cosAz = cos(azimuth);
  float sinAz = sin(azimuth);
  for (int i = 0; i < MAX_HAZES; i++) {
    if (i >= uHazeCount) {
      break;
    }
    // Domain-warp the noise sample point's elevation axis by a low-frequency
    // function of azimuth (and optionally time)
    float warpOffset = 0.0;
    if (uHazeWarp[i] != 0.0) {
      vec3 warpP = vec3(cosAz * uHazeWarpScale[i], uTime * uHazeWarpSpeed[i], sinAz * uHazeWarpScale[i]);
      warpOffset = (fbm(warpP) - 0.5) * 2.0 * uHazeWarp[i];
    }

    float w = max(uHazeWidths[i], 1e-4);
    float shape = 1.0 - smoothstep(0.0, w, abs(elev - uHazeCenters[i]));
    if (shape <= 0.0) {
      continue;
    }
    vec3 dirWarped = vec3(dir.x, dir.y + warpOffset, dir.z);
    vec3 p = dirWarped * uHazeScales[i] + uHazeSpeeds[i] * uTime;
    float f = skyFbm(p, uHazeOctaves[i], uHazeLacunarity[i], uHazeGain[i]);
    f = clamp(f + uHazeBias[i], 0.0, 1.0);
    f = pow(f, max(uHazePow[i], 1e-3));
    float edge = clamp(uHazeSharpness[i], 1e-3, 0.5);
    float density = smoothstep(0.5 - edge, 0.5 + edge, f);
    float a = clamp(shape * density * uHazeIntensities[i], 0.0, 1.0);
    // Oklab mix between the low- and high-density colors. Uses the shaped fBm
    // value (not the post-smoothstep density) so the color gradient spans the
    // full range of values that contribute to the band, not just the edge.
    vec3 hazeCol = oklabMix(uHazeColors[i], uHazeHighColors[i], clamp(f, 0.0, 1.0));
    // Alpha-over accumulation.
    outColor = outColor * (1.0 - a) + hazeCol * a;
    outAlpha = outAlpha + a * (1.0 - outAlpha);
  }
  return vec4(outColor, outAlpha);
}

// --- Canvas layer: procedural starfield ------------------------------------
//
// Cell-hash starfield using *adaptive rings*: the number of azimuth cells in
// each elevation ring scales with cos(elev), so cells stay roughly square on
// the sphere surface. This keeps angular star density uniform and — most
// importantly — prevents the zenith squish that caused multiple stars to
// collapse into a single pixel (aliasing near straight up).
//
// Ring boundaries are discrete, so adjacent rings with different ring counts
// don't line up perfectly; in practice this just reshuffles stars between
// neighboring rings, which reads as natural randomness rather than a seam.
vec4 evalStars(float elev, float azimuth, float cosElev) {
  if (uStarIntensity <= 0.0) {
    return vec4(0.0);
  }
  float elevFade = smoothstep(uStarMinElev - 0.03, uStarMinElev + 0.03, elev);
  if (elevFade <= 0.0) {
    return vec4(0.0);
  }

  float vCells = max(1.0, floor(uStarDensity * 0.5 + 0.5));
  float v = elev * 0.5 + 0.5;
  float ring = floor(v * vCells);

  float cellsPerRing = max(1.0, floor(uStarDensity * cosElev + 0.5));
  float u = azimuth / TWO_PI + 0.5;
  float cellX = mod(floor(u * cellsPerRing), cellsPerRing);
  vec2 cell = vec2(cellX, ring);
  vec2 local = vec2(fract(u * cellsPerRing), fract(v * vCells));

  float present = hash(cell);
  if (present > uStarThreshold) {
    return vec4(0.0);
  }

  vec2 starPos = vec2(hash(cell + vec2(1.3, 2.7)), hash(cell + vec2(4.7, 6.1)));
  float d = distance(local, starPos);
  float point = smoothstep(uStarSize, 0.0, d);
  if (point <= 0.0) {
    return vec4(0.0);
  }

  float phase = hash(cell + vec2(7.7, 9.3)) * TWO_PI;
  float twinkle = 0.55 + 0.45 * sin(uTime * uStarTwinkleSpeed + phase);

  float brightness = point * twinkle * uStarIntensity * elevFade;
  return vec4(uStarColor * brightness, brightness);
}

void main() {
  // --- Sky coordinates ----------------------------------------------------
  vec3 dir = normalize(vWorldPos - cameraPosition);

  float dy = clamp(dir.y, -1.0, 1.0);
  float elev = asin(dy) / HALF_PI;             // [-1, 1] angle-uniform
  float cosElev = sqrt(max(1.0 - dy * dy, 0.0)); // 0 at poles, 1 at horizon
  float azimuth = atan(dir.z, dir.x);            // [-pi, pi]

  // UV helpers for new canvas layers:
  //   skyUv  (equirectangular): vec2(azimuth/TWO_PI + 0.5, elev*0.5 + 0.5)
  //   diskUv (hemispherical disk, zenith at origin, horizon on unit circle):
  //          cosElev * vec2(cos(azimuth), sin(azimuth))

  // --- Base gradient + horizon ------------------------------------------
  float relElev = elev - uHorizonOffset;
  float tAbove = clamp(relElev, 0.0, 1.0);
  vec3 aboveColor = evalGradient(tAbove);

  vec3 belowColor;
  if (uHorizonMode == HORIZON_MODE_MIRROR) {
    belowColor = evalGradient(clamp(-relElev, 0.0, 1.0));
  } else if (uHorizonMode == HORIZON_MODE_EXTEND) {
    belowColor = evalGradient(0.0);
  } else {
    belowColor = uBelowColor;
  }

  float horizonBlend = smoothstep(-uHorizonBlend, uHorizonBlend, relElev);
  vec3 color = mix(belowColor, aboveColor, horizonBlend);

  // --- Canvas overlays (clipped to sky side of the horizon) --------------
  // Solid bands: additive.
  color += evalBands(relElev, cosElev) * horizonBlend;

  // Stars: additive (composited under the haze so clouds occlude them).
  vec4 stars = evalStars(relElev, azimuth, cosElev);
  color += stars.rgb * horizonBlend;

  // Noise haze: alpha-over onto the gradient + stars.
  vec4 haze = evalHaze(relElev, azimuth, dir);
  haze.a *= horizonBlend;
  color = color * (1.0 - haze.a) + haze.rgb * haze.a;

  fragColor = vec4(color, 1.0);
}
