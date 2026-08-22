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

/** `grad` carries the per-axis uv gradients and texture size, computed once per function above the
 *  axis branches so samplers that need them don't take derivatives in non-uniform control flow. */
export interface SampleGrad {
  dx: string;
  dy: string;
  res: string;
}
type SampleExprBuilder = (sampler: string, uv: string, mean: string, grad: SampleGrad) => string;

/**
 * `buildSampleExpr` substitutes the per-axis texture fetch (e.g. a
 * tile-breaking wrapper); `mean` is a GLSL expression for the texture's
 * precomputed mean color. `tileBreakingMode` controls how
 * `getCombinedTriplanarTapCount` reports per-axis cost.
 *
 * With `stackSampleExpr`, a second copy of every sampler-taking function is emitted
 * overloaded on `sampler2DArray`, sampling via that builder (e.g. `sampleStack(..., _stackT)`,
 * declared by the caller's stack-helpers block). GLSL overload resolution picks by the
 * declared type of the texture uniform, so call sites are identical for singles and stacks.
 */
export const buildTriplanarDefsFragment = (
  { contrastPreservationFactor, sharpenFactor }: TriplanarMappingParams,
  buildSampleExpr: SampleExprBuilder = (s, u) => `texture2D(${s}, ${u})`,
  tileBreakingMode: 'none' | 'neyret' = 'none',
  stackSampleExpr?: SampleExprBuilder
) => {
  const perAxisTapCountExpr = (axisUv: string) =>
    tileBreakingMode === 'neyret' ? `getNeyretTapCount(${axisUv})` : '1.0';

  const samplerFns = (samplerType: string, sample: SampleExprBuilder) => {
    const axis = (sw: string) => `pos.${sw} * uvScale`;
    const grad = (sw: string): SampleGrad => ({
      dx: `_dpdx.${sw} * uvScale`,
      dy: `_dpdy.${sw} * uvScale`,
      res: '_res',
    });
    const sampleAxis = (sw: string) => sample('map', axis(sw), 'meanColor', grad(sw));
    const prelude = /* glsl */ `
    vec3 _dpdx = dFdx(pos), _dpdy = dFdy(pos);
    vec2 _res = texRes(map);`;
    return /* glsl */ `
  vec4 triplanarTexture(${samplerType} map, vec3 pos, vec2 uvScale, vec3 normal, vec4 meanColor) {
    vec3 weights = generateTriplanarWeights(normal);${prelude}

    vec4 outColor = vec4(0.);
    if (weights.x > 0.01) {
      outColor += ${sampleAxis('yz')} * weights.x;
    }
    if (weights.y > 0.01) {
      outColor += ${sampleAxis('zx')} * weights.y;
    }
    if (weights.z > 0.01) {
      outColor += ${sampleAxis('xy')} * weights.z;
    }
    return outColor;
  }

  // World-space tangent-plane perturbation (UDN-style, cf.
  // https://bgolus.medium.com/normal-mapping-for-a-triplanar-shader-10bf39dca05a) from a
  // tangent-space normal map, *without* the base normal added back. Adding it to a unit
  // normal and normalizing reproduces the classic triplanar normal map; POM adds it to the
  // analytic floor normal instead. Each projection's (u,v) is a fixed pair of world axes —
  // yz / zx / xy — so tangent x perturbs along u's axis and tangent y along v's, on back
  // faces too (a height-field normal only depends on ∂p/∂u, ∂p/∂v, not the frame's handedness).
  vec3 triplanarNormalMapPerturbation(${samplerType} map, vec3 pos, vec2 uvScale, vec3 normal, vec2 normalScale, vec4 meanColor) {
    vec3 weights = generateTriplanarWeights(normal);${prelude}
    if (weights.x < 0.01) {
      weights.x = 0.;
    }
    if (weights.y < 0.01) {
      weights.y = 0.;
    }
    if (weights.z < 0.01) {
      weights.z = 0.;
    }

    vec2 tnormalX_xy = vec2(0.), tnormalY_xy = vec2(0.), tnormalZ_xy = vec2(0.);
    if (weights.x > 0.) tnormalX_xy = (${sampleAxis('yz')}.xy * 2. - 1.) * normalScale;
    if (weights.y > 0.) tnormalY_xy = (${sampleAxis('zx')}.xy * 2. - 1.) * normalScale;
    if (weights.z > 0.) tnormalZ_xy = (${sampleAxis('xy')}.xy * 2. - 1.) * normalScale;

    vec3 normalX = vec3(0.0, tnormalX_xy.x, tnormalX_xy.y);
    vec3 normalY = vec3(tnormalY_xy.y, 0.0, tnormalY_xy.x);
    vec3 normalZ = vec3(tnormalZ_xy.x, tnormalZ_xy.y, 0.0);

    return normalX * weights.x + normalY * weights.y + normalZ * weights.z;
  }

  vec4 triplanarTextureNormalMap(${samplerType} map, vec3 pos, vec2 uvScale, vec3 normal, vec2 normalScale, vec4 meanColor) {
    vec3 perturbation = triplanarNormalMapPerturbation(map, pos, uvScale, normal, normalScale, meanColor);
    return vec4(normalize(perturbation + normal), 1.0);
  }

  vec4 triplanarTextureFixContrast(${samplerType} map, vec3 pos, vec2 uvScale, vec3 normal, vec4 meanColor) {
    vec3 weights = generateTriplanarWeights(normal);${prelude}

    vec4 outColor = vec4(0.);
    if (weights.x > 0.01) {
      outColor += ${sampleAxis('yz')} * weights.x;
    }
    if (weights.y > 0.01) {
      outColor += ${sampleAxis('zx')} * weights.y;
    }
    if (weights.z > 0.01) {
      outColor += ${sampleAxis('xy')} * weights.z;
    }

    ${
      contrastPreservationFactor > 0
        ? `
      // contrast preserving interp. cf https://www.shadertoy.com/view/4dcSDr
      float divisor = sqrt(weights.x * weights.x + weights.y * weights.y + weights.z * weights.z);
      vec4 contrastCorrected = meanColor + (outColor - meanColor) * divisor;
      outColor = mix(outColor, contrastCorrected, ${contrastPreservationFactor.toFixed(3)});
    `
        : ''
    }
    return outColor;
  }`;
  };

  return /* glsl */ `
  vec2 texRes(sampler2D s) { return vec2(textureSize(s, 0)); }
  vec2 texRes(sampler2DArray s) { return vec2(textureSize(s, 0).xy); }
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
${samplerFns('sampler2D', buildSampleExpr)}
${stackSampleExpr ? samplerFns('sampler2DArray', stackSampleExpr) : ''}`;
};
