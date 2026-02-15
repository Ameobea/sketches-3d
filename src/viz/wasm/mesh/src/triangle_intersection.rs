use crate::linked_mesh::Vec3;
use nalgebra::Vector3;

type Vec3d = Vector3<f64>;

fn to_f64(v: Vec3) -> Vec3d {
  Vec3d::new(v.x as f64, v.y as f64, v.z as f64)
}

fn to_f32(v: Vec3d) -> Vec3 {
  Vec3::new(v.x as f32, v.y as f32, v.z as f32)
}

/// The type of triangle-triangle intersection found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriTriIntersectionType {
  /// Triangles intersect along a line segment
  Segment,
  /// Triangles are coplanar and overlap
  Coplanar,
}

/// Result of a triangle-triangle intersection test.
#[derive(Debug, Clone)]
pub struct TriTriIntersectionResult {
  /// A representative point on the intersection
  pub point: Vec3,
  /// The type of intersection
  pub intersection_type: TriTriIntersectionType,
}

/// Tests if two triangles intersect using the Guigue-Devillers algorithm.
///
/// All orientation predicates are computed in f64 for robustness with f32 inputs.
///
/// Degenerate intersections where triangles merely touch at a single point or along an edge
/// (without their interiors overlapping) are not reported.  This means shared vertices,
/// coincident vertices, and edge-to-vertex contact are all correctly filtered out.
pub fn tri_tri_intersection(
  v0: Vec3,
  v1: Vec3,
  v2: Vec3,
  u0: Vec3,
  u1: Vec3,
  u2: Vec3,
) -> Option<TriTriIntersectionResult> {
  let v0d = to_f64(v0);
  let v1d = to_f64(v1);
  let v2d = to_f64(v2);
  let u0d = to_f64(u0);
  let u1d = to_f64(u1);
  let u2d = to_f64(u2);

  // Compute plane of triangle 2: N2 . X = d2
  let e2_0 = u1d - u0d;
  let e2_1 = u2d - u0d;
  let n2 = e2_0.cross(&e2_1);

  // Signed distances of v0, v1, v2 to plane of triangle 2
  let dv0 = n2.dot(&(v0d - u0d));
  let dv1 = n2.dot(&(v1d - u0d));
  let dv2 = n2.dot(&(v2d - u0d));

  // Classify signs with a small epsilon to handle near-zero values
  let n2_len_sq = n2.norm_squared();
  let eps = n2_len_sq * 1e-14; // relative epsilon

  let sv0 = classify(dv0, eps);
  let sv1 = classify(dv1, eps);
  let sv2 = classify(dv2, eps);

  // If all vertices of triangle 1 are on the same side of plane 2, no intersection
  if sv0 == sv1 && sv1 == sv2 && sv0 != 0 {
    return None;
  }

  // Compute plane of triangle 1: N1 . X = d1
  let e1_0 = v1d - v0d;
  let e1_1 = v2d - v0d;
  let n1 = e1_0.cross(&e1_1);

  // Signed distances of u0, u1, u2 to plane of triangle 1
  let du0 = n1.dot(&(u0d - v0d));
  let du1 = n1.dot(&(u1d - v0d));
  let du2 = n1.dot(&(u2d - v0d));

  let n1_len_sq = n1.norm_squared();
  let eps1 = n1_len_sq * 1e-14;

  let su0 = classify(du0, eps1);
  let su1 = classify(du1, eps1);
  let su2 = classify(du2, eps1);

  // If all vertices of triangle 2 are on the same side of plane 1, no intersection
  if su0 == su1 && su1 == su2 && su0 != 0 {
    return None;
  }

  // Check for coplanar case: all vertices of both triangles lie on each other's planes
  if sv0 == 0 && sv1 == 0 && sv2 == 0 {
    return coplanar_tri_tri(v0d, v1d, v2d, u0d, u1d, u2d, n1);
  }

  // Non-coplanar intersection.
  //
  // Permute vertices of each triangle so that the "lone" vertex (the one on the opposite side
  // from the other two) is first.  After permutation, the first vertex has a sign opposite to
  // the other two (or one of the other two is zero).
  let (p1, q1, r1, dp1, dq1, dr1) = permute_vertices(v0d, v1d, v2d, sv0, sv1, sv2, dv0, dv1, dv2);
  let (p2, q2, r2, dp2, dq2, dr2) = permute_vertices(u0d, u1d, u2d, su0, su1, su2, du0, du1, du2);

  // The intersection line L = plane1 ∩ plane2.
  //
  // Each triangle clips L to a segment.  Triangle 1 clips L to [i1, j1] where:
  //   i1 = intersection of edge(p1,q1) with plane2
  //   j1 = intersection of edge(p1,r1) with plane2
  // Triangle 2 clips L to [i2, j2] similarly.
  //
  // Intersection exists iff these two segments on L overlap.  We can check this using
  // orientation predicates rather than computing the actual intersection line.

  // Compute the 4 edge-plane intersection points
  let i1 = edge_plane_intersection(p1, q1, dp1, dq1);
  let j1 = edge_plane_intersection(p1, r1, dp1, dr1);
  let i2 = edge_plane_intersection(p2, q2, dp2, dq2);
  let j2 = edge_plane_intersection(p2, r2, dp2, dr2);

  // Project the 4 points onto the intersection line and check overlap.
  // The intersection line direction is N1 × N2.
  let line_dir = n1.cross(&n2);

  // Project points onto the line direction to get 1D parameters
  let ti1 = line_dir.dot(&i1);
  let tj1 = line_dir.dot(&j1);
  let ti2 = line_dir.dot(&i2);
  let tj2 = line_dir.dot(&j2);

  // Ensure segments are ordered: [min, max]
  let (lo1, hi1, lo1_pt, hi1_pt) = if ti1 <= tj1 {
    (ti1, tj1, i1, j1)
  } else {
    (tj1, ti1, j1, i1)
  };
  let (lo2, hi2, lo2_pt, hi2_pt) = if ti2 <= tj2 {
    (ti2, tj2, i2, j2)
  } else {
    (tj2, ti2, j2, i2)
  };

  // Check overlap of [lo1, hi1] and [lo2, hi2]
  let line_dir_len_sq = line_dir.norm_squared();
  let overlap_eps = line_dir_len_sq * 1e-14;

  if lo1 > hi2 + overlap_eps || lo2 > hi1 + overlap_eps {
    return None;
  }

  // Compute the overlap segment
  let (overlap_lo, overlap_lo_pt) = if lo1 > lo2 {
    (lo1, lo1_pt)
  } else {
    (lo2, lo2_pt)
  };
  let (overlap_hi, overlap_hi_pt) = if hi1 < hi2 {
    (hi1, hi1_pt)
  } else {
    (hi2, hi2_pt)
  };

  // Filter degenerate zero-length intersections.  These occur when triangles merely touch
  // at a single point (shared vertex, coincident vertex, or edge-to-vertex contact) without
  // actually penetrating through each other.
  let seg_len = (overlap_hi - overlap_lo).abs();
  if seg_len < overlap_eps.sqrt().max(1e-10) {
    return None;
  }

  let midpoint = (overlap_lo_pt + overlap_hi_pt) * 0.5;
  Some(TriTriIntersectionResult {
    point: to_f32(midpoint),
    intersection_type: TriTriIntersectionType::Segment,
  })
}

/// Classify a signed distance as -1, 0, or +1 with epsilon tolerance.
fn classify(d: f64, eps: f64) -> i32 {
  if d > eps {
    1
  } else if d < -eps {
    -1
  } else {
    0
  }
}

/// Permute triangle vertices so the "lone" vertex (opposite sign from the others) is first.
///
/// Returns (p, q, r, dp, dq, dr) where p is the lone vertex.
/// The permutation preserves winding order when possible.
fn permute_vertices(
  v0: Vec3d,
  v1: Vec3d,
  v2: Vec3d,
  s0: i32,
  s1: i32,
  s2: i32,
  d0: f64,
  d1: f64,
  d2: f64,
) -> (Vec3d, Vec3d, Vec3d, f64, f64, f64) {
  // We need to find the vertex that is alone on one side.
  // If signs are (+, -, -) or (+, 0, -) etc., the + vertex is alone.
  // We want the lone vertex first, and the other two maintaining relative order.

  if s0 > 0 {
    if s1 > 0 {
      // s0 > 0, s1 > 0, s2 must be <= 0 (the lone one on the other side)
      (v2, v0, v1, d2, d0, d1)
    } else if s2 > 0 {
      // s0 > 0, s2 > 0, s1 is the lone one
      (v1, v2, v0, d1, d2, d0)
    } else {
      // s0 is the lone positive
      (v0, v1, v2, d0, d1, d2)
    }
  } else if s0 < 0 {
    if s1 < 0 {
      (v2, v0, v1, d2, d0, d1)
    } else if s2 < 0 {
      (v1, v2, v0, d1, d2, d0)
    } else {
      (v0, v1, v2, d0, d1, d2)
    }
  } else {
    // s0 == 0
    if s1 > 0 {
      if s2 > 0 {
        // Both s1 and s2 positive, s0 is on the plane → s0 is "alone"
        (v0, v1, v2, d0, d1, d2)
      } else {
        // s1 > 0, s2 <= 0: s1 is the lone positive
        (v1, v2, v0, d1, d2, d0)
      }
    } else if s1 < 0 {
      if s2 < 0 {
        (v0, v1, v2, d0, d1, d2)
      } else {
        // s1 < 0, s2 >= 0: s1 is the lone negative
        (v1, v2, v0, d1, d2, d0)
      }
    } else {
      // s0 == 0 && s1 == 0 → s2 is the lone one (or all zero, but that was handled earlier)
      (v2, v0, v1, d2, d0, d1)
    }
  }
}

/// Compute the intersection point of edge (a, b) with a plane, given signed distances da, db.
///
/// The intersection point is: a + t * (b - a) where t = da / (da - db)
fn edge_plane_intersection(a: Vec3d, b: Vec3d, da: f64, db: f64) -> Vec3d {
  let denom = da - db;
  if denom.abs() < 1e-30 {
    // Edge is nearly parallel to the plane; return midpoint as fallback
    (a + b) * 0.5
  } else {
    let t = da / denom;
    a + t * (b - a)
  }
}

/// Handle the coplanar case: both triangles lie in the same plane.
///
/// Projects to 2D and tests for overlap using edge-crossing tests.
fn coplanar_tri_tri(
  v0: Vec3d,
  v1: Vec3d,
  v2: Vec3d,
  u0: Vec3d,
  u1: Vec3d,
  u2: Vec3d,
  normal: Vec3d,
) -> Option<TriTriIntersectionResult> {
  // Project to 2D by dropping the axis with the largest normal component
  let abs_n = Vec3d::new(normal.x.abs(), normal.y.abs(), normal.z.abs());
  let (ax0, ax1) = if abs_n.x >= abs_n.y && abs_n.x >= abs_n.z {
    (1, 2) // drop X
  } else if abs_n.y >= abs_n.z {
    (0, 2) // drop Y
  } else {
    (0, 1) // drop Z
  };

  let proj = |v: Vec3d| -> (f64, f64) { (v[ax0], v[ax1]) };

  let pv0 = proj(v0);
  let pv1 = proj(v1);
  let pv2 = proj(v2);
  let pu0 = proj(u0);
  let pu1 = proj(u1);
  let pu2 = proj(u2);

  let tri1_edges = [(pv0, pv1), (pv1, pv2), (pv2, pv0)];
  let tri2_edges = [(pu0, pu1), (pu1, pu2), (pu2, pu0)];

  // Check if any edges strictly cross (endpoint-touching is excluded)
  for &(a, b) in &tri1_edges {
    for &(c, d) in &tri2_edges {
      if let Some(pt) = segment_segment_intersection_2d(a, b, c, d) {
        let pt3d = reconstruct_3d_from_2d(pt, pv0, pv1, pv2, v0, v1, v2, ax0, ax1);
        return Some(TriTriIntersectionResult {
          point: to_f32(pt3d),
          intersection_type: TriTriIntersectionType::Coplanar,
        });
      }
    }
  }

  // Check if one triangle is entirely inside the other.
  // point_in_triangle_2d uses a strictly-interior test, so shared/coincident vertices on
  // the boundary won't trigger a false positive.  We test all vertices of each triangle to
  // handle cases where some vertices are on the boundary and others are in the interior.
  for &pu in &[pu0, pu1, pu2] {
    if point_in_triangle_2d(pu, pv0, pv1, pv2) {
      let pt3d = reconstruct_3d_from_2d(pu, pv0, pv1, pv2, v0, v1, v2, ax0, ax1);
      return Some(TriTriIntersectionResult {
        point: to_f32(pt3d),
        intersection_type: TriTriIntersectionType::Coplanar,
      });
    }
  }
  for &pv in &[pv0, pv1, pv2] {
    if point_in_triangle_2d(pv, pu0, pu1, pu2) {
      let pt3d = reconstruct_3d_from_2d(pv, pu0, pu1, pu2, u0, u1, u2, ax0, ax1);
      return Some(TriTriIntersectionResult {
        point: to_f32(pt3d),
        intersection_type: TriTriIntersectionType::Coplanar,
      });
    }
  }

  // Handle boundary-only overlap: coplanar triangles that overlap in area but have
  // no strict crossings and no strictly interior vertices (e.g., duplicate faces).
  for &(a, b) in &tri1_edges {
    for &(c, d) in &tri2_edges {
      if let Some(pt) = colinear_overlap_point_2d(a, b, c, d) {
        let pt3d = reconstruct_3d_from_2d(pt, pv0, pv1, pv2, v0, v1, v2, ax0, ax1);
        return Some(TriTriIntersectionResult {
          point: to_f32(pt3d),
          intersection_type: TriTriIntersectionType::Coplanar,
        });
      }
    }
  }

  None
}

/// Reconstruct a 3D point from a 2D projected point using barycentric interpolation
/// within a reference triangle.
fn reconstruct_3d_from_2d(
  pt: (f64, f64),
  pv0: (f64, f64),
  pv1: (f64, f64),
  pv2: (f64, f64),
  v0: Vec3d,
  v1: Vec3d,
  v2: Vec3d,
  ax0: usize,
  ax1: usize,
) -> Vec3d {
  let (u, v, w) = barycentric_2d(pt, pv0, pv1, pv2);
  let drop_ax = 3 - ax0 - ax1;
  let mut pt3d = Vec3d::zeros();
  pt3d[ax0] = pt.0;
  pt3d[ax1] = pt.1;
  pt3d[drop_ax] = u * v0[drop_ax] + v * v1[drop_ax] + w * v2[drop_ax];
  pt3d
}

/// Compute barycentric coordinates of point p in triangle (a, b, c) in 2D.
fn barycentric_2d(
  p: (f64, f64),
  a: (f64, f64),
  b: (f64, f64),
  c: (f64, f64),
) -> (f64, f64, f64) {
  let v0 = (b.0 - a.0, b.1 - a.1);
  let v1 = (c.0 - a.0, c.1 - a.1);
  let v2 = (p.0 - a.0, p.1 - a.1);

  let d00 = v0.0 * v0.0 + v0.1 * v0.1;
  let d01 = v0.0 * v1.0 + v0.1 * v1.1;
  let d11 = v1.0 * v1.0 + v1.1 * v1.1;
  let d20 = v2.0 * v0.0 + v2.1 * v0.1;
  let d21 = v2.0 * v1.0 + v2.1 * v1.1;

  let denom = d00 * d11 - d01 * d01;
  if denom.abs() < 1e-30 {
    return (1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0); // degenerate triangle
  }

  let v = (d11 * d20 - d01 * d21) / denom;
  let w = (d00 * d21 - d01 * d20) / denom;
  let u = 1.0 - v - w;
  (u, v, w)
}

/// 2D segment-segment intersection test.
///
/// Returns the intersection point if segments (a,b) and (c,d) properly cross.
fn segment_segment_intersection_2d(
  a: (f64, f64),
  b: (f64, f64),
  c: (f64, f64),
  d: (f64, f64),
) -> Option<(f64, f64)> {
  let dx_ab = b.0 - a.0;
  let dy_ab = b.1 - a.1;
  let dx_cd = d.0 - c.0;
  let dy_cd = d.1 - c.1;

  let denom = dx_ab * dy_cd - dy_ab * dx_cd;
  if denom.abs() < 1e-20 {
    return None; // parallel or degenerate
  }

  let dx_ac = c.0 - a.0;
  let dy_ac = c.1 - a.1;

  let t = (dx_ac * dy_cd - dy_ac * dx_cd) / denom;
  let u = (dx_ac * dy_ab - dy_ac * dx_ab) / denom;

  // Require strictly interior crossings: both t and u must be bounded away from 0 and 1.
  // This ensures that endpoint-touching (shared/coincident vertices, edge-to-vertex contact)
  // is not reported as an intersection.
  let eps = 1e-8;
  if t > eps && t < 1.0 - eps && u > eps && u < 1.0 - eps {
    Some((a.0 + t * dx_ab, a.1 + t * dy_ab))
  } else {
    None
  }
}

/// Test if a point is inside a triangle in 2D using cross product signs.
fn point_in_triangle_2d(
  p: (f64, f64),
  a: (f64, f64),
  b: (f64, f64),
  c: (f64, f64),
) -> bool {
  let cross =
    |o: (f64, f64), p1: (f64, f64), p2: (f64, f64)| (p1.0 - o.0) * (p2.1 - o.1) - (p1.1 - o.1) * (p2.0 - o.0);

  let d1 = cross(p, a, b);
  let d2 = cross(p, b, c);
  let d3 = cross(p, c, a);

  // Strictly interior: all cross products must have the same sign and be bounded away from
  // zero.  Points on the boundary (edges, vertices) will have at least one cross product
  // near zero and will be rejected.
  let eps = 1e-10;
  (d1 > eps && d2 > eps && d3 > eps) || (d1 < -eps && d2 < -eps && d3 < -eps)
}

/// Return a point on the overlapping portion of two colinear segments, if they overlap
/// with positive length (not just a single point).
fn colinear_overlap_point_2d(
  a: (f64, f64),
  b: (f64, f64),
  c: (f64, f64),
  d: (f64, f64),
) -> Option<(f64, f64)> {
  let ab = (b.0 - a.0, b.1 - a.1);
  let cross = |u: (f64, f64), v: (f64, f64)| u.0 * v.1 - u.1 * v.0;
  let eps = 1e-12;

  // Check colinearity of c and d with segment ab.
  if cross(ab, (c.0 - a.0, c.1 - a.1)).abs() > eps
    || cross(ab, (d.0 - a.0, d.1 - a.1)).abs() > eps
  {
    return None;
  }

  let use_x = ab.0.abs() >= ab.1.abs();
  let (a1, b1, c1, d1) = if use_x {
    (a.0, b.0, c.0, d.0)
  } else {
    (a.1, b.1, c.1, d.1)
  };

  let (min1, max1) = if a1 < b1 { (a1, b1) } else { (b1, a1) };
  let (min2, max2) = if c1 < d1 { (c1, d1) } else { (d1, c1) };

  let lo = min1.max(min2);
  let hi = max1.min(max2);
  if hi - lo <= eps {
    return None; // overlap is a point or empty
  }

  let mid = 0.5 * (lo + hi);
  let denom = if use_x { ab.0 } else { ab.1 };
  if denom.abs() < 1e-20 {
    return None;
  }
  let t = (mid - if use_x { a.0 } else { a.1 }) / denom;
  Some((a.0 + t * ab.0, a.1 + t * ab.1))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_non_intersecting_triangles() {
    let v0 = Vec3::new(0.0, 0.0, 0.0);
    let v1 = Vec3::new(1.0, 0.0, 0.0);
    let v2 = Vec3::new(0.0, 1.0, 0.0);

    let u0 = Vec3::new(0.0, 0.0, 1.0);
    let u1 = Vec3::new(1.0, 0.0, 1.0);
    let u2 = Vec3::new(0.0, 1.0, 1.0);

    assert!(tri_tri_intersection(v0, v1, v2, u0, u1, u2).is_none());
  }

  #[test]
  fn test_intersecting_triangles() {
    // Two triangles that cross each other
    let v0 = Vec3::new(-1.0, 0.0, 0.0);
    let v1 = Vec3::new(1.0, 0.0, 0.0);
    let v2 = Vec3::new(0.0, 1.0, 0.0);

    let u0 = Vec3::new(0.0, 0.5, -1.0);
    let u1 = Vec3::new(0.0, 0.5, 1.0);
    let u2 = Vec3::new(0.0, -0.5, 0.0);

    let result = tri_tri_intersection(v0, v1, v2, u0, u1, u2);
    assert!(result.is_some());
    let result = result.unwrap();
    assert_eq!(result.intersection_type, TriTriIntersectionType::Segment);
  }

  #[test]
  fn test_shared_vertex_degenerate() {
    // Two triangles sharing a vertex, touching only at that vertex
    let v0 = Vec3::new(0.0, 0.0, 0.0);
    let v1 = Vec3::new(1.0, 0.0, 0.0);
    let v2 = Vec3::new(0.0, 1.0, 0.0);

    let u0 = Vec3::new(0.0, 0.0, 0.0); // shared vertex
    let u1 = Vec3::new(-1.0, 0.0, 0.0);
    let u2 = Vec3::new(0.0, 0.0, 1.0);

    // Should filter out the degenerate point-only intersection at the shared vertex
    assert!(tri_tri_intersection(v0, v1, v2, u0, u1, u2).is_none());
  }

  #[test]
  fn test_coplanar_overlapping() {
    let v0 = Vec3::new(0.0, 0.0, 0.0);
    let v1 = Vec3::new(2.0, 0.0, 0.0);
    let v2 = Vec3::new(1.0, 2.0, 0.0);

    let u0 = Vec3::new(1.0, 0.0, 0.0);
    let u1 = Vec3::new(3.0, 0.0, 0.0);
    let u2 = Vec3::new(2.0, 2.0, 0.0);

    let result = tri_tri_intersection(v0, v1, v2, u0, u1, u2);
    assert!(result.is_some());
    let result = result.unwrap();
    assert_eq!(result.intersection_type, TriTriIntersectionType::Coplanar);
  }

  #[test]
  fn test_coplanar_non_overlapping() {
    let v0 = Vec3::new(0.0, 0.0, 0.0);
    let v1 = Vec3::new(1.0, 0.0, 0.0);
    let v2 = Vec3::new(0.0, 1.0, 0.0);

    let u0 = Vec3::new(2.0, 0.0, 0.0);
    let u1 = Vec3::new(3.0, 0.0, 0.0);
    let u2 = Vec3::new(2.0, 1.0, 0.0);

    assert!(tri_tri_intersection(v0, v1, v2, u0, u1, u2).is_none());
  }

  #[test]
  fn test_nearly_parallel_no_intersection() {
    // Two triangles in nearly parallel planes that don't intersect
    let v0 = Vec3::new(0.0, 0.0, 0.0);
    let v1 = Vec3::new(1.0, 0.0, 0.0);
    let v2 = Vec3::new(0.5, 1.0, 0.0);

    let u0 = Vec3::new(0.0, 0.0, 0.1);
    let u1 = Vec3::new(1.0, 0.0, 0.1);
    let u2 = Vec3::new(0.5, 1.0, 0.1);

    let result = tri_tri_intersection(v0, v1, v2, u0, u1, u2);
    assert!(result.is_none());
  }

  #[test]
  fn test_coplanar_shared_vertex_bowtie() {
    // Two coplanar triangles touching at exactly one shared vertex (bowtie configuration).
    // This is the case from the user's bug report — should NOT be self-intersecting.
    let v0 = Vec3::new(0.0, 0.0, 0.0);
    let v1 = Vec3::new(1.0, 1.0, 0.0);
    let v2 = Vec3::new(1.0, 0.0, 0.0);

    // Shares v0 at the origin, extends in the opposite direction
    let u0 = Vec3::new(-1.0, -1.0, 0.0);
    let u1 = Vec3::new(0.0, 0.0, 0.0); // same position as v0
    let u2 = Vec3::new(-1.0, 0.0, 0.0);

    assert!(tri_tri_intersection(v0, v1, v2, u0, u1, u2).is_none());
  }

  #[test]
  fn test_coplanar_coincident_vertex_not_shared() {
    // Two coplanar triangles with coincident vertices (same position, different keys).
    // Touching at the coincident point only — should NOT be self-intersecting.
    let v0 = Vec3::new(0.0, 0.0, 0.0);
    let v1 = Vec3::new(1.0, 0.0, 0.0);
    let v2 = Vec3::new(0.5, 1.0, 0.0);

    // u0 is at the same position as v1, but they're not shared vertices in the mesh
    let u0 = Vec3::new(1.0, 0.0, 0.0);
    let u1 = Vec3::new(2.0, 0.0, 0.0);
    let u2 = Vec3::new(1.5, 1.0, 0.0);

    assert!(tri_tri_intersection(v0, v1, v2, u0, u1, u2).is_none());
  }

  #[test]
  fn test_coplanar_duplicate_triangles_overlap() {
    // Two coplanar triangles occupying the same area (duplicate faces).
    let v0 = Vec3::new(0.0, 0.0, 0.0);
    let v1 = Vec3::new(2.0, 0.0, 0.0);
    let v2 = Vec3::new(0.0, 2.0, 0.0);

    let u0 = Vec3::new(0.0, 0.0, 0.0);
    let u1 = Vec3::new(2.0, 0.0, 0.0);
    let u2 = Vec3::new(0.0, 2.0, 0.0);

    let result = tri_tri_intersection(v0, v1, v2, u0, u1, u2);
    assert!(result.is_some());
    let result = result.unwrap();
    assert_eq!(result.intersection_type, TriTriIntersectionType::Coplanar);
  }
}
