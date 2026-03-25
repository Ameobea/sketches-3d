use std::collections::HashSet;

use crate::Vec2;

pub(crate) type VertexSet = HashSet<(u32, u32)>;

pub(crate) struct CriticalPointConfig {
  pub angle_threshold: f32,
  pub segment_fraction: f32,
}

impl Default for CriticalPointConfig {
  fn default() -> Self {
    CriticalPointConfig {
      angle_threshold: 0.3,
      segment_fraction: 0.0,
    }
  }
}

/// Minimum angle for the segment-fraction check.
///
/// Intentionally small: the check fires on arc-to-straight (or straight-to-arc) transitions
/// where the last arc step can be very short, making the direction change proportionally tiny
/// (observed as ~0.026 rad in practice) even though the vertex borders a long segment.
/// A larger threshold (e.g. 0.035 rad) caused missed critical points when Clipper2 emitted a
/// partial final arc step, merging the arc and the adjacent straight section into one span and
/// producing asymmetric over-sampling at T-junction profiles.
const MIN_SEGMENT_CRITICAL_ANGLE: f32 = 1e-4;
const DEFAULT_SEGMENT_FRACTION: f32 = 0.05;

pub(crate) fn collect_vertex_set(coords: &[f64]) -> VertexSet {
  let mut set = VertexSet::default();
  let mut i = 0;
  while i + 1 < coords.len() {
    let x = (coords[i] as f32).to_bits();
    let y = (coords[i + 1] as f32).to_bits();
    set.insert((x, y));
    i += 2;
  }
  set
}

pub(crate) fn collect_vertex_set_multi(a: &[f64], b: &[f64]) -> VertexSet {
  let mut set = collect_vertex_set(a);
  let mut i = 0;
  while i + 1 < b.len() {
    let x = (b[i] as f32).to_bits();
    let y = (b[i + 1] as f32).to_bits();
    set.insert((x, y));
    i += 2;
  }
  set
}

/// Detects "critical t values" along a provided path.  Critical t values correspond to
/// sharp features on the path which should be preserved exactly during later sampling.
///
/// A point is considered critical if any of the following apply:
///  - The angle between its adjacent segments exceeds `config.angle_threshold`
///  - The angle exceeds a small minimum and one of the adjacent segments is long relative to the
///    total path length
///  - The vertex wasn't present in the original geometry meaning that it was created by the boolean
///    operation and should be preserved explicitly
pub(crate) fn detect_critical_points(
  paths: &[Vec<Vec2>],
  config: &CriticalPointConfig,
  pre_op_vertices: Option<&VertexSet>,
) -> Vec<f32> {
  let mut t_values: Vec<f32> = Vec::new();

  if paths.is_empty() {
    return t_values;
  }

  let angle_threshold = config.angle_threshold.max(0.0);
  let seg_fraction = if config.segment_fraction <= 0.0 {
    DEFAULT_SEGMENT_FRACTION
  } else {
    config.segment_fraction
  };
  let use_segment_check = seg_fraction < 1.0;

  /// Minimum `t` gap between an existing (segment-length) critical point and a
  /// new angle-based critical point.  Angle-based points that fall within this
  /// distance of an already-flagged point are suppressed to avoid near-duplicates.
  const MIN_T_GAP_FOR_ANGLE_CRITICAL: f32 = 0.005;

  for path in paths {
    let n = path.len();
    if n < 3 {
      continue;
    }

    let mut segment_lengths = Vec::with_capacity(n);
    let mut total_length = 0.0f32;
    for i in 0..n {
      let next = (i + 1) % n;
      let len = (path[next] - path[i]).norm();
      segment_lengths.push(len);
      total_length += len;
    }

    if total_length < 1e-10 {
      continue;
    }

    let mut cumulative = Vec::with_capacity(n);
    cumulative.push(0.0f32);
    for i in 1..n {
      cumulative.push(cumulative[i - 1] + segment_lengths[i - 1]);
    }

    let long_threshold = seg_fraction * total_length;

    // Track which vertices are already flagged as critical (by index).
    let mut is_critical = vec![false; n];

    // ── Pass 1: segment-length critical points ──────────────────────────
    // Greedily merge consecutive collinear segments (angle between them
    // ≤ MIN_SEGMENT_CRITICAL_ANGLE) into runs, then flag the endpoints of
    // any run whose total length exceeds `long_threshold`.
    if use_segment_check {
      // Build merged collinear runs.  Each run is (start_vertex, end_vertex, total_length).
      let mut runs: Vec<(usize, usize, f32)> = Vec::new();
      let mut run_start = 0usize;
      let mut run_len = segment_lengths[0];

      for i in 1..n {
        // Angle at vertex i between incoming segment (i-1 → i) and outgoing (i → i+1)
        let prev = i - 1;
        let next = (i + 1) % n;
        let v1 = path[i] - path[prev];
        let v2 = path[next] - path[i];
        let l1 = v1.norm();
        let l2 = v2.norm();

        let collinear = if l1 > 1e-10 && l2 > 1e-10 {
          let cos_a = (v1.dot(&v2) / (l1 * l2)).clamp(-1., 1.);
          cos_a.acos() <= MIN_SEGMENT_CRITICAL_ANGLE
        } else {
          // Degenerate edge — treat as collinear so it gets absorbed
          true
        };

        if collinear {
          // Extend the current run through segment i
          run_len += segment_lengths[i];
        } else {
          // Close the current run and start a new one
          runs.push((run_start, i, run_len));
          run_start = i;
          run_len = segment_lengths[i];
        }
      }

      // Close the final run.  For a closed polygon the last run may wrap
      // around and be collinear with the first run; merge them if so.
      if runs.is_empty() {
        // Every segment is collinear — one big run around the whole polygon
        runs.push((run_start, run_start, run_len));
      } else {
        let first_start = runs[0].0;
        let first_end = runs[0].1;

        // Check if last run's end → vertex 0 → first run's start is collinear
        let last_end_vertex = run_start; // start vertex of the not-yet-pushed run
        let wrap_collinear = if first_start == 0 && last_end_vertex != 0 {
          let v_at_0 = path[0];
          let v_prev = path[n - 1];
          let v_next = path[1 % n];
          let v1 = v_at_0 - v_prev;
          let v2 = v_next - v_at_0;
          let l1 = v1.norm();
          let l2 = v2.norm();
          if l1 > 1e-10 && l2 > 1e-10 {
            let cos_a = (v1.dot(&v2) / (l1 * l2)).clamp(-1., 1.);
            cos_a.acos() <= MIN_SEGMENT_CRITICAL_ANGLE
          } else {
            true
          }
        } else {
          false
        };

        if wrap_collinear {
          // Merge: absorb the first run into the last run
          let merged_len = run_len + runs[0].2;
          runs[0] = (run_start, first_end, merged_len);
        } else {
          runs.push((run_start, n - 1, run_len));
        }
      }

      for &(start, end, len) in &runs {
        if len < long_threshold {
          continue;
        }

        // Flag the start vertex of this run (angle with the preceding run)
        {
          let prev_of_start = if start == 0 { n - 1 } else { start - 1 };
          let next_of_start = (start + 1) % n;
          let v1 = path[start] - path[prev_of_start];
          let v2 = path[next_of_start] - path[start];
          if v1.norm() > 1e-10 && v2.norm() > 1e-10 {
            let cos_a = (v1.dot(&v2) / (v1.norm() * v2.norm())).clamp(-1., 1.);
            if cos_a.acos() > MIN_SEGMENT_CRITICAL_ANGLE && !is_critical[start] {
              let t_i = cumulative[start] / total_length;
              is_critical[start] = true;
              t_values.push(t_i);
            }
          }
        }

        // Flag the end vertex of this run (angle with the following run)
        let end_next = (end + 1) % n;
        {
          let prev_of_end = if end == 0 { n - 1 } else { end - 1 };
          let v1 = path[end] - path[prev_of_end];
          let v2 = path[end_next] - path[end];
          if v1.norm() > 1e-10 && v2.norm() > 1e-10 {
            let cos_a = (v1.dot(&v2) / (v1.norm() * v2.norm())).clamp(-1., 1.);
            if cos_a.acos() > MIN_SEGMENT_CRITICAL_ANGLE && !is_critical[end] {
              let t_end = cumulative[end] / total_length;
              is_critical[end] = true;
              t_values.push(t_end);
            }
          }
        }
      }
    }

    // ── Pass 2: angle-based critical points ─────────────────────────────
    // Flag vertices where the direction change exceeds the angle threshold,
    // unless the vertex is already critical or too close to one that is.
    for i in 0..n {
      if is_critical[i] {
        continue;
      }

      let prev = if i == 0 { n - 1 } else { i - 1 };
      let next = (i + 1) % n;

      let v1 = path[i] - path[prev];
      let v2 = path[next] - path[i];
      let len1 = v1.norm();
      let len2 = v2.norm();

      if len1 > 1e-10 && len2 > 1e-10 {
        let cos_a = (v1.dot(&v2) / (len1 * len2)).clamp(-1., 1.);
        let angle = cos_a.acos();

        if angle > angle_threshold {
          let t_i = cumulative[i] / total_length;

          // Check proximity to any already-flagged critical point
          let too_close = t_values
            .iter()
            .any(|&t| (t_i - t).abs() < MIN_T_GAP_FOR_ANGLE_CRITICAL);

          if too_close {
          } else {
            is_critical[i] = true;
            t_values.push(t_i);
          }
        }
      }
    }

    // // Pass 3 (disabled): explicitly include new vertices created by the boolean op
    // if let Some(pre_op) = pre_op_vertices {
    //   if !pre_op.is_empty() {
    //     for i in 0..n {
    //       if is_critical[i] {
    //         continue;
    //       }
    //       let key = (path[i].x.to_bits(), path[i].y.to_bits());
    //       if !pre_op.contains(&key) {
    //         let t_i = cumulative[i] / total_length;
    //         is_critical[i] = true;
    //         t_values.push(t_i);
    //       }
    //     }
    //   }
    // }
  }

  t_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
  t_values.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
  t_values
}

#[cfg(test)]
mod tests {
  use super::*;

  /// Regression test for the asymmetric T-junction sampling bug.
  ///
  /// When Clipper2 generates a round-offset path it occasionally ends an arc with a partial
  /// (shorter-than-normal) step so that the arc terminates exactly at the adjacent straight
  /// edge.  That final step can be so short that the direction change at the arc→straight
  /// transition vertex is only ~0.026 rad — well below the old `MIN_CRITICAL_ANGLE` of 0.035,
  /// causing the vertex to be silently skipped by the segment-fraction check.
  ///
  /// The result was that the entire arc plus its following long straight ended up in one span,
  /// giving the adaptive sampler a skewed curvature map and dumping far too many samples on one
  /// side of the T-junction.
  ///
  /// After the fix (using `MIN_SEGMENT_CRITICAL_ANGLE = 1e-4` instead of `MIN_CRITICAL_ANGLE`
  /// for the segment check) this transition vertex must be detected as critical.
  #[test]
  fn test_arc_to_straight_transition_with_small_angle_is_critical() {
    // Build a 4-vertex closed polygon that mimics the right-junction geometry:
    //
    //   v0 ──────── 3.0 ─────────── v1
    //                                │ arc step (len≈0.008, dir≈1 rad from horizontal)
    //                               v2  ← THE TRANSITION VERTEX
    //                                │ long outgoing (len≈3.0, dir≈1.026 rad from horizontal)
    //                               v3
    //   └──── closing diagonal (len≈5.2) ──────────────────────────────┘
    //
    // At v2: incoming = short arc step, outgoing = long straight, angle ≈ 0.026 rad.
    // 0.026 rad is above MIN_SEGMENT_CRITICAL_ANGLE (1e-4) but BELOW the old
    // MIN_CRITICAL_ANGLE (0.035), so the old code would miss it.

    let arc_dir: f32 = 1.0_f32; // direction of the arc step (radians from +x)
    let out_dir: f32 = arc_dir + 0.026; // outgoing straight is 0.026 rad from arc step

    let v0 = Vec2::new(0.0, 0.0);
    let v1 = Vec2::new(3.0, 0.0);
    let v2 = Vec2::new(v1.x + 0.008 * arc_dir.cos(), v1.y + 0.008 * arc_dir.sin());
    let v3 = Vec2::new(v2.x + 3.0 * out_dir.cos(), v2.y + 3.0 * out_dir.sin());

    // Verify the angle at v2 is in the problematic range: > 1e-4, < 0.035
    let incoming = v2 - v1;
    let outgoing = v3 - v2;
    let cos_a = (incoming.dot(&outgoing) / (incoming.norm() * outgoing.norm())).clamp(-1.0, 1.0);
    let angle_at_v2 = cos_a.acos();
    assert!(
      angle_at_v2 > 1e-4 && angle_at_v2 < 0.035,
      "test setup: angle_at_v2={angle_at_v2:.4} should be in (1e-4, 0.035)"
    );

    let path = vec![v0, v1, v2, v3];
    let config = CriticalPointConfig::default();
    let critical = detect_critical_points(&[path], &config, None);

    // Compute the expected t-value for v2.
    let seg_v0_v1 = (v1 - v0).norm();
    let seg_v1_v2 = (v2 - v1).norm();
    let total: f32 = seg_v0_v1 + seg_v1_v2 + (v3 - v2).norm() + (v0 - v3).norm();
    let t_v2 = (seg_v0_v1 + seg_v1_v2) / total;

    assert!(
      critical.iter().any(|&t| (t - t_v2).abs() < 1e-4),
      "arc→straight transition vertex (angle={angle_at_v2:.4} rad, t≈{t_v2:.4}) must be detected \
       as critical because its outgoing segment is long, but was not found in \
       critical_t_values={critical:?}"
    );
  }
}
