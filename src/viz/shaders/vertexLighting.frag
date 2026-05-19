float totalShadow = 1.;

#if defined(USE_SHADOWMAP)
  float computedShadow;

  #if (NUM_DIR_LIGHTS > 0) && (NUM_DIR_LIGHT_SHADOWS > 0)
    DirectionalLightShadow vtxDirShadow;
    #pragma unroll_loop_start
    for (int i = 0; i < NUM_DIR_LIGHTS; i++) {
      #if (UNROLLED_LOOP_INDEX < NUM_DIR_LIGHT_SHADOWS)
        vtxDirShadow = directionalLightShadows[i];
        computedShadow = getShadow(directionalShadowMap[i], vtxDirShadow.shadowMapSize, vtxDirShadow.shadowBias, vtxDirShadow.shadowRadius, vDirectionalShadowCoord[i]);
        totalShadow *= computedShadow;
      #endif
    }
    #pragma unroll_loop_end
  #endif

  #if (NUM_SPOT_LIGHTS > 0) && (NUM_SPOT_LIGHT_SHADOWS > 0)
    SpotLightShadow vtxSpotShadow;
    #pragma unroll_loop_start
    for (int i = 0; i < NUM_SPOT_LIGHTS; i++) {
      #if (UNROLLED_LOOP_INDEX < NUM_SPOT_LIGHT_SHADOWS)
        vtxSpotShadow = spotLightShadows[i];
        computedShadow = getShadow(spotShadowMap[i], vtxSpotShadow.shadowMapSize, vtxSpotShadow.shadowBias, vtxSpotShadow.shadowRadius, vSpotLightCoord[i]);
        totalShadow *= computedShadow;
      #endif
    }
    #pragma unroll_loop_end
  #endif

  #if (NUM_POINT_LIGHTS > 0) && (NUM_POINT_LIGHT_SHADOWS > 0)
    PointLightShadow vtxPointShadow;
    #pragma unroll_loop_start
    for (int i = 0; i < NUM_POINT_LIGHTS; i++) {
      #if (UNROLLED_LOOP_INDEX < NUM_POINT_LIGHT_SHADOWS)
        vtxPointShadow = pointLightShadows[i];
        computedShadow = getPointShadow(pointShadowMap[i], vtxPointShadow.shadowMapSize, vtxPointShadow.shadowBias, vtxPointShadow.shadowRadius, vPointShadowCoord[i], vtxPointShadow.shadowCameraNear, vtxPointShadow.shadowCameraFar);
        totalShadow *= computedShadow;
      #endif
    }
    #pragma unroll_loop_end
  #endif
#endif

vec3 totalDiffuse = diffuseColor.rgb * RECIPROCAL_PI * (vVertexDirect * totalShadow + vVertexIndirect);
vec3 totalSpecular = ${vertexLightingShininess > 0 ? 'vVertexSpecular * totalShadow' : 'vec3(0.0)'};