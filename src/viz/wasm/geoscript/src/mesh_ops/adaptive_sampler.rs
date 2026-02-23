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
//! Uses a span-based curvature integration approach:
//!
//! ### Span Formation
//!
//! Critical t-values from `initial_ts` act as hard structural boundaries, splitting the domain
//! `[0, 1]` into independent spans. Curvature is never computed across a span boundary, so a
//! sharp corner at a critical point does not bleed density into adjacent spans.
//!
//! ### Per-Span Analysis
//!
//! Each span is independently oversampled (≥64 points, ≥15x target density) to build a density
//! field: `arc_length + CURVATURE_WEIGHT * avg_chord_deviation`. Deviations at span endpoints are
//! forced to 0.0 to enforce the boundary isolation.
//!
//! ### Budget Allocation and Placement
//!
//! The free budget (`target_count - mandatory_count`) is distributed across spans proportional to
//! their metric mass using the largest-remainder method. Within each span, `k` interior points are
//! placed at cumulative-mass quantiles `j/(k+1)`, guaranteeing no point can be placed adjacent to
//! a span endpoint unless the budget is extremely large.

use std::cmp::Ordering;
use std::ops::Sub;

use mesh::linked_mesh::Vec3;

use crate::Vec2;

/// Minimum segment length to prevent infinite subdivision on degenerate geometry.
const DEFAULT_MIN_SEGMENT_LENGTH: f32 = 1e-5;

/// Oversampling factor for Phase 1. Dense enough to catch features missed by a single midpoint
/// check, while remaining practical for WASM execution in a browser.
const OVERSAMPLE_FACTOR: usize = 25;

/// Hard minimum for the number of dense samples regardless of target_count.
const MIN_DENSE_SAMPLES: usize = 64;

/// Multiplier for the curvature contribution to the density field.
/// Higher values concentrate more samples at high-curvature regions at the cost of uniformity
/// in low-curvature regions.
const CURVATURE_WEIGHT: f32 = 70.0;

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

/// Adaptively samples a curve using curvature-aware density integration.
///
/// # Arguments
///
/// * `target_count` - The desired number of sample points
/// * `initial_ts` - Critical t-values to seed the algorithm (will be included in output). Should
///   include at least 0.0 and 1.0 as boundaries.
/// * `sample_fn` - Function that evaluates the curve at a given t in [0, 1]
/// * `min_segment_length` - Minimum segment length to prevent sliver triangles
///
/// # Returns
///
/// A sorted vector of t-values in [0, 1) with up to `target_count` elements.
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

/// Computed metrics for a single span `[t_start, t_end]`.
///
/// Stores a dense curvature analysis of the span, used to proportionally allocate budget
/// and place samples via center-of-mass integration.
struct SpanData {
  t_start: f32,
  t_end: f32,
  /// Total metric mass (sum of all densities in this span).
  mass: f32,
  /// Uniformly-spaced t-values including both endpoints; len = n_dense + 1.
  dense_ts: Vec<f32>,
  /// Per-sub-segment density values; len = n_dense.
  densities: Vec<f32>,
  /// Cumulative prefix sum of densities; len = n_dense + 1, starts at 0.0.
  cumulative: Vec<f32>,
}

impl SpanData {
  /// Places `k` t-values strictly inside `(t_start, t_end)` by center-of-mass placement.
  ///
  /// The j-th sample targets cumulative mass `j / (k+1) * total_mass`, which guarantees
  /// samples cannot be adjacent to span endpoints (making slivers structurally impossible).
  /// Candidates too close to the previous sample or to `t_end` are filtered out.
  fn sample_internal(&self, k: usize, min_seg_len: f32) -> Vec<f32> {
    let mut result = Vec::with_capacity(k);
    if k == 0 || self.mass <= 0.0 {
      return result;
    }
    let mut last_t = self.t_start;
    for j in 1..=k {
      let target = (j as f32 / (k + 1) as f32) * self.mass;
      // Find the first cumulative index where value >= target.
      let ix = self.cumulative.partition_point(|&c| c < target);
      let ix = ix.max(1).min(self.cumulative.len() - 1);
      let t = {
        let seg_density = self.densities[ix - 1];
        if seg_density < 1e-10 {
          self.dense_ts[ix - 1]
        } else {
          let c0 = self.cumulative[ix - 1];
          let t0 = self.dense_ts[ix - 1];
          let t1 = self.dense_ts[ix];
          t0 + (target - c0) / seg_density * (t1 - t0)
        }
      };
      // Filter: must be far enough from the previous sample and from span end.
      if t - last_t >= min_seg_len && self.t_end - t >= min_seg_len {
        result.push(t);
        last_t = t;
      }
    }
    result
  }
}

/// Analyzes a single span `[t_start, t_end]` with a dense curvature pass.
///
/// Samples `n_dense + 1` uniformly-spaced points (both endpoints included), computes chord
/// deviations zeroed at both endpoints (preventing cross-boundary curvature leakage), and
/// builds the density/cumulative arrays used for proportional sample placement.
fn analyze_span<P, E>(
  t_start: f32,
  t_end: f32,
  n_dense: usize,
  sample_fn: &impl Fn(f32) -> Result<P, E>,
) -> Result<SpanData, E>
where
  P: AdaptiveSamplePoint,
{
  let n_pts = n_dense + 1;
  let span_len = t_end - t_start;

  // Uniform t-values covering [t_start, t_end], endpoints included.
  let dense_ts: Vec<f32> = (0..n_pts)
    .map(|i| t_start + span_len * (i as f32 / n_dense as f32))
    .collect();

  // Evaluate the curve at every dense t-value.
  let mut dense_pts: Vec<P> = Vec::with_capacity(n_pts);
  for &t in &dense_ts {
    dense_pts.push(sample_fn(t)?);
  }

  // Chord deviations: forced to 0.0 at both span endpoints (hard boundaries) so that corner
  // curvature at t_start or t_end cannot bleed into this span's density field.
  let mut chord_devs: Vec<f32> = vec![0.0; n_pts];
  for i in 1..n_dense {
    chord_devs[i] = dense_pts[i].distance_to_line(dense_pts[i - 1], dense_pts[i + 1]);
  }

  // Per-sub-segment density: arc length baseline + curvature contribution.
  let mut densities: Vec<f32> = Vec::with_capacity(n_dense);
  for i in 0..n_dense {
    let arc_len = (dense_pts[i + 1] - dense_pts[i]).norm();
    let curvature_avg = (chord_devs[i] + chord_devs[i + 1]) * 0.5;
    densities.push(arc_len + CURVATURE_WEIGHT * curvature_avg);
  }

  // Cumulative prefix sum: cumulative[i] = sum of densities[0..i].
  let mut cumulative: Vec<f32> = Vec::with_capacity(n_pts);
  cumulative.push(0.0);
  for &d in &densities {
    let last = *cumulative.last().unwrap();
    cumulative.push(last + d);
  }

  let mass = *cumulative.last().unwrap();

  Ok(SpanData {
    t_start,
    t_end,
    mass,
    dense_ts,
    densities,
    cumulative,
  })
}

/// Distributes `free_budget` samples across spans proportional to their metric mass.
///
/// Uses the largest-remainder (Hamilton) method to ensure the integer allocations sum exactly
/// to `free_budget`, minimising rounding bias across spans.
fn distribute_budget(free_budget: usize, spans: &[SpanData], total_mass: f32) -> Vec<usize> {
  let n = spans.len();
  if n == 0 {
    return Vec::new();
  }

  let mut allocations = vec![0usize; n];

  if total_mass <= 0.0 || free_budget == 0 {
    return allocations;
  }

  // Raw proportional allocation (floating-point).
  let raw: Vec<f64> = spans
    .iter()
    .map(|s| s.mass as f64 / total_mass as f64 * free_budget as f64)
    .collect();

  // Floor each allocation; distribute the shortfall via largest-remainder method.
  let floored: Vec<usize> = raw.iter().map(|&r| r.floor() as usize).collect();
  let floor_sum: usize = floored.iter().sum();
  let shortfall = free_budget.saturating_sub(floor_sum);

  let mut order: Vec<usize> = (0..n).collect();
  order.sort_by(|&a, &b| {
    let ra = raw[a] - raw[a].floor();
    let rb = raw[b] - raw[b].floor();
    rb.partial_cmp(&ra).unwrap_or(Ordering::Equal)
  });

  for (rank, &i) in order.iter().enumerate() {
    allocations[i] = if rank < shortfall {
      floored[i] + 1
    } else {
      floored[i]
    };
  }

  allocations
}

/// Adaptively samples a curve using curvature-aware density integration (fallible version).
///
/// Like `adaptive_sample` but accepts a fallible sample function that can return errors.
/// This is useful when the sample function involves evaluating user-provided callbacks.
///
/// Uses a span-based algorithm: critical t-values from `initial_ts` define hard span
/// boundaries. Each span is analyzed and sampled independently, preventing curvature from
/// bleeding across corners and making slivers near critical points structurally impossible.
///
/// # Arguments
///
/// * `target_count` - The desired number of sample points
/// * `initial_ts` - Critical t-values to seed the algorithm (will be included in output). Should
///   include at least 0.0 and 1.0 as boundaries.
/// * `sample_fn` - Fallible function that evaluates the curve at a given t in [0, 1]
/// * `min_segment_length` - Minimum segment length to prevent sliver triangles
///
/// # Returns
///
/// A sorted vector of t-values in [0, 1) with up to `target_count` elements,
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

  // --- Step 1: Prepare span boundaries ---
  // Critical t-values (including 1.0) form hard structural boundaries between spans.
  // 1.0 is kept here (previously filtered to < 1.0 - 1e-6) so it becomes a span endpoint.
  let mut boundaries: Vec<f32> = initial_ts
    .iter()
    .copied()
    .filter(|t| t.is_finite() && *t >= 0.0 && *t <= 1.0)
    .collect();
  if boundaries.iter().all(|&t| t > 1e-6) {
    boundaries.push(0.0);
  }
  if boundaries.iter().all(|&t| t < 1.0 - 1e-6) {
    boundaries.push(1.0);
  }
  boundaries.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
  boundaries.dedup_by(|a, b| (*a - *b).abs() < 1e-6);

  // mandatory_count = number of span starts (all boundaries except the final 1.0).
  // These are the t-values emitted as span-start outputs; 1.0 is excluded per API contract.
  let mandatory_count = boundaries.len().saturating_sub(1);

  // --- Step 2: Early exit ---
  // If the mandatory points alone satisfy the target, subsample them directly.
  if mandatory_count >= target_count {
    let mandatory = &boundaries[..mandatory_count];
    let result: Vec<f32> = if target_count == 1 {
      vec![mandatory[0]]
    } else {
      (0..target_count)
        .map(|i| mandatory[i * (mandatory.len() - 1) / (target_count - 1)])
        .collect()
    };
    return Ok(result);
  }

  let free_budget = target_count - mandatory_count;
  let n_spans = mandatory_count; // = boundaries.len() - 1

  // --- Step 3: Per-span analysis ---
  // Each span is independently oversampled. Total evaluations ≈ OVERSAMPLE_FACTOR * target_count,
  // matching the previous global approach.
  let n_dense_per_span = MIN_DENSE_SAMPLES.max(OVERSAMPLE_FACTOR * target_count / n_spans.max(1));

  let mut spans: Vec<SpanData> = Vec::with_capacity(n_spans);
  for i in 0..n_spans {
    let span = analyze_span::<P, E>(
      boundaries[i],
      boundaries[i + 1],
      n_dense_per_span,
      &sample_fn,
    )?;
    spans.push(span);
  }

  // --- Step 4: Budget allocation ---
  let total_mass: f32 = spans.iter().map(|s| s.mass).sum();
  let allocations = distribute_budget(free_budget, &spans, total_mass);

  // --- Step 5: Emit output ---
  // Each span contributes: its start t (mandatory) + interior samples from center-of-mass
  // placement. 1.0 is intentionally not added (API contract: output is in [0, 1)).
  let mut result: Vec<f32> = Vec::with_capacity(target_count);
  for (i, span) in spans.iter().enumerate() {
    result.push(boundaries[i]);
    result.extend(span.sample_internal(allocations[i], min_seg_len));
  }

  result.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
  result.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
  result.truncate(target_count);

  Ok(result)
}

#[cfg(test)]
mod tests {
  use crate::{
    builtins::trace_path::{PathSampler, PathSegment, PathSubpath, PathTracerCallable},
    EvalCtx, Sym,
  };

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
    for &[a, b] in result.array_windows::<2>() {
      assert!(a < b, "Results should be sorted");
    }
  }

  #[test]
  fn test_adaptive_sample_circle_roughly_uniform() {
    // For a circle (constant curvature), samples should be roughly uniform
    let result = adaptive_sample(8, &[0.0, 1.0], circle_profile, 1e-5);

    assert_eq!(result.len(), 8);

    // Check that gaps are reasonably uniform (within 2x of each other)
    let mut gaps: Vec<f32> = Vec::new();
    for &[a, b] in result.array_windows::<2>() {
      let next = b;
      let current = a;
      gaps.push(next - current);
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
    for &[a, b] in result.array_windows::<2>() {
      gaps.push(b - a);
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
    for &[a, b] in result.array_windows::<2>() {
      assert!(a < b, "Results should be sorted");
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

  #[test]
  fn test_bad_adaptive_sampler_repro() {
    let raw_test_data = include_str!("./test_data/adaptive_sampler_repro_pts.txt");

    // format:
    // start_x
    // start_y
    // end_x
    // end_y
    // length

    // 589 line segments

    let mut segments = Vec::new();
    for line in raw_test_data.lines().take(589 * 5).array_chunks::<5>() {
      let start_x: f32 = line[0].parse().unwrap();
      let start_y: f32 = line[1].parse().unwrap();
      let end_x: f32 = line[2].parse().unwrap();
      let end_y: f32 = line[3].parse().unwrap();
      let length: f32 = line[4].parse().unwrap();

      segments.push(PathSegment::Line {
        start: Vec2::new(start_x, start_y),
        end: Vec2::new(end_x, end_y),
        length,
      });
    }

    // on line 2946 (1-indexed) the `cumulative_lengths` list starts
    //
    // all total lengths are equal to the last cumulative length
    let mut cumulative_lens = Vec::new();
    for line in raw_test_data.lines().skip(589 * 5).take(589) {
      let cumulative_len: f32 = line.parse().unwrap();
      cumulative_lens.push(cumulative_len);
    }
    assert_eq!(cumulative_lens[0], 0.07176949);
    let total_len = cumulative_lens.last().copied().unwrap();
    assert_eq!(total_len, 32.876934);

    let sampler = PathTracerCallable {
      interned_t_kwarg: Sym(0),
      subpaths: vec![PathSubpath {
        segments,
        cumulative_lengths: cumulative_lens,
        total_length: total_len,
        closed: true,
      }],
      subpath_cumulative_lengths: vec![total_len],
      total_length: total_len,
      reverse: false,
      override_critical_points: Some(Vec::new()),
      fill_rule: None,
    };
    let ring_resolution = 80;
    let min_segment_length = 0.00001;
    let initial_ts = vec![0.0, 1.0];

    let ctx = EvalCtx::default();
    let sample_fn = |t| sampler.eval_at(t, &ctx);

    let samples =
      adaptive_sample_fallible(ring_resolution, &initial_ts, sample_fn, min_segment_length)
        .unwrap();

    // Verify basic properties
    assert_eq!(
      samples.len(),
      ring_resolution,
      "Should return exactly ring_resolution samples"
    );
    assert!(
      samples.iter().all(|&t| t >= 0.0 && t < 1.0),
      "All samples must be in [0, 1)"
    );
    for &[a, b] in samples.array_windows::<2>() {
      assert!(a < b, "Samples must be strictly increasing");
    }

    // The old greedy algorithm produced a gap from t=0.0 to t=0.1 (8x the expected ~0.0125
    // average gap), because the S-curve midpoints coincided with chords.  Verify this is fixed:
    // no consecutive gap should exceed 4x the average spacing.
    let avg_gap = 1.0 / ring_resolution as f32;
    let max_consecutive_gap = samples
      .array_windows::<2>()
      .map(|&[a, b]| b - a)
      .fold(0.0f32, f32::max);
    assert!(
      max_consecutive_gap < avg_gap * 4.0,
      "Max consecutive gap {max_consecutive_gap:.4} exceeds 4x average {:.4} (old algorithm \
       produced 0.1 gap at the start)",
      avg_gap * 4.0
    );
  }
}
