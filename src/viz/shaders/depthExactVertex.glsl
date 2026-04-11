// Computes bit-exact depth values for the vertex shader.  This matches the behavior of other
// Three.JS shaders exactly and prevents artifacts when interacting with the depth pre-pass.

vec4 localPos = vec4(position, 1.0);
vec3 localNormal = normal;
#ifdef USE_INSTANCING
  localPos = instanceMatrix * localPos;
  localNormal = (instanceMatrix * vec4(localNormal, 0.0)).xyz;
#endif
vec4 mvPos = modelViewMatrix * localPos;
gl_Position = projectionMatrix * mvPos;
