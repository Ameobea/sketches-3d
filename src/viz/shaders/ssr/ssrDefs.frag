void writeSSRData(vec3 normal) {
  outReflectionData = vec4(normal, SSR_ALPHA);
}
