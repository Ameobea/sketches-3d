uniform mat4 projectionMatrixInverse;
uniform mat4 cameraWorldMatrix;

varying vec2 vUv;
varying vec3 vWorldRay;

void main() {
  vUv = uv;
  vec2 ndcXY = uv * 2.0 - 1.0;

  vec3 viewRay = vec3(
    projectionMatrixInverse[0][0] * ndcXY.x + projectionMatrixInverse[3][0],
    projectionMatrixInverse[1][1] * ndcXY.y + projectionMatrixInverse[3][1],
    projectionMatrixInverse[3][2]
  );
  vWorldRay = mat3(cameraWorldMatrix) * viewRay;

  gl_Position = vec4(ndcXY, 0.0, 1.0);
}
