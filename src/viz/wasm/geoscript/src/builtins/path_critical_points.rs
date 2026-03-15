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

const MIN_CRITICAL_ANGLE: f32 = 0.035;
const DEFAULT_SEGMENT_FRACTION: f32 = 0.15;

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

    for i in 0..n {
      let prev = if i == 0 { n - 1 } else { i - 1 };
      let next = (i + 1) % n;

      let v1 = path[i] - path[prev];
      let v2 = path[next] - path[i];
      let len1 = v1.norm();
      let len2 = v2.norm();

      let mut is_critical = false;

      if len1 > 1e-10 && len2 > 1e-10 {
        let cos_a = (v1.dot(&v2) / (len1 * len2)).clamp(-1.0, 1.0);
        let angle = cos_a.acos();

        if angle > angle_threshold {
          is_critical = true;
        } else if use_segment_check && angle > MIN_CRITICAL_ANGLE {
          if segment_lengths[prev] >= long_threshold || segment_lengths[i] >= long_threshold {
            is_critical = true;
          }
        }
      }

      // explicitly include new vertices created by the boolean op as critical
      if !is_critical {
        if let Some(pre_op) = pre_op_vertices {
          if !pre_op.is_empty() {
            let key = (path[i].x.to_bits(), path[i].y.to_bits());
            if !pre_op.contains(&key) {
              is_critical = true;
            }
          }
        }
      }

      if is_critical {
        t_values.push(cumulative[i] / total_length);
      }
    }
  }

  t_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
  t_values.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
  t_values
}
