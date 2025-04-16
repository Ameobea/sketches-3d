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

vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec3 outColor = baseColor;
  outColor = mix(vec3(0.95, 0.65, 0.24) * outColor, outColor, getStripeActivation(pos, normal, 0.6, 0.4, 0.283));
  outColor = mix(vec3(0.75, 0.65, 0.74) * outColor, outColor, getStripeActivation(pos, normal, 0.2, 0.47, 0.283));
  outColor = mix(vec3(0.95, 0.85, 0.24) * outColor, outColor, getStripeActivation(pos, normal, 0.8, 0.8, 0.283));

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

  outColor = mix(outColor * noise, outColor, 0.3);

  return vec4(outColor, 1.);
}
