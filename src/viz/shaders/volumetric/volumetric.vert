precision highp float;

uniform mat4 cameraProjectionMatrixInv;
uniform mat4 cameraMatrixWorld;

varying vec2 vUv;
varying vec3 vWorldRay;

void main() {
  vec2 ndcXY = position.xy;
  vUv = ndcXY * 0.5 + 0.5;

  vec3 viewRay = vec3(
    cameraProjectionMatrixInv[0][0] * ndcXY.x + cameraProjectionMatrixInv[3][0],
    cameraProjectionMatrixInv[1][1] * ndcXY.y + cameraProjectionMatrixInv[3][1],
    cameraProjectionMatrixInv[3][2]
  );
  vWorldRay = mat3(cameraMatrixWorld) * viewRay;

  gl_Position = vec4(ndcXY, 1., 1.);
}
