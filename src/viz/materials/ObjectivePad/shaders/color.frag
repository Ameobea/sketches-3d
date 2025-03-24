vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 pulseCenter = vec2(-10., -20.);

  float period = 2.5;
  float sineVal = sin(2. * PI * (curTimeSeconds / period));
  float t = (sineVal + 1.) * 0.5;

  float easedT = (t < 0.5) ? (4. * t * t * t) : (1. - pow(-2. * t + 2., 3.) / 2.);
  float bgOpacity = mix(0.35, 0.55, easedT);

  vec3 rippleColor = vec3(0.08, 0.91, 0.95);
  vec3 baseMatColor = vec3(0.32, 0.5, 0.5);

  // float distanceFromCenter = distance(pos.xz, pulseCenter);
  // float distanceFromCenter = abs(pos.x - pulseCenter.x) + abs(pos.z - pulseCenter.y);
  float distanceFromCenter = max(abs(pos.x - pulseCenter.x), abs(pos.z - pulseCenter.y));

  float pulseSpeed = -2.5;
  float pulseSpacing = 2.6;
  float pulseWidth = 1.4;

  float modVal = mod(distanceFromCenter - curTimeSeconds * pulseSpeed, pulseSpacing);
  float distToPulse = min(modVal, pulseSpacing - modVal);

  float rippleIntensity = smoothstep(0., pulseWidth, distToPulse);

  vec3 finalColor = mix(baseMatColor, rippleColor, rippleIntensity);

  float combinedAlpha = clamp(bgOpacity + pow(rippleIntensity, 3.5) * 0.2, 0., 1.);

  return vec4(finalColor, combinedAlpha);
}
