// Gates ordered cheapest-first: raw compares only (no ALU) until a fragment is
// inside the probe Y band, squared-distance circle test before the sqrt.
if (
  playerShadowParams.y > 0. &&
  vWorldNormal.y > 0.2 &&
  vWorldPos.y < playerShadowParams.w + 0.4 &&
  vWorldPos.y > playerShadowParams.z - 1.6
) {
  float psRadius = playerShadowParams.x;
  vec2 psDelta = vWorldPos.xz - playerShadowPos.xz;

  if (dot(psDelta, psDelta) < psRadius * psRadius) {
    float psDist = length(psDelta);
    float psCircle = 1. - smoothstep(psRadius * 0.6, psRadius, psDist);

    // Bilateral reconstruction over the 4 surrounding grid probes: bilinear
    // weights gated by Y-proximity to this fragment, so it references only
    // probes on its own surface. Plain bilinear chased heights blended across
    // steps/ledges, eating the shadow on the upper surface and smearing it
    // partway down; missed rays (parked ~50 below) poisoned their neighborhood.
    // Both now just lose their weight.
    vec2 psTc = clamp((psDelta / psRadius * 0.5 + 0.5) * 7., 0., 7.);
    vec2 psCell = min(floor(psTc), vec2(6.));
    vec2 psF = psTc - psCell;
    int psBase = int(psCell.y) * 8 + int(psCell.x);
    float psY00 = psGridFetch(psBase);
    float psY10 = psGridFetch(psBase + 1);
    float psY01 = psGridFetch(psBase + 8);
    float psY11 = psGridFetch(psBase + 9);

    float psW00 = (1. - psF.x) * (1. - psF.y) * (1. - smoothstep(0.25, 1.25, abs(vWorldPos.y - psY00)));
    float psW10 = psF.x * (1. - psF.y) * (1. - smoothstep(0.25, 1.25, abs(vWorldPos.y - psY10)));
    float psW01 = (1. - psF.x) * psF.y * (1. - smoothstep(0.25, 1.25, abs(vWorldPos.y - psY01)));
    float psW11 = psF.x * psF.y * (1. - smoothstep(0.25, 1.25, abs(vWorldPos.y - psY11)));
    float psWSum = psW00 + psW10 + psW01 + psW11;

    // Spatial weights sum to 1, so psWSum is the fraction of probe support that
    // agrees with this fragment's height; fade where support is thin (over a
    // void, under an overhang).
    float psValidity = smoothstep(0.1, 0.4, psWSum);
    if (psValidity > 0.) {
      float psReceiverY = (psW00 * psY00 + psW10 * psY10 + psW01 * psY01 + psW11 * psY11) / psWSum;
      float psDropDist = playerShadowPos.y - psReceiverY;

      // Asymmetric surface check: tight above, gradual bleed below
      float psYDiff = vWorldPos.y - psReceiverY;
      float psOnSurface = psYDiff > 0.
        ? 1. - smoothstep(0., 0.4, psYDiff)
        : 1. - smoothstep(0., 1.5, -psYDiff);

      // Fade shadow with height above surface
      float psHeightFade = 1. - smoothstep(0., 40., psDropDist);

      float psNormalUp = smoothstep(0.2, 0.5, vWorldNormal.y);
      float psShadow = psCircle * psOnSurface * psValidity * psNormalUp * psHeightFade * playerShadowParams.y;
      float lightFactor = 1. - psShadow;
      totalDiffuse *= lightFactor;
      totalSpecular *= lightFactor;
      totalShadow *= lightFactor;
    }
  }
}
