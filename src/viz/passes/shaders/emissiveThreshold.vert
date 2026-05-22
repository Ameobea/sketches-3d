varying vec2 vUv;

#if defined(HAS_FOG) && !defined(FOG_DISABLED)
uniform mat4 projectionMatrixInverse;
uniform mat4 cameraWorldMatrix;
varying vec3 vWorldRay;
#endif

void main() {
  vec2 ndcXY = position.xy;
  vUv = ndcXY * 0.5 + 0.5;

#if defined(HAS_FOG) && !defined(FOG_DISABLED)
  vec3 viewRay = vec3(
    projectionMatrixInverse[0][0] * ndcXY.x + projectionMatrixInverse[3][0],
    projectionMatrixInverse[1][1] * ndcXY.y + projectionMatrixInverse[3][1],
    projectionMatrixInverse[3][2]
  );
  vWorldRay = mat3(cameraWorldMatrix) * viewRay;
#endif

  gl_Position = vec4(ndcXY, 1.0, 1.0);
}
