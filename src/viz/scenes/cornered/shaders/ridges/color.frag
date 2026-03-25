float getStripeActivation(vec3 pos, vec3 normal, float stripeWidth, float stripeSpacing, float stripeBlendWidth) {
  float stripePos;
  if (abs(normal.z) > 0.5) {
    stripePos = mod(pos.x, stripeWidth * 2.);
  } else {
    stripePos = mod(pos.z, stripeWidth * 2.);
  }
  float stripeActivation = 0.0;
  if (stripePos < stripeWidth) {
    stripeActivation = smoothstep(0., stripeBlendWidth, stripePos);
  } else if (stripePos > stripeWidth + stripeSpacing - stripeBlendWidth) {
    stripeActivation = smoothstep(stripeWidth + stripeSpacing - stripeBlendWidth, stripeWidth + stripeSpacing, stripePos);
  }
  return stripeActivation;
}

vec3 desaturate(vec3 color) {
  float gray = dot(color, vec3(0.299, 0.587, 0.114)) * 5.5;
  vec3 mixed = mix(vec3(gray), color, 0.765);
  return clamp(mixed, vec3(0.0), vec3(1.0));
}

vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec3 outColor = baseColor;

  outColor = mix(desaturate((vec3(0.22, 0.16, 0.13) * 1.8)) * outColor, outColor, getStripeActivation(pos, normal, 0.6, 0.4, 0.283));
  outColor = mix(desaturate((vec3(0.22, 0.18, 0.19) * 1.8)) * outColor, outColor, getStripeActivation(pos, normal, 0.2, 0.47, 0.283));
  outColor = mix(desaturate((vec3(0.32, 0.25, 0.13) * 1.8)) * outColor, outColor, getStripeActivation(pos, normal, 0.8, 0.8, 0.283));

  // [-1, 1]
  vec2 noisePos;
  if (abs(normal.z) > 0.5) {
    noisePos = pos.xy;
  } else {
    noisePos = pos.xz;
  }
  float noise = fbm(noisePos * 10.);
  // [0, 1]
  noise = pow((noise + 1.) * 0.5, 9.5);

  // Mix factor: how much of the un-darkened color bleeds in (lower = darker overall).
  // outColor = mix(outColor * noise, outColor, 0.1);
  outColor = mix(outColor * noise, outColor, 0.3); // original

  return vec4(outColor, 1.);
}
