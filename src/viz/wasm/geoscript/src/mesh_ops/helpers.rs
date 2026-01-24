use mesh::linked_mesh::Vec3;

/// Epsilon for determining if ring/row vertices have collapsed to a single point
const COLLAPSE_EPSILON: f32 = 1e-5;

/// Checks whether all vertices in a ring/row have collapsed to approximately the same point
#[inline]
pub fn vertices_are_collapsed(vertices: &[Vec3]) -> bool {
  if vertices.is_empty() {
    return true;
  }
  let first = vertices[0];
  let epsilon_sq = COLLAPSE_EPSILON * COLLAPSE_EPSILON;
  vertices
    .iter()
    .all(|v| (*v - first).norm_squared() <= epsilon_sq)
}

/// Computes the centroid of a ring/row of vertices
#[inline]
pub fn compute_centroid(vertices: &[Vec3]) -> Vec3 {
  if vertices.is_empty() {
    return Vec3::new(0., 0., 0.);
  }
  vertices
    .iter()
    .fold(Vec3::new(0., 0., 0.), |acc, v| acc + *v)
    / (vertices.len() as f32)
}
