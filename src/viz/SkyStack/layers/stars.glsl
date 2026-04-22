uniform vec3 uStarColor_$ID;
uniform float uStarIntensity_$ID;
uniform float uStarDensity_$ID;
uniform float uStarThreshold_$ID;
uniform float uStarSize_$ID;
uniform float uStarTwinkleSpeed_$ID;
uniform float uStarTwinkleDepth_$ID;
uniform float uStarMinElev_$ID;
uniform float uStarFadeRange_$ID;

// Returns (color * brightness, brightness) — premul-ish so accumulation stays
// additive in the emissive channel.
vec4 sampleStars_$ID(vec3 dir, float elev, float azimuth, float cosElev) {
  if (uStarIntensity_$ID <= 0.0) {
    return vec4(0.0);
  }

  float horizonAlpha = smoothstep(
    uStarMinElev_$ID - uStarFadeRange_$ID,
    uStarMinElev_$ID + uStarFadeRange_$ID,
    elev
  );
  if (horizonAlpha <= 0.0) {
    return vec4(0.0);
  }

  float vCells = max(1.0, floor(uStarDensity_$ID * 0.5 + 0.5));
  float v = elev * 0.5 + 0.5;
  float ring = floor(v * vCells);

  float cellsPerRing = max(1.0, floor(uStarDensity_$ID * cosElev + 0.5));
  float u = azimuth / TWO_PI + 0.5;
  float cellX = mod(floor(u * cellsPerRing), cellsPerRing);
  vec2 cell = vec2(cellX, ring);
  vec2 local = vec2(fract(u * cellsPerRing), fract(v * vCells));

  float present = hash(cell);
  if (present > uStarThreshold_$ID) {
    return vec4(0.0);
  }

  vec2 starPos = vec2(hash(cell + vec2(1.3, 2.7)), hash(cell + vec2(4.7, 6.1)));
  float d = distance(local, starPos);
  float point = smoothstep(uStarSize_$ID, 0.0, d);
  if (point <= 0.0) {
    return vec4(0.0);
  }

  float fastPhase = hash(cell + vec2(7.7, 9.3)) * TWO_PI;
  float slowPhase = hash(cell + vec2(5.1, 2.9)) * TWO_PI;
  float fast = 0.5 + 0.5 * sin(uTime * uStarTwinkleSpeed_$ID + fastPhase);
  float slow = 0.5 + 0.5 * sin(uTime * uStarTwinkleSpeed_$ID * 0.15 + slowPhase);
  float flickerMag = smoothstep(0.4, 1.0, slow);
  float twinkle = 1.0 - uStarTwinkleDepth_$ID * flickerMag * fast;

  float brightness = point * twinkle * uStarIntensity_$ID * horizonAlpha;
  return vec4(uStarColor_$ID * brightness, brightness);
}
