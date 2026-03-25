uniform float bboxYMin;
uniform float bboxYMax;
uniform float fadeTopDist;
uniform float fadeTopSteepness;
uniform float fadeBottomDist;
uniform float fadeBottomSteepness;
uniform float fadeEdgeAmp;
uniform float fadeEdgeFreq;
uniform vec2 fadeEdgeSpeed;

__FADE_DEFS__
__EDGE_WARP_DEFS__

// Oklab conversion for perceptually-uniform color mixing.
// Input/output: linear RGB (no gamma). Ref: https://bottosson.github.io/posts/oklab/
vec3 rgbToOklab(vec3 c) {
  float l = 0.4122214708*c.r + 0.5363325363*c.g + 0.0514459929*c.b;
  float m = 0.2119034982*c.r + 0.6806995451*c.g + 0.1073969566*c.b;
  float s = 0.0883024619*c.r + 0.2817188376*c.g + 0.6299787005*c.b;
  float l_ = pow(max(l, 0.0), 1.0/3.0);
  float m_ = pow(max(m, 0.0), 1.0/3.0);
  float s_ = pow(max(s, 0.0), 1.0/3.0);
  return vec3(
    0.2104542553*l_ + 0.7936177850*m_ - 0.0040720468*s_,
    1.9779984951*l_ - 2.4285922050*m_ + 0.4505937099*s_,
    0.0259040371*l_ + 0.7827717662*m_ - 0.8086757660*s_
  );
}
vec3 oklabToRgb(vec3 c) {
  float l_ = c.x + 0.3963377774*c.y + 0.2158037573*c.z;
  float m_ = c.x - 0.1055613458*c.y - 0.0638541728*c.z;
  float s_ = c.x - 0.0894841775*c.y - 1.2914855480*c.z;
  return vec3(
     4.0767416621*(l_*l_*l_) - 3.3077115913*(m_*m_*m_) + 0.2309699292*(s_*s_*s_),
    -1.2684380046*(l_*l_*l_) + 2.6097574011*(m_*m_*m_) - 0.3413193965*(s_*s_*s_),
    -0.0041960863*(l_*l_*l_) - 0.7034186147*(m_*m_*m_) + 1.6956082560*(s_*s_*s_)
  );
}

vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec4 outColor = vec4(0.8, 0.5, 0.6, 0.0);

  // Vertical bias: interpolates between two bias values across the mesh height so flame
  // tips are sparser than the base, matching real flame appearance.
  float vertBias = 0.0;
  #ifdef VERT_BIAS_ACTIVE
    float vertT = clamp((pos.y - bboxYMin) / (bboxYMax - bboxYMin), 0.0, 1.0);
    vertBias = mix(__VERT_BIAS_AMT_LO__, __VERT_BIAS_AMT_HI__, smoothstep(__VERT_BIAS_LO__, __VERT_BIAS_HI__, vertT));
  #endif

  vec3 noisePos = __NOISE_ROT__ * pos;
  noisePos = quantize(noisePos, __NOISE_POS_QUANT__);
  noisePos += curTimeSeconds * __NOISE_DIR__;
  float noise_ = fbm_2_octaves(noisePos * __NOISE_FREQ__);

#ifdef EDGE_WARP_ACTIVE
  // 1D time-domain noise drives a "breeze" envelope.  Hoist before alpha so it can
  // also influence noise bias and amplitude.
  float breezeNoise = noise(curTimeSeconds * __BREEZE_TIME_FREQ__);
  float breezeT = smoothstep(__BREEZE_THRESHOLD__, __BREEZE_THRESHOLD_HI__, breezeNoise);

  noise_ = pow(max(noise_ + __NOISE_BIAS__ + breezeT * __BREEZE_BIAS_DELTA__ + vertBias, 0.), __NOISE_POW__);
  noise_ = quantize(noise_, __NOISE_QUANT__);
  outColor.a = clamp(noise_ * __NOISE_MULTIPLIER__ * (1.0 + breezeT * __BREEZE_NOISE_AMP_MULT__), 0.0, 1.0);

  outColor.rgb = oklabToRgb(mix(rgbToOklab(outColor.rgb), rgbToOklab(__BREEZE_HOT_COLOR__), breezeT * __BREEZE_COLOR_MIX__));
#else
  noise_ = pow(max(noise_ + __NOISE_BIAS__ + vertBias, 0.), __NOISE_POW__);
  noise_ = quantize(noise_, __NOISE_QUANT__);
  outColor.a = clamp(noise_ * __NOISE_MULTIPLIER__, 0.0, 1.0);
#endif

#ifdef FADE_ACTIVE
  #ifdef EDGE_WARP_ACTIVE
    // PM-style domain warping: breeze displaces sample position rather than scaling
    // frequency, keeping phase continuous (no discontinuous jumps).
    vec2 samplePos = vec2(pos.x, pos.z) * fadeEdgeFreq + curTimeSeconds * fadeEdgeSpeed;
    vec2 pmDisplace = vec2(
      noise(samplePos * __BREEZE_MOD_SCALE__ + 13.7),
      noise(samplePos * __BREEZE_MOD_SCALE__ + 27.4)
    ) * 2.0 - 1.0;
    float effectiveAmp = fadeEdgeAmp * (1.0 + breezeT * __BREEZE_AMP_MULT__);
    float edgeWarp = noise(samplePos + pmDisplace * breezeT * __BREEZE_PM_DEPTH__) * effectiveAmp;
  #else
    float edgeWarp = 0.0;
  #endif

  float topFade    = fadeTopDist    > 0.0 ? pow(clamp((bboxYMax - edgeWarp - pos.y)   / fadeTopDist,    0.0, 1.0), fadeTopSteepness)    : 1.0;
  float bottomFade = fadeBottomDist > 0.0 ? pow(clamp((pos.y - (bboxYMin + edgeWarp)) / fadeBottomDist, 0.0, 1.0), fadeBottomSteepness) : 1.0;
  outColor.a *= topFade * bottomFade;
#endif

#ifdef X_FADE_ACTIVE
  outColor.a *= 1.0 - smoothstep(__X_FADE_LO__, __X_FADE_HI__, pos.x);
#endif

  return outColor;
}
