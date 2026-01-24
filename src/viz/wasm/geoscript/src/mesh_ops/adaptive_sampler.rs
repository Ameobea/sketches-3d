//! Adaptive Sampler
//!
//! This module provides curvature-aware adaptive sampling for curves in `rail_sweep`.
//! Instead of distributing samples uniformly, it concentrates vertices in regions of high
//! curvature (where the curve deviates most from a straight line).
//!
//! Works with both 2D curves (Vec2, for profile sampling) and 3D curves (Vec3, for spine sampling).
//!
//! ## Algorithm
//!
//! Uses a priority-queue-based greedy approach:
//! 1. Initialize segments from critical points plus uniform seed points (for anti-aliasing)
//! 2. For each segment, compute error (perpendicular distance from curve midpoint to chord)
//! 3. Push segments to max-heap keyed by error
//! 4. Loop until vertex_count == target_count:
//!    - Pop segment with highest error
//!    - Split at midpoint, insert new vertex
//!    - Compute errors for two new sub-segments, push to heap
//! 5. Return sorted t-values
//!
//! ## Anti-Aliasing
//!
//! To prevent missing high-curvature regions that fall between critical points, the algorithm
//! seeds the initial segments with uniform samples. This ensures we're "looking" at enough
//! places across the domain to catch features that might otherwise be missed.
//!
//! ## Error Metric
//!
//! The error for a segment [t_start, t_end] is the perpendicular distance from the curve's
//! midpoint f((t_start + t_end) / 2) to the chord line between f(t_start) and f(t_end).

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::ops::Sub;

use mesh::linked_mesh::Vec3;

use crate::Vec2;

/// Minimum segment length to prevent infinite subdivision on degenerate geometry.
const DEFAULT_MIN_SEGMENT_LENGTH: f32 = 1e-5;

/// Minimum number of uniform seed points for anti-aliasing.
/// These ensure we sample the curve at regular intervals to catch high-curvature
/// regions that might fall between critical points.
const MIN_UNIFORM_SEEDS: usize = 8;

/// Trait for point types that can be used with adaptive sampling.
///
/// This abstracts over Vec2 and Vec3, allowing the same algorithm to work for both
/// 2D profile curves and 3D spine curves.
pub trait AdaptiveSamplePoint: Copy + Sub<Output = Self> {
  /// Returns the squared norm of the vector.
  fn norm_squared(&self) -> f32;

  /// Returns the norm (length) of the vector.
  fn norm(&self) -> f32 {
    self.norm_squared().sqrt()
  }

  /// Computes the perpendicular distance from this point to a line segment.
  ///
  /// The line segment is defined by `seg_start` and `seg_end`.
  fn distance_to_line(&self, seg_start: Self, seg_end: Self) -> f32;
}

impl AdaptiveSamplePoint for Vec2 {
  fn norm_squared(&self) -> f32 {
    self.norm_squared()
  }

  fn distance_to_line(&self, seg_start: Vec2, seg_end: Vec2) -> f32 {
    let line_vec = seg_end - seg_start;
    let line_len_sq = line_vec.norm_squared();

    // Degenerate case: segment is a point
    if line_len_sq < 1e-12 {
      return (*self - seg_start).norm();
    }

    let point_vec = *self - seg_start;

    // 2D cross product magnitude: |a.x * b.y - a.y * b.x|
    let cross = line_vec.x * point_vec.y - line_vec.y * point_vec.x;

    cross.abs() / line_len_sq.sqrt()
  }
}

impl AdaptiveSamplePoint for Vec3 {
  fn norm_squared(&self) -> f32 {
    self.norm_squared()
  }

  fn distance_to_line(&self, seg_start: Vec3, seg_end: Vec3) -> f32 {
    let line_vec = seg_end - seg_start;
    let line_len_sq = line_vec.norm_squared();

    // Degenerate case: segment is a point
    if line_len_sq < 1e-12 {
      return (*self - seg_start).norm();
    }

    let point_vec = *self - seg_start;

    // 3D cross product, then take norm
    let cross = line_vec.cross(&point_vec);

    cross.norm() / line_len_sq.sqrt()
  }
}

/// Segment candidate for the priority queue.
///
/// Stores the segment boundaries, precomputed midpoint, and error value.
/// Ordered by error (highest first) for max-heap behavior.
#[derive(Clone)]
struct SegmentCandidate {
  t_start: f32,
  t_end: f32,
  t_mid: f32,
  /// The error metric: perpendicular distance from midpoint to chord
  error: f32,
}

impl PartialEq for SegmentCandidate {
  fn eq(&self, other: &Self) -> bool {
    // For heap ordering, we only care about error equality
    (self.error - other.error).abs() < 1e-9
  }
}

impl Eq for SegmentCandidate {}

impl Ord for SegmentCandidate {
  fn cmp(&self, other: &Self) -> Ordering {
    // Max-heap: higher error comes first
    self
      .error
      .partial_cmp(&other.error)
      .unwrap_or(Ordering::Equal)
  }
}

impl PartialOrd for SegmentCandidate {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

/// Creates a segment candidate by evaluating the curve and computing error (fallible version).
fn create_segment_candidate<P, E>(
  t_start: f32,
  t_end: f32,
  sample_fn: &impl Fn(f32) -> Result<P, E>,
) -> Result<SegmentCandidate, E>
where
  P: AdaptiveSamplePoint,
{
  let t_mid = (t_start + t_end) * 0.5;

  let p_start = sample_fn(t_start)?;
  let p_end = sample_fn(t_end)?;
  let p_mid = sample_fn(t_mid)?;

  let error = p_mid.distance_to_line(p_start, p_end);

  Ok(SegmentCandidate {
    t_start,
    t_end,
    t_mid,
    error,
  })
}

/// Adaptively samples a curve using curvature-aware subdivision.
///
/// # Arguments
///
/// * `target_count` - The desired number of sample points
/// * `initial_ts` - Critical t-values to seed the algorithm (will be included in output). Should
///   include at least 0.0 and 1.0 as boundaries.
/// * `sample_fn` - Function that evaluates the curve at a given t in [0, 1]
/// * `min_segment_length` - Minimum segment length to prevent infinite subdivision
///
/// # Returns
///
/// A sorted vector of t-values in [0, 1) with exactly `target_count` elements.
/// The returned values do not include 1.0 (matching `build_topology_samples` behavior).
pub fn adaptive_sample<P, F>(
  target_count: usize,
  initial_ts: &[f32],
  sample_fn: F,
  min_segment_length: f32,
) -> Vec<f32>
where
  P: AdaptiveSamplePoint,
  F: Fn(f32) -> P,
{
  adaptive_sample_fallible::<P, std::convert::Infallible>(
    target_count,
    initial_ts,
    |t| Ok(sample_fn(t)),
    min_segment_length,
  )
  .expect("infallible sample_fn should not fail")
}

/// Adaptively samples a curve using curvature-aware subdivision (fallible version).
///
/// Like `adaptive_sample` but accepts a fallible sample function that can return errors.
/// This is useful when the sample function involves evaluating user-provided callbacks.
///
/// # Arguments
///
/// * `target_count` - The desired number of sample points
/// * `initial_ts` - Critical t-values to seed the algorithm (will be included in output). Should
///   include at least 0.0 and 1.0 as boundaries.
/// * `sample_fn` - Fallible function that evaluates the curve at a given t in [0, 1]
/// * `min_segment_length` - Minimum segment length to prevent infinite subdivision
///
/// # Returns
///
/// A sorted vector of t-values in [0, 1) with exactly `target_count` elements,
/// or an error if the sample function fails.
pub fn adaptive_sample_fallible<P, E>(
  target_count: usize,
  initial_ts: &[f32],
  sample_fn: impl Fn(f32) -> Result<P, E>,
  min_segment_length: f32,
) -> Result<Vec<f32>, E>
where
  P: AdaptiveSamplePoint,
{
  if target_count == 0 {
    return Ok(Vec::new());
  }

  let min_seg_len = if min_segment_length > 0.0 {
    min_segment_length
  } else {
    DEFAULT_MIN_SEGMENT_LENGTH
  };

  // Normalize and deduplicate initial t-values (critical points)
  let mut ts: Vec<f32> = initial_ts
    .iter()
    .copied()
    .filter(|t| t.is_finite())
    .map(|t| t.clamp(0., 1.))
    .collect();

  // Add uniform seed points for anti-aliasing.
  // This ensures we're sampling the curve at regular intervals to catch high-curvature
  // regions that might fall between critical points.
  let seed_count = MIN_UNIFORM_SEEDS.max(target_count / 8);
  for i in 0..=seed_count {
    ts.push(i as f32 / seed_count as f32);
  }

  ts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
  ts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);

  // If we already have enough points, select evenly distributed ones (excluding 1.0)
  let available: Vec<f32> = ts.iter().copied().filter(|t| *t < 1.0 - 1e-6).collect();
  if available.len() >= target_count {
    // Select evenly distributed indices to preserve coverage across the full range
    // Always include the first point (0.0) to maintain the starting boundary
    let mut result = Vec::with_capacity(target_count);
    for i in 0..target_count {
      // Map index i to a position in the available array
      // This distributes selections evenly across all available points
      let idx = if target_count == 1 {
        0
      } else {
        (i * (available.len() - 1)) / (target_count - 1)
      };
      result.push(available[idx]);
    }
    result.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    return Ok(result);
  }

  // Initialize the priority queue with segments between consecutive t-values
  let mut heap: BinaryHeap<SegmentCandidate> = BinaryHeap::new();

  for window in ts.windows(2) {
    let t_start = window[0];
    let t_end = window[1];

    // Only create candidates for segments that are large enough to split
    if t_end - t_start >= min_seg_len * 2.0 {
      let candidate = create_segment_candidate(t_start, t_end, &sample_fn)?;
      heap.push(candidate);
    }
  }

  // The "vertices" are the boundary points between segments.
  // We start with len(ts) vertices and need to add more by splitting.
  let mut vertex_count = ts.len();
  let mut split_points: Vec<f32> = Vec::new();

  // Greedily split segments until we reach target count
  // Note: target_count is for [0, 1) range, but we're counting all vertices including 1.0
  // So we need vertex_count - 1 samples in [0, 1)
  while vertex_count - 1 < target_count {
    let Some(segment) = heap.pop() else {
      // No more segments to split
      break;
    };

    // Add the midpoint as a new vertex
    split_points.push(segment.t_mid);
    vertex_count += 1;

    // Create two new sub-segments and add them to the heap if large enough
    let left_len = segment.t_mid - segment.t_start;
    let right_len = segment.t_end - segment.t_mid;

    if left_len >= min_seg_len * 2.0 {
      let left = create_segment_candidate(segment.t_start, segment.t_mid, &sample_fn)?;
      heap.push(left);
    }

    if right_len >= min_seg_len * 2.0 {
      let right = create_segment_candidate(segment.t_mid, segment.t_end, &sample_fn)?;
      heap.push(right);
    }
  }

  // Combine initial t-values with split points and sort
  let mut result: Vec<f32> = ts
    .into_iter()
    .chain(split_points)
    .filter(|t| *t < 1.0 - 1e-6) // Exclude 1.0
    .collect();

  result.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
  result.dedup_by(|a, b| (*a - *b).abs() < 1e-6);

  // Ensure we have exactly target_count samples
  // If we somehow have too many (shouldn't happen), truncate
  result.truncate(target_count);

  Ok(result)
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::f32::consts::PI;

  fn circle_profile(t: f32) -> Vec2 {
    let angle = t * 2.0 * PI;
    Vec2::new(angle.cos(), angle.sin())
  }

  fn superellipse_profile(exponent: f32) -> impl Fn(f32) -> Vec2 {
    move |t: f32| {
      let angle = t * 2.0 * PI;
      let cos_a = angle.cos();
      let sin_a = angle.sin();

      // Superellipse formula: |x|^n + |y|^n = 1
      // Parametric form with signed power
      let x = cos_a.abs().powf(2.0 / exponent) * cos_a.signum();
      let y = sin_a.abs().powf(2.0 / exponent) * sin_a.signum();

      Vec2::new(x, y)
    }
  }

  fn helix_3d(t: f32) -> Vec3 {
    let angle = t * 4.0 * PI; // Two full turns
    Vec3::new(angle.cos(), angle.sin(), t * 2.0)
  }

  #[test]
  fn test_adaptive_sample_basic() {
    let result = adaptive_sample(10, &[0.0, 1.0], circle_profile, 1e-5);

    assert_eq!(result.len(), 10);
    assert!(result[0] >= 0.0);
    assert!(result.last().unwrap() < &1.0);

    // Verify sorted
    for window in result.windows(2) {
      assert!(window[0] < window[1], "Results should be sorted");
    }
  }

  #[test]
  fn test_adaptive_sample_circle_roughly_uniform() {
    // For a circle (constant curvature), samples should be roughly uniform
    let result = adaptive_sample(8, &[0.0, 1.0], circle_profile, 1e-5);

    assert_eq!(result.len(), 8);

    // Check that gaps are reasonably uniform (within 2x of each other)
    let mut gaps: Vec<f32> = Vec::new();
    for i in 0..result.len() {
      let next = if i + 1 < result.len() {
        result[i + 1]
      } else {
        1.0
      };
      gaps.push(next - result[i]);
    }

    let min_gap = gaps.iter().copied().fold(f32::INFINITY, f32::min);
    let max_gap = gaps.iter().copied().fold(0.0, f32::max);

    // For a circle, gaps should be within 3x of each other
    assert!(
      max_gap < min_gap * 3.0,
      "Circle gaps should be roughly uniform: min={min_gap}, max={max_gap}"
    );
  }

  #[test]
  fn test_adaptive_sample_superellipse_concentrates_at_corners() {
    // High-exponent superellipse has sharp corners at t=0.125, 0.375, 0.625, 0.875
    // (corresponding to angles 45, 135, 225, 315 degrees)
    let profile = superellipse_profile(8.0);
    let result = adaptive_sample(20, &[0.0, 1.0], profile, 1e-5);

    assert_eq!(result.len(), 20);

    // Verify that the algorithm produces non-uniform sampling
    // by checking that the gaps between samples vary significantly.
    // For a high-exponent superellipse, gaps near corners should be smaller
    // than gaps along the flat sides.
    let mut gaps: Vec<f32> = Vec::new();
    for i in 0..result.len() {
      let next = if i + 1 < result.len() {
        result[i + 1]
      } else {
        1.0
      };
      gaps.push(next - result[i]);
    }

    let min_gap = gaps.iter().copied().fold(f32::INFINITY, f32::min);
    let max_gap = gaps.iter().copied().fold(0.0, f32::max);

    // For a superellipse, the max gap should be significantly larger than min gap
    // because the algorithm concentrates samples in high-curvature regions (corners)
    // and spreads them out in low-curvature regions (flat sides)
    assert!(
      max_gap > min_gap * 1.5,
      "Expected non-uniform gap distribution for superellipse: min={min_gap}, max={max_gap}"
    );
  }

  #[test]
  fn test_adaptive_sample_respects_critical_points() {
    // Critical points should always be included in output
    let critical_points = vec![0.0, 0.25, 0.5, 0.75, 1.0];
    let result = adaptive_sample(10, &critical_points, circle_profile, 1e-5);

    assert_eq!(result.len(), 10);

    // All critical points except 1.0 should be in the result
    for &cp in &[0.0, 0.25, 0.5, 0.75] {
      assert!(
        result.iter().any(|&t| (t - cp).abs() < 1e-5),
        "Critical point {cp} should be in result"
      );
    }
  }

  #[test]
  fn test_adaptive_sample_min_segment_length() {
    // With a large min_segment_length, subdivision should stop early
    let result = adaptive_sample(100, &[0.0, 1.0], circle_profile, 0.2);

    // With min_segment_length of 0.2, we can have at most ~5 segments (1.0 / 0.2)
    // So we can't get 100 samples
    assert!(
      result.len() < 100,
      "Min segment length should limit subdivision"
    );
  }

  #[test]
  fn test_adaptive_sample_target_count() {
    for target in [3, 5, 10, 20, 50] {
      let result = adaptive_sample(target, &[0.0, 1.0], circle_profile, 1e-5);
      assert_eq!(
        result.len(),
        target,
        "Should return exactly target_count samples"
      );
    }
  }

  #[test]
  fn test_adaptive_sample_empty() {
    let result = adaptive_sample(0, &[0.0, 1.0], circle_profile, 1e-5);
    assert!(result.is_empty());
  }

  #[test]
  fn test_adaptive_sample_single() {
    let result = adaptive_sample(1, &[0.0, 1.0], circle_profile, 1e-5);
    assert_eq!(result.len(), 1);
    assert!((result[0] - 0.0).abs() < 1e-5);
  }

  #[test]
  fn test_adaptive_sample_3d_helix() {
    // Test that 3D sampling works with a helix
    let result = adaptive_sample(15, &[0.0, 1.0], helix_3d, 1e-5);

    assert_eq!(result.len(), 15);
    assert!(result[0] >= 0.0);
    assert!(result.last().unwrap() < &1.0);

    // Verify sorted
    for window in result.windows(2) {
      assert!(window[0] < window[1], "Results should be sorted");
    }
  }

  #[test]
  fn test_adaptive_sample_3d_with_sharp_turn() {
    // A path with a sharp turn should concentrate samples at the turn
    fn sharp_turn(t: f32) -> Vec3 {
      if t < 0.5 {
        Vec3::new(t * 2.0, 0.0, 0.0)
      } else {
        Vec3::new(1.0, (t - 0.5) * 2.0, 0.0)
      }
    }

    let result = adaptive_sample(10, &[0.0, 1.0], sharp_turn, 1e-5);

    assert_eq!(result.len(), 10);

    // Should have a sample close to t=0.5 (the turn point)
    let has_sample_near_turn = result.iter().any(|&t| (t - 0.5).abs() < 0.1);
    assert!(
      has_sample_near_turn,
      "Should have sample near turn at t=0.5"
    );
  }
}
