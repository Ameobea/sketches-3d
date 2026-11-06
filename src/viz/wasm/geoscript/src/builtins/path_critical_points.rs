use fxhash::FxHashSet;

use crate::Vec2;

pub(crate) type VertexSet = FxHashSet<(u32, u32)>;

pub(crate) fn vertex_key(p: Vec2) -> (u32, u32) {
  // Boolean kernels can change the sign bit of zero without moving a vertex.
  (
    if p.x == 0. { 0 } else { p.x.to_bits() },
    if p.y == 0. { 0 } else { p.y.to_bits() },
  )
}

/// Restore explicit input anchors on a boolean's surviving boundary. Apply this *after* the
/// geometry-only memo lookup so identical polygons with different flags cannot contaminate
/// one another. Interior/clipped-away anchors are discarded. Collinear anchors simplified
/// out by the kernel are reinserted without changing the boundary geometry.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn restore_boundary_anchors(
  paths: &mut [Vec<Vec2>],
  flags: &mut [Vec<bool>],
  source: &[Vec2],
) {
  let source_keys: VertexSet = source.iter().map(|&p| vertex_key(p)).collect();
  let present: VertexSet = paths.iter().flatten().map(|&p| vertex_key(p)).collect();
  let mut missing: Vec<Vec2> = source
    .iter()
    .copied()
    .filter(|&p| !present.contains(&vertex_key(p)))
    .collect();
  missing.sort_unstable_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
  missing.dedup();
  for (path, anchors) in paths.iter_mut().zip(flags) {
    for (&p, flag) in path.iter().zip(anchors.iter_mut()) {
      *flag |= source_keys.contains(&vertex_key(p));
    }
    if missing.is_empty() || path.len() < 2 {
      continue;
    }
    let mut rebuilt = Vec::with_capacity(path.len());
    let mut rebuilt_flags = Vec::with_capacity(path.len());
    let mut insertions = Vec::new();
    for i in 0..path.len() {
      let a = path[i];
      let b = path[(i + 1) % path.len()];
      rebuilt.push(a);
      rebuilt_flags.push(anchors[i]);
      let start = missing.partition_point(|p| p.x < a.x.min(b.x));
      let end = missing.partition_point(|p| p.x <= a.x.max(b.x));
      let edge = b.cast::<f64>() - a.cast::<f64>();
      let len_sq = edge.norm_squared();
      if len_sq == 0. {
        continue;
      }
      insertions.clear();
      for &p in &missing[start..end] {
        if p.y < a.y.min(b.y) || p.y > a.y.max(b.y) {
          continue;
        }
        let delta = p.cast::<f64>() - a.cast::<f64>();
        let cross = edge.x * delta.y - edge.y * delta.x;
        let t = delta.dot(&edge) / len_sq;
        // Do not snap nearby, distinct features onto this edge: only restore points that
        // lie on the output segment, up to f64 arithmetic roundoff on f32 coordinates.
        let roundoff = 4. * f64::EPSILON * (edge.x * delta.y).abs().max((edge.y * delta.x).abs());
        if cross.abs() <= roundoff && t > 0. && t < 1. {
          insertions.push((t, p));
        }
      }
      insertions.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
      for &(_, p) in &insertions {
        rebuilt.push(p);
        rebuilt_flags.push(true);
      }
    }
    *path = rebuilt;
    *anchors = rebuilt_flags;
  }
}

const ANGLE_THRESHOLD: f32 = 0.3;
/// Runs at least this fraction of the perimeter get their endpoints flagged.
const SEGMENT_FRACTION: f32 = 0.05;
/// Minimum angle for the segment-fraction check. Intentionally small: arc-to-straight
/// transitions can end the arc on a very short step (observed ~0.026 rad) even though the
/// vertex borders a long segment; missing those merged arcs into adjacent straights and skewed
/// the adaptive sampler at T-junctions.
const MIN_SEGMENT_CRITICAL_ANGLE: f32 = 1e-4;
/// Angle-based points this close (in `t`) to an already-flagged one are suppressed.
const MIN_T_GAP_FOR_ANGLE_CRITICAL: f32 = 0.005;

#[allow(dead_code)] // used only by wasm-gated path_boolean
pub(crate) fn collect_vertex_set(coords: &[f32]) -> VertexSet {
  coords
    .chunks_exact(2)
    .map(|c| vertex_key(Vec2::new(c[0], c[1])))
    .collect()
}

#[allow(dead_code)] // used only by wasm-gated path_boolean
pub(crate) fn collect_vertex_set_multi(a: &[f32], b: &[f32]) -> VertexSet {
  let mut set = collect_vertex_set(a);
  set.extend(collect_vertex_set(b));
  set
}

/// Per-vertex critical flags for one closed polyline. A vertex is critical when its turn angle
/// exceeds `ANGLE_THRESHOLD`, when it turns at all and borders a run that's long relative to the
/// path, or when it's absent from `pre_op_vertices` (created by a boolean op).
pub(crate) fn detect_critical_vertices(
  path: &[Vec2],
  pre_op_vertices: Option<&VertexSet>,
) -> Vec<bool> {
  let n = path.len();
  if n < 3 {
    return vec![false; n];
  }
  let seg_len = |i: usize| (path[(i + 1) % n] - path[i]).norm();
  let total_length: f32 = (0..n).map(seg_len).sum();
  if total_length < 1e-10 {
    return vec![false; n];
  }
  let mut cumulative = Vec::with_capacity(n);
  cumulative.push(0.0f32);
  for i in 1..n {
    cumulative.push(cumulative[i - 1] + seg_len(i - 1));
  }
  let t_at = |i: usize| cumulative[i] / total_length;
  let angle = |i: usize| -> Option<f32> {
    let v1 = path[i] - path[(i + n - 1) % n];
    let v2 = path[(i + 1) % n] - path[i];
    let (l1, l2) = (v1.norm(), v2.norm());
    (l1 > 1e-10 && l2 > 1e-10).then(|| (v1.dot(&v2) / (l1 * l2)).clamp(-1., 1.).acos())
  };
  // Degenerate edges count as collinear so they get absorbed into the surrounding run.
  let collinear = |i: usize| angle(i).map_or(true, |a| a <= MIN_SEGMENT_CRITICAL_ANGLE);

  let mut is_critical = vec![false; n];

  // Pass 1: merge collinear segments into `(start, end, len)` runs; flag long runs' endpoints.
  let mut runs: Vec<(usize, usize, f32)> = Vec::new();
  let (mut run_start, mut run_len) = (0usize, seg_len(0));
  for i in 1..n {
    if collinear(i) {
      run_len += seg_len(i);
    } else {
      runs.push((run_start, i, run_len));
      run_start = i;
      run_len = seg_len(i);
    }
  }
  if runs.is_empty() {
    runs.push((run_start, run_start, run_len));
  } else if collinear(0) {
    runs[0] = (run_start, runs[0].1, run_len + runs[0].2);
  } else {
    runs.push((run_start, n - 1, run_len));
  }
  let mut run_ts = Vec::new();
  for &(start, end, len) in &runs {
    if len < SEGMENT_FRACTION * total_length {
      continue;
    }
    for v in [start, end] {
      if !is_critical[v] && angle(v).is_some_and(|a| a > MIN_SEGMENT_CRITICAL_ANGLE) {
        is_critical[v] = true;
        run_ts.push(t_at(v));
      }
    }
  }
  run_ts.sort_unstable_by(f32::total_cmp);

  // Pass 2: sharp turns, spaced from pass-1 points and from each other (monotone in `i`).
  let mut last_t = f32::NEG_INFINITY;
  for i in 0..n {
    if is_critical[i] || !angle(i).is_some_and(|a| a > ANGLE_THRESHOLD) {
      continue;
    }
    let t = t_at(i);
    let k = run_ts.partition_point(|&r| r < t);
    let near = |j: Option<usize>| {
      j.and_then(|j| run_ts.get(j))
        .is_some_and(|&r| (t - r).abs() < MIN_T_GAP_FOR_ANGLE_CRITICAL)
    };
    if !(near(Some(k)) || near(k.checked_sub(1)) || t - last_t < MIN_T_GAP_FOR_ANGLE_CRITICAL) {
      is_critical[i] = true;
      last_t = t;
    }
  }

  if let Some(pre_op) = pre_op_vertices.filter(|s| !s.is_empty()) {
    for (p, c) in path.iter().zip(&mut is_critical) {
      *c |= !pre_op.contains(&vertex_key(*p));
    }
  }
  is_critical
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn boolean_anchor_restoration_keeps_boundary_features_only() {
    let v = Vec2::new;
    let outline = vec![v(0., 0.), v(4., 0.), v(4., 4.), v(0., 4.)];
    let mut paths = vec![outline.clone()];
    let mut flags = vec![vec![false; 4]];
    let source = [
      v(-0., 0.),
      v(3., 0.),
      v(1., 0.),
      v(2., 2.),
      v(8., 0.),
      v(2., 1e-6),
    ];
    restore_boundary_anchors(&mut paths, &mut flags, &source);
    assert_eq!(
      paths[0],
      vec![
        v(0., 0.),
        v(1., 0.),
        v(3., 0.),
        v(4., 0.),
        v(4., 4.),
        v(0., 4.)
      ]
    );
    assert_eq!(flags[0], vec![true, true, true, false, false, false]);
    // Reapplying restoration is idempotent; a fresh geometry-cache clone has no stale flags.
    let expected = paths.clone();
    restore_boundary_anchors(&mut paths, &mut flags, &source);
    assert_eq!(paths, expected);
    let mut unmarked = vec![outline];
    let mut unmarked_flags = vec![vec![false; 4]];
    restore_boundary_anchors(&mut unmarked, &mut unmarked_flags, &[]);
    assert_eq!(unmarked_flags[0], vec![false; 4]);
  }

  #[test]
  fn surviving_explicit_corner_bypasses_detector_spacing_suppression() {
    let v = Vec2::new;
    let p = vec![
      v(0., 0.),
      v(0.25, 0.),
      v(0.5, 0.25),
      v(100., 0.25),
      v(100., 100.),
      v(0., 100.),
    ];
    let mut flags = vec![detect_critical_vertices(&p, None)];
    assert!(
      !flags[0][1],
      "repro depends on the short corner being suppressed"
    );
    let mut paths = vec![p.clone()];
    restore_boundary_anchors(&mut paths, &mut flags, &p);
    assert!(flags[0].iter().all(|&a| a));
  }

  /// A short final arc step leaves a tiny turn (~0.026 rad) at the arc→straight transition; the
  /// long outgoing run must still flag it.
  #[test]
  fn test_arc_to_straight_transition_with_small_angle_is_critical() {
    let arc_dir: f32 = 1.0_f32;
    let out_dir: f32 = arc_dir + 0.026;

    let v0 = Vec2::new(0.0, 0.0);
    let v1 = Vec2::new(3.0, 0.0);
    let v2 = Vec2::new(v1.x + 0.008 * arc_dir.cos(), v1.y + 0.008 * arc_dir.sin());
    let v3 = Vec2::new(v2.x + 3.0 * out_dir.cos(), v2.y + 3.0 * out_dir.sin());

    let incoming = v2 - v1;
    let outgoing = v3 - v2;
    let cos_a = (incoming.dot(&outgoing) / (incoming.norm() * outgoing.norm())).clamp(-1.0, 1.0);
    let angle_at_v2 = cos_a.acos();
    assert!(
      angle_at_v2 > 1e-4 && angle_at_v2 < 0.035,
      "test setup: angle_at_v2={angle_at_v2:.4} should be in (1e-4, 0.035)"
    );

    let path = vec![v0, v1, v2, v3];
    let critical = detect_critical_vertices(&path, None);
    assert!(
      critical[2],
      "arc→straight transition vertex (angle={angle_at_v2:.4} rad) must be critical because its \
       outgoing segment is long; flags={critical:?}"
    );
  }
}
