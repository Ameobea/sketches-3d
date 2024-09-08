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
    weights = weights / (weights.x + weights.y + weights.z);
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

  // Adjusted version that works for normal maps
  //
  // Adapted from this code:
  // https://github.com/bgolus/Normal-Mapping-for-a-Triplanar-Shader/blob/master/TriplanarGPUGems3.shader#L62
  //
  // Also see:
  // https://bgolus.medium.com/normal-mapping-for-a-triplanar-shader-10bf39dca05a
  vec4 triplanarTextureNormalMap(sampler2D map, vec3 pos, vec2 uvScale, vec3 normal, vec2 normalScale) {
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

    vec2 uvX = pos.yz * uvScale;
    vec2 uvY = pos.zx * uvScale;
    vec2 uvZ = pos.xy * uvScale;

    vec3 tnormalX = vec3(0.);
    if (weights.x > 0.) {
      tnormalX = texture2D(map, uvX).xyz * vec3(normalScale, 1.);
    }
    vec3 tnormalY = vec3(0.);
    if (weights.y > 0.) {
      tnormalY = texture2D(map, uvY).xyz * vec3(normalScale, 1.);
    }
    vec3 tnormalZ = vec3(0.);
    if (weights.z > 0.) {
      tnormalZ = texture2D(map, uvZ).xyz * vec3(normalScale, 1.);
    }

    vec3 axisSign = sign(normal);

    vec3 normalX = vec3(0., tnormalX.yx);
    vec3 normalY = vec3(tnormalY.x, 0., tnormalY.y);
    vec3 normalZ = vec3(tnormalZ.xy, 0.);

    normalX *= axisSign.x;
    normalY *= axisSign.y;
    normalZ *= -axisSign.z;

    vec3 worldNormal = normalize(
      normalX * weights.x +
      normalY * weights.y +
      normalZ * weights.z +
      normal
    );
    return vec4(worldNormal, 1.);
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
