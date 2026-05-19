if (playerShadowParams.y > 0.0) {
  float psRadius = playerShadowParams.x;
  float psCenterReceiverY = playerShadowParams.z;
  float psCenterDropDist = playerShadowParams.w;

  vec2 psDelta = vWorldPos.xz - playerShadowPos.xz;
  float psDist = length(psDelta);
  float psCircle = 1.0 - smoothstep(psRadius * 0.6, psRadius, psDist);

  // Polar bilinear interpolation of receiver Y from ring probes
  float psAngle = atan(psDelta.y, psDelta.x); // -PI to PI
  float psSector = fract(psAngle / 6.2831853) * 8.0; // 0 to 8
  float psSectorFrac = fract(psSector);
  int psIdx0 = int(mod(floor(psSector), 8.0));
  int psIdx1 = int(mod(floor(psSector) + 1.0, 8.0));

  // Look up receiverY from ring mat4 by index, using max() to bias toward closest surface
  // mat4 layout: cols 0-1 = outer ring, cols 2-3 = inner ring
  // psRingData[col][row] where col = i/4, row = i%4
  float psOuterY = max(psRingData[psIdx0 / 4][psIdx0 - (psIdx0 / 4) * 4], psRingData[psIdx1 / 4][psIdx1 - (psIdx1 / 4) * 4]);
  float psInnerY = max(psRingData[2 + psIdx0 / 4][psIdx0 - (psIdx0 / 4) * 4], psRingData[2 + psIdx1 / 4][psIdx1 - (psIdx1 / 4) * 4]);

  // Radial interpolation: center → inner ring → outer ring, biased toward closest surface
  float psRadialT = clamp(psDist / psRadius, 0.0, 1.0);
  float psReceiverY;
  if (psRadialT < 0.5) {
    psReceiverY = max(psCenterReceiverY, psInnerY);
  } else {
    psReceiverY = max(psInnerY, psOuterY);
  }

  // Drop distance derived from final receiverY
  float psDropDist = playerShadowPos.y - psReceiverY;

  // Asymmetric surface check: tight above, gradual bleed below
  float psYDiff = vWorldPos.y - psReceiverY;
  float psOnSurface = psYDiff > 0.0
    ? 1.0 - smoothstep(0.0, 0.3, psYDiff)
    : 1.0 - smoothstep(0.0, 1.5, -psYDiff);

  // Skip undersides and vertical walls
  float psNormalUp = smoothstep(0.2, 0.5, vWorldNormal.y);

  // Fade shadow with height above surface
  float psHeightFade = 1. - smoothstep(0., 40., psDropDist);

  float psShadow = psCircle * psOnSurface * psNormalUp * psHeightFade * playerShadowParams.y;
  totalDiffuse *= (1. - psShadow);
  totalSpecular *= (1. - psShadow);
  totalShadow *= (1. - psShadow);
}
