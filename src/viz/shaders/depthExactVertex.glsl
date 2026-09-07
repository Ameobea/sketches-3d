// Computes bit-exact depth values for the vertex shader.  This matches the behavior of other
// Three.JS shaders exactly and prevents artifacts when interacting with the depth pre-pass.

vec3 transformed = position;
vec3 objectNormal = normal;
#include <skinbase_vertex>
#include <skinnormal_vertex>
#include <skinning_vertex>
vec4 localPos = vec4(transformed, 1.0);
vec3 localNormal = objectNormal;
#ifdef USE_INSTANCING
  localPos = instanceMatrix * localPos;
  localNormal = (instanceMatrix * vec4(localNormal, 0.0)).xyz;
#endif
vec4 mvPos = modelViewMatrix * localPos;
gl_Position = projectionMatrix * mvPos;
