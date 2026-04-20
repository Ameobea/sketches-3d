out vec3 vWorldPos;

void main() {
  vec4 worldPosition = modelMatrix * vec4(position, 1.0);
  vWorldPos = worldPosition.xyz;

  gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
  // Push to the far plane so the sky never occludes real geometry.
  gl_Position.z = gl_Position.w;
}
