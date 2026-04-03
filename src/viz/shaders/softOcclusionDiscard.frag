float highlightFactor = 0.;
float softOcclusionAlpha = 1.;
float occlusionNormalAlignment = 0.;
float segLen = 0.;
vec3 segDir = vec3(0., 0., 0.);
float segLenSq = 0.;
// Soft camera occlusion: discard fragments inside the reveal cylinder via Bayer dithering.
// Requires: vWorldPos (vec3 world-space fragment position), getBayer4x4, occlusionParams,
//           occlusionStart, occlusionEnd — all provided by softOcclusionPreamble.frag.
if (occlusionParams.z > 0.5) {
  vec3 seg = occlusionEnd - occlusionStart;
  segLenSq = dot(seg, seg);
  if (segLenSq > 0.0001) {
    segLen = sqrt(segLenSq);
    segDir = seg / segLen;

    // Normal-based occlusion weight: surfaces facing the camera ray (platforms between
    // camera and player) get full dithering; surfaces parallel to the ray (walls beside
    // the player) are left opaque.
    occlusionNormalAlignment = abs(dot(normalize(vWorldNormal), segDir));

    if (gl_FrontFacing) {
      vec3 toFrag = vWorldPos - occlusionStart;
      float rawT = dot(toFrag, seg) / segLenSq;
      // Fade occlusion in over occlusionParams.w metres ahead of the player eye.
      // projDist < 0 = behind eye; smoothstep ramps to full dither at eyeMargin metres.
      float projDist = rawT * segLen;
      float eyeMargin = occlusionParams.w;
      float eyeFade = smoothstep(0.0, max(eyeMargin, 0.001), projDist);
      
      // float normalAlignment = abs(dot(vec3(0., 1., 0.), segDir));
      float occlusionWeight = smoothstep(0.2, 0.7, occlusionNormalAlignment);

      float t = clamp(rawT, 0.0, 1.0);
      float dist = distance(vWorldPos, occlusionStart + t * seg);
      float revealRadius = occlusionParams.x;
      // TEST: approximately normalize `revealRadius` based on distance to camera with a baseline of 25 units
      float distanceToCamera = distance(cameraPosition, vWorldPos);
      revealRadius = max(revealRadius * mix(distanceToCamera / 15.0, 1., 0.5), revealRadius * 0.7);

      float revealFade = occlusionParams.y;
      if (dist < revealRadius) {
        softOcclusionAlpha = smoothstep(revealRadius - revealFade, revealRadius, dist);
        // mix toward 1. when eyeFade is low or surface is parallel to the ray.
        float combinedWeight = eyeFade * occlusionWeight;
        softOcclusionAlpha = mix(1., softOcclusionAlpha, combinedWeight);

        if (softOcclusionAlpha <= getBayer4x4(gl_FragCoord.xy) - 0.1) {
          discard;
        } else {
          // Highlight at the edge of the dither boundary, gated by the same occlusion weight
          // so walls that aren't being dithered don't show a spurious highlight ring.
          float edgeT = dist / revealRadius;
          highlightFactor = smoothstep(0.6, 0.85, edgeT) * (1.0 - smoothstep(0.85, 1.0, edgeT)) * combinedWeight;
        }
      }
    }
  }
}
