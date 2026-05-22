if (playerShadowParams.y > 0.) {
  float psNormalUp = smoothstep(0.2, 0.5, vWorldNormal.y);
  if (psNormalUp > 0. && vWorldPos.y < playerShadowParams.w + 0.3) {
    float psRadius = playerShadowParams.x;

    vec2 psDelta = vWorldPos.xz - playerShadowPos.xz;
    float psDist = length(psDelta);
    float psCircle = 1. - smoothstep(psRadius * 0.6, psRadius, psDist);

    if (psCircle > 0.) {
      float psCenterReceiverY = playerShadowParams.z;

      // Angular lookup of receiver Y from ring probes
      float psAngle = atan(psDelta.y, psDelta.x); // -PI to PI
      float psSector = fract(psAngle / 6.2831853) * 8.; // 0 to 8
      int psIdx0 = int(floor(psSector)); // 0..7, fract() keeps this in range
      int psIdx1 = int(mod(float(psIdx0) + 1., 8.));

      // Look up receiverY by sector index, using max() to bias toward closest surface.
      // Flat layout: [0..7] = outer ring (angles 0-7), [8..15] = inner ring.
      float psOuterY = max(psRingData[psIdx0], psRingData[psIdx1]);
      float psInnerY = max(psRingData[8 + psIdx0], psRingData[8 + psIdx1]);

      // Radial interpolation: center → inner ring → outer ring, biased toward closest surface
      float psRadialT = clamp(psDist / psRadius, 0., 1.);
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
      float psOnSurface = psYDiff > 0.
        ? 1. - smoothstep(0., 0.3, psYDiff)
        : 1. - smoothstep(0., 1.5, -psYDiff);

      // Fade shadow with height above surface
      float psHeightFade = 1. - smoothstep(0., 40., psDropDist);

      float psShadow = psCircle * psOnSurface * psNormalUp * psHeightFade * playerShadowParams.y;
      float lightFactor = 1. - psShadow;
      totalDiffuse *= lightFactor;
      totalSpecular *= lightFactor;
      totalShadow *= psShadow;
    }
  }
}
