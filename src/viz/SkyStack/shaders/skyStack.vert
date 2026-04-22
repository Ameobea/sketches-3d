out vec2 vUv;

void main() {
  // Fullscreen quad — PlaneGeometry(2, 2) positions are already in clip space
  // on the near plane. Bypass the camera matrices to avoid depending on
  // whatever camera is passed to renderer.render().
  vUv = position.xy * 0.5 + 0.5;
  gl_Position = vec4(position.xy, 0.0, 1.0);
}
