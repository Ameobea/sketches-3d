export interface TriplanarMappingParams {
  /**
   * Number from 0 to 1 controlling the mix factor for contrast-preserving blending (https://www.shadertoy.com/view/4dcSDr).
   *
   * If 0, no contrast preservation is done.
   */
  contrastPreservationFactor: number;
  /**
   * Number from 1 to infinity controlling the amount of sharpening to apply to the weights.
   *
   * This is the exponent that weights are raised to before being normalized.  Higher numbers
   * reduce the area in which different axes are blended together making the transitions between
   * axes sharper.
   */
  sharpenFactor: number;
}

/**
 * `buildSampleExpr` substitutes the per-axis texture fetch (e.g. a
 * tile-breaking wrapper). `tileBreakingMode` controls how
 * `getCombinedTriplanarTapCount` reports per-axis cost.
 */
export const buildTriplanarDefsFragment = (
  { contrastPreservationFactor, sharpenFactor }: TriplanarMappingParams,
  buildSampleExpr: (sampler: string, uv: string) => string = (s, u) => `texture2D(${s}, ${u})`,
  tileBreakingMode: 'none' | 'neyret' = 'none'
) => {
  const perAxisTapCountExpr = (axisUv: string) =>
    tileBreakingMode === 'neyret' ? `getNeyretTapCount(${axisUv})` : '1.0';

  return `
  // sharpenFactor < 1 smooths, > 1 sharpens
  vec3 generateTriplanarWeights(vec3 normal) {
    vec3 weights = abs(normal);
    weights = pow(weights, vec3(${sharpenFactor.toFixed(
      3
    )})); // sharpen to get more weight on the dominant axis
    weights = weights / (weights.x + weights.y + weights.z);
    return weights;
  }

  // Per-fragment tap count matching the > 0.01 weight skip in the real
  // sample path, for cost visualization.
  float getCombinedTriplanarTapCount(vec3 pos, vec3 normal, vec2 uvScale) {
    vec3 w = generateTriplanarWeights(normal);
    float total = 0.0;
    if (w.x > 0.01) total += ${perAxisTapCountExpr('pos.yz * uvScale')};
    if (w.y > 0.01) total += ${perAxisTapCountExpr('pos.zx * uvScale')};
    if (w.z > 0.01) total += ${perAxisTapCountExpr('pos.xy * uvScale')};
    return total;
  }

  vec4 triplanarTexture(sampler2D map, vec3 pos, vec2 uvScale, vec3 normal) {
    vec3 weights = generateTriplanarWeights(normal);

    vec4 outColor = vec4(0.);
    if (weights.x > 0.01) {
      outColor += ${buildSampleExpr('map', 'pos.yz * uvScale')} * weights.x;
    }
    if (weights.y > 0.01) {
      outColor += ${buildSampleExpr('map', 'pos.zx * uvScale')} * weights.y;
    }
    if (weights.z > 0.01) {
      outColor += ${buildSampleExpr('map', 'pos.xy * uvScale')} * weights.z;
    }
    return outColor;
  }

  // Adjusted version that works for normal maps
  //
  // Adapted from this code:
  // https://github.com/bgolus/Normal-Mapping-for-a-Triplanar-Shader/blob/a3571bf5f6e857e85c2f37875e79568282277de8/TriplanarGPUGems3.shader#L62
  //
  // Also see:
  // https://bgolus.medium.com/normal-mapping-for-a-triplanar-shader-10bf39dca05a
  // World-space tangent-plane perturbation (UDN-style) from a tangent-space
  // normal map, *without* the base normal added back. Adding it to a unit
  // normal and normalizing reproduces the classic triplanar normal map; POM
  // adds it to the analytic floor normal instead.
  vec3 triplanarNormalMapPerturbation(sampler2D map, vec3 pos, vec2 uvScale, vec3 normal, vec2 normalScale) {
    vec3 weights = generateTriplanarWeights(normal);
    if (weights.x < 0.01) {
      weights.x = 0.;
    }
    if (weights.y < 0.01) {
      weights.y = 0.;
    }
    if (weights.z < 0.01) {
      weights.z = 0.;
    }

    vec3 axisSign = sign(normal);

    vec2 tnormalX_xy = (${buildSampleExpr('map', 'pos.yz * uvScale')}.xy * 2. - 1.) * normalScale;
    vec2 tnormalY_xy = (${buildSampleExpr('map', 'pos.zx * uvScale')}.xy * 2. - 1.) * normalScale;
    vec2 tnormalZ_xy = (${buildSampleExpr('map', 'pos.xy * uvScale')}.xy * 2. - 1.) * normalScale;

    // correct for back-side projection by flipping the x-component
    tnormalX_xy.x *= axisSign.x;
    tnormalY_xy.x *= axisSign.y;
    tnormalZ_xy.x *= axisSign.z;

    // swizzle tangent-space normals to world-space perturbation vectors
    vec3 normalX = vec3(0.0, tnormalX_xy.y, tnormalX_xy.x);
    vec3 normalY = vec3(tnormalY_xy.x, 0.0, tnormalY_xy.y);
    vec3 normalZ = vec3(tnormalZ_xy.x, tnormalZ_xy.y, 0.0);

    return normalX * weights.x + normalY * weights.y + normalZ * weights.z;
  }

  vec4 triplanarTextureNormalMap(sampler2D map, vec3 pos, vec2 uvScale, vec3 normal, vec2 normalScale) {
    vec3 perturbation = triplanarNormalMapPerturbation(map, pos, uvScale, normal, normalScale);
    return vec4(normalize(perturbation + normal), 1.0);
  }

  vec4 triplanarTextureFixContrast(sampler2D map, vec3 pos, vec2 uvScale, vec3 normal) {
    vec3 weights = generateTriplanarWeights(normal);

    vec4 outColor = vec4(0.);
    if (weights.x > 0.01) {
      outColor += ${buildSampleExpr('map', 'pos.yz * uvScale')} * weights.x;
    }
    if (weights.y > 0.01) {
      outColor += ${buildSampleExpr('map', 'pos.zx * uvScale')} * weights.y;
    }
    if (weights.z > 0.01) {
      outColor += ${buildSampleExpr('map', 'pos.xy * uvScale')} * weights.z;
    }

    ${
      contrastPreservationFactor > 0
        ? `
      vec4 meanTextureColor = texture(map, vec2(0.5, 0.5), 99.);
      // contrast preserving interp. cf https://www.shadertoy.com/view/4dcSDr
      float divisor = sqrt(weights.x * weights.x + weights.y * weights.y + weights.z * weights.z);
      vec4 contrastCorrected = meanTextureColor + (outColor - meanTextureColor) * divisor;
      outColor = mix(outColor, contrastCorrected, ${contrastPreservationFactor.toFixed(3)});
    `
        : ''
    }
    return outColor;
  }`;
};
