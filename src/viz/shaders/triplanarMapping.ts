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

export const buildTriplanarDefsFragment = ({
  contrastPreservationFactor,
  sharpenFactor,
}: TriplanarMappingParams) => `
  // sharpenFactor < 1 smooths, > 1 sharpens
  vec3 generateTriplanarWeights(vec3 normal) {
    vec3 weights = abs(normal);
    weights = pow(weights, vec3(${sharpenFactor.toFixed(
      3
    )})); // sharpen to get more weight on the dominant axis
    weights = weights / dot(weights, vec3(1.)); // normalize
    return weights;
  }

  vec4 triplanarTexture(sampler2D map, vec3 pos, vec2 uvScale, vec3 normal) {
    vec3 weights = generateTriplanarWeights(normal);

    vec4 outColor = vec4(0.);
    if (weights.x > 0.01) {
      outColor += texture2D(map, pos.yz * uvScale) * weights.x;
    }
    if (weights.y > 0.01) {
      outColor += texture2D(map, pos.zx * uvScale) * weights.y;
    }
    if (weights.z > 0.01) {
      outColor += texture2D(map, pos.xy * uvScale) * weights.z;
    }
    return outColor;
  }

  vec4 triplanarTextureFixContrast(sampler2D map, vec3 pos, vec2 uvScale, vec3 normal) {
    vec3 weights = generateTriplanarWeights(normal);

    vec4 outColor = vec4(0.);
    if (weights.x > 0.01) {
      outColor += texture2D(map, pos.yz * uvScale) * weights.x;
    }
    if (weights.y > 0.01) {
      outColor += texture2D(map, pos.zx * uvScale) * weights.y;
    }
    if (weights.z > 0.01) {
      outColor += texture2D(map, pos.xy * uvScale) * weights.z;
    }

    ${
      contrastPreservationFactor > 0
        ? `
      vec4 meanTextureColor = srgb2rgb(texture(map, vec2(0.5, 0.5), 99.));
      // contrast preserving interp. cf https://www.shadertoy.com/view/4dcSDr
      float divisor = sqrt(weights.x * weights.x + weights.y * weights.y + weights.z * weights.z);
      vec4 contrastCorrected = meanTextureColor + (outColor - meanTextureColor) * divisor;
      outColor = mix(outColor, contrastCorrected, ${contrastPreservationFactor.toFixed(3)});
    `
        : ''
    }
    return outColor;
  }`;
