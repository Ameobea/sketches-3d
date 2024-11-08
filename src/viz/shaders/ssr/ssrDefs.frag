void writeSSRData(vec3 normal) {
  outReflectionData = vec4(normalize(normal), SSR_ALPHA);
}
