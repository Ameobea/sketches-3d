uniform float uBuildingCount_$ID;
uniform float uBuildingPresence_$ID;
uniform float uBuildingGap_$ID;
uniform float uBuildingMinHeight_$ID;
uniform float uBuildingMaxHeight_$ID;
uniform float uFloorsMin_$ID;
uniform float uFloorsMax_$ID;
uniform float uWindowsMin_$ID;
uniform float uWindowsMax_$ID;
uniform float uMaxFloorStride_$ID;
uniform float uMaxWindowStride_$ID;
uniform float uLitFractionMin_$ID;
uniform float uLitFractionMax_$ID;
uniform float uGroundElev_$ID;
uniform float uWindowWidth_$ID;
uniform float uWindowHeight_$ID;
uniform vec3 uCityColor_$ID;
uniform vec3 uCityColorAlt_$ID;
uniform float uCityIntensity_$ID;
uniform float uBuildingTwinkleSpeed_$ID;
uniform float uBuildingTwinkleDepth_$ID;
uniform vec3 uSilhouetteColor_$ID;

struct BuildingHit_$ID {
  bool hasBody;
  vec2 bCell;
  float localX;
  float localY;
  float floorIdx;
  float windowIdx;
  vec2 cellLocal;
  float floorCount;
  float windowCount;
  float floorStride;
  float windowStride;
  float litFrac;
  float colorH;
  // Analytic per-pixel change rate of the window/floor grid coordinates,
  // replacing fwidth() which is undefined after discardIfOccluded().
  float cellRate;
};

BuildingHit_$ID probeBuilding_$ID(float elev, float azimuth, float cosElev) {
  BuildingHit_$ID hit;
  hit.hasBody = false;
  hit.bCell = vec2(0.0);
  hit.localX = 0.0;
  hit.localY = 0.0;
  hit.floorIdx = 0.0;
  hit.windowIdx = 0.0;
  hit.cellLocal = vec2(0.0);
  hit.floorCount = 1.0;
  hit.windowCount = 1.0;
  hit.floorStride = 1.0;
  hit.windowStride = 1.0;
  hit.litFrac = 0.0;
  hit.colorH = 0.0;
  hit.cellRate = 0.0;

  if (elev < uGroundElev_$ID || elev > uGroundElev_$ID + uBuildingMaxHeight_$ID) {
    return hit;
  }

  float az01 = azimuth / TWO_PI + 0.5;

  float slotF = az01 * uBuildingCount_$ID;
  float slotIdx = floor(slotF);
  float slotLocal = fract(slotF);
  vec2 bCell = vec2(slotIdx, 0.0);

  float presentH = hash(bCell + vec2(11.1, 13.3));
  if (presentH > uBuildingPresence_$ID) {
    return hit;
  }

  float halfGap = clamp(uBuildingGap_$ID, 0.0, 0.98) * 0.5;
  float bodyWidth = max(1.0 - 2.0 * halfGap, 1e-4);
  if (slotLocal < halfGap || slotLocal > 1.0 - halfGap) {
    return hit;
  }
  float localX = (slotLocal - halfGap) / bodyWidth;

  float heightH = hash(bCell + vec2(17.3, 23.5));
  float buildingHeight = mix(uBuildingMinHeight_$ID, uBuildingMaxHeight_$ID, heightH);
  float topElev = uGroundElev_$ID + buildingHeight;
  if (elev > topElev) {
    return hit;
  }
  float localY = (elev - uGroundElev_$ID) / max(buildingHeight, 1e-4);

  float floorsH = hash(bCell + vec2(2.3, 5.7));
  float windowsH = hash(bCell + vec2(8.9, 4.1));
  float floorCount = max(1.0, floor(mix(uFloorsMin_$ID, uFloorsMax_$ID, floorsH) + 0.5));
  float windowCount = max(1.0, floor(mix(uWindowsMin_$ID, uWindowsMax_$ID, windowsH) + 0.5));

  float floorStrideH = hash(bCell + vec2(7.7, 9.3));
  float windowStrideH = hash(bCell + vec2(5.1, 2.9));
  float floorStride = max(1.0, floor(mix(1.0, uMaxFloorStride_$ID, floorStrideH) + 0.5));
  float windowStride = max(1.0, floor(mix(1.0, uMaxWindowStride_$ID, windowStrideH) + 0.5));

  float litH = hash(bCell + vec2(3.1, 19.7));
  float litFrac = mix(uLitFractionMin_$ID, uLitFractionMax_$ID, litH);

  float colorH = hash(bCell + vec2(29.3, 31.7));

  float floorF = localY * floorCount;
  float windowF = localX * windowCount;
  float floorIdx = floor(floorF);
  float windowIdx = floor(windowF);
  vec2 cellLocal = vec2(fract(windowF), fract(floorF));

  // Analytic pixel footprint in floor/window grid space.
  // Ref: https://iquilezles.org/articles/filtering/
  // floorF = localY * floorCount, where localY = (elev - ground) / buildingHeight.
  // One pixel spans angPx radians; in elevation units that's angPx / (HALF_PI * cosElev)
  // (accounting for the asin nonlinearity). Similarly for the azimuth → window mapping.
  float angPx = 2.0 * abs(uProjectionMatrixInverse[1][1])
               / float(textureSize(uSceneDepth, 0).y);
  float ce = max(cosElev, 0.01);
  float dFloor = floorCount / max(buildingHeight, 1e-4) * angPx / (HALF_PI * ce);
  float dWindow = windowCount / bodyWidth * uBuildingCount_$ID / TWO_PI * angPx / ce;

  hit.hasBody = true;
  hit.bCell = bCell;
  hit.localX = localX;
  hit.localY = localY;
  hit.floorIdx = floorIdx;
  hit.windowIdx = windowIdx;
  hit.cellLocal = cellLocal;
  hit.floorCount = floorCount;
  hit.windowCount = windowCount;
  hit.floorStride = floorStride;
  hit.windowStride = windowStride;
  hit.litFrac = litFrac;
  hit.colorH = colorH;
  hit.cellRate = max(dFloor, dWindow);
  return hit;
}

// Returns (color * brightness, brightness) for a lit window at this fragment,
// or vec4(0) if outside a building body or in a dark cell.
vec4 sampleWindows_$ID(BuildingHit_$ID hit) {
  vec3 buildingCol = mix(uCityColor_$ID, uCityColorAlt_$ID, hit.colorH);

  float cellRate = hit.cellRate;
  float lod = smoothstep(0.35, 1.0, cellRate);

  float avgCoverage = uWindowWidth_$ID * uWindowHeight_$ID * hit.litFrac /
                      max(hit.floorStride * hit.windowStride, 1.0);

  float sharp = 0.0;
  vec3 sharpCol = buildingCol;
  float twinkle = 1.0;

  bool strideOk = mod(hit.floorIdx, hit.floorStride) < 0.5 &&
                  mod(hit.windowIdx, hit.windowStride) < 0.5;
  if (strideOk) {
    vec2 wCell = hit.bCell + vec2(hit.windowIdx * 1.7 + 3.0, hit.floorIdx * 2.3 + 7.0);
    float windowLitH = hash(wCell);
    if (windowLitH <= hit.litFrac) {
      vec2 halfSize = vec2(uWindowWidth_$ID, uWindowHeight_$ID) * 0.5;
      vec2 offset = abs(hit.cellLocal - 0.5);
      vec2 outside = offset - halfSize;
      float sdf = max(outside.x, outside.y);
      float aaW = max(cellRate, 1e-5) * 0.5;
      sharp = 1.0 - smoothstep(-aaW, aaW, sdf);

      float winColorH = hash(wCell + vec2(13.3, 17.1));
      sharpCol = mix(buildingCol, uCityColorAlt_$ID, winColorH * 0.5);

      float fastPhase = windowLitH * TWO_PI;
      float slowPhase = winColorH * TWO_PI;
      float fast = 0.5 + 0.5 * sin(uTime * uBuildingTwinkleSpeed_$ID + fastPhase);
      float slow = 0.5 + 0.5 * sin(uTime * uBuildingTwinkleSpeed_$ID * 0.15 + slowPhase);
      float flickerMag = smoothstep(0.4, 1.0, slow);
      twinkle = 1.0 - uBuildingTwinkleDepth_$ID * flickerMag * fast;
    }
  }

  float coverage = mix(sharp * twinkle, avgCoverage, lod);
  if (coverage <= 0.0) {
    return vec4(0.0);
  }

  float brightness = coverage * uCityIntensity_$ID;
  return vec4(sharpCol * brightness, brightness);
}
