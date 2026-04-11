// Vertex shader for CheckpointMaterial.
//
// The depth-exact clip-space transform lives in the shared snippet
// `src/viz/shaders/depthExactVertex.glsl`, which must be kept in lockstep with
// `buildOcclusionDepthMaterial` in customShader.ts — see that file for the
// precision/bit-exactness rationale. Do not inline or reorder the transform
// here; edit the shared snippet instead.

out vec3 vWorldPos;
out vec3 vWorldNormal;

void main() {
  __DEPTH_EXACT_VERTEX_BODY__

  // World-space outputs, derived from the same `localPos` / `localNormal` that
  // fed gl_Position. These feed the fragment noise sampling; their precision
  // behavior is independent of the depth pre-pass match.
  vWorldPos = (modelMatrix * localPos).xyz;
  // Inverse-transpose of the upper-3x3 of modelMatrix so non-uniform scale on
  // portal meshes is handled correctly. Cheap at vertex rate.
  mat3 worldNormalMat = transpose(inverse(mat3(modelMatrix)));
  vWorldNormal = normalize(worldNormalMat * localNormal);
}
