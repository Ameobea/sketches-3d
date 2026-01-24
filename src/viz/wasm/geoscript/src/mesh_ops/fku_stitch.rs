//! FKU (Fuchs/Kedem/Uselton) Dynamic Programming Stitching Algorithm
//!
//! This module provides an optimal triangulation algorithm for connecting two rings/rows
//! of vertices with potentially different vertex counts.  It uses dynamic programming to
//! minimize the total cost (edge length) of connecting the rings.  It's basically a constrained
//! pathfinding problem in a 2D grid.
//!
//! The DP table represents states (i, j) where we've connected vertices 0..i from
//! ring A to vertices 0..j from ring B. At each state, we can either:
//! - Advance on ring A: create triangle (A[i-1], A[i], B[j])
//! - Advance on ring B: create triangle (A[i], B[j-1], B[j])
//!
//! Reference: Fuchs, Kedem, Uselton (1977) - "Optimal Surface Reconstruction from
//! Planar Contours"
//!
//! https://www.cs.jhu.edu/~misha/Fall13b/Papers/Fuchs77.pdf

use std::cmp::Ordering;

use bitvec::prelude::*;
use mesh::linked_mesh::Vec3;

/// Maximum ring/row resolution for DP-based stitching.  Beyond this, we fall back to uniform
/// stitching.  This limit exists because the DP algorithm has O(N*M) time and space complexity.
/// At 5000 vertices per ring, the DP table is 25M cells (~200MB) which should be fine.
pub const MAX_DP_STITCH_RESOLUTION: usize = 5000;

const AREA_WEIGHT: f32 = 0.35;
const EDGE_LEN_WEIGHT: f32 = 1.;

/// Cost function for DP stitching.
///
/// Computes a combined cost metric based on triangle area and edge length.
/// - `p1`, `p2`: The two vertices on the ring that is advancing (the "segment" being added)
/// - `p3`: The vertex on the opposite ring (the "pivot")
///
/// The triangle formed is (p1, p2, p3). The new edge added to the triangulation is (p2, p3).
#[inline]
pub fn dp_stitch_cost(p1: Vec3, p2: Vec3, p3: Vec3) -> f32 {
  let edge1 = p2 - p1;
  let edge2 = p3 - p1;
  let area = edge1.cross(&edge2).norm() * 0.5;

  let edge_len = (p2 - p3).norm();

  AREA_WEIGHT * area + EDGE_LEN_WEIGHT * edge_len
}

/// Creates a physically rotated copy of a ring.
/// This eliminates modulo operations in the DP solver's inner loop.
fn rotate_ring(pts: &[Vec3], offset: usize) -> Vec<Vec3> {
  let m = pts.len();
  if offset == 0 || m == 0 {
    return pts.to_vec();
  }
  (0..m).map(|i| pts[(i + offset) % m]).collect()
}

/// Represents a move direction in the DP backtracking phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DpMove {
  /// Advance on ring A (horizontal move in DP table)
  AdvanceA,
  /// Advance on ring B (vertical move in DP table)
  AdvanceB,
}

/// DP table storing data in SoA format.
struct DpTable {
  costs: Vec<f32>,
  came_from: BitVec,
  cols: usize,
}

impl DpTable {
  /// Creates a new DP table with the given dimensions.
  ///
  /// Costs are initialized to INFINITY, came_from to AdvanceA (0).
  fn new(rows: usize, cols: usize) -> Self {
    let size = rows * cols;
    Self {
      costs: vec![f32::INFINITY; size],
      came_from: bitvec![0; size],
      cols,
    }
  }

  #[inline]
  fn ix(&self, row: usize, col: usize) -> usize {
    row * self.cols + col
  }

  #[inline]
  pub fn get_cost(&self, row: usize, col: usize) -> f32 {
    self.costs[self.ix(row, col)]
  }

  #[inline]
  pub fn set_cost(&mut self, row: usize, col: usize, cost: f32) {
    let idx = self.ix(row, col);
    self.costs[idx] = cost;
  }

  #[inline]
  pub fn get_came_from(&self, row: usize, col: usize) -> DpMove {
    let idx = self.ix(row, col);
    if self.came_from[idx] {
      DpMove::AdvanceB
    } else {
      DpMove::AdvanceA
    }
  }

  #[inline]
  pub fn set(&mut self, row: usize, col: usize, cost: f32, came_from: DpMove) {
    let idx = self.ix(row, col);
    self.costs[idx] = cost;
    self.came_from.set(idx, came_from == DpMove::AdvanceB);
  }

  /// Efficiently populates the first row using a generator/iterator for edge costs.
  /// This optimizes by avoiding index multiplication and reading back from the table.
  pub fn init_row_0(&mut self, count: usize, mut get_edge_cost: impl FnMut(usize) -> f32) {
    let mut current_cost = self.costs[0];
    for j in 1..=count {
      let edge_cost = get_edge_cost(j);
      current_cost += edge_cost;
      self.costs[j] = current_cost;
      self.came_from.set(j, true); // AdvanceB
    }
  }

  /// Efficiently populates the first column using a generator/iterator for edge costs.
  /// This optimizes by using stride addition instead of multiplication and skips
  /// setting came_from since the default (false/AdvanceA) is correct.
  pub fn init_col_0(&mut self, count: usize, mut get_edge_cost: impl FnMut(usize) -> f32) {
    let mut current_cost = self.costs[0];
    let stride = self.cols;
    let mut idx = 0;

    for i in 1..=count {
      idx += stride;
      let edge_cost = get_edge_cost(i);
      current_cost += edge_cost;
      self.costs[idx] = current_cost;
      // No need to set `came_from``, defaults to 0 (AdvanceA)
    }
  }
}

/// Find the best starting offset for ring B to minimize twist/misalignment with ring A.
/// This rotates ring B's starting index to best align with ring A's starting position.
pub fn find_best_ring_alignment(pts_a: &[Vec3], pts_b: &[Vec3]) -> usize {
  if pts_b.is_empty() || pts_a.is_empty() {
    return 0;
  }

  // Use centroid-based alignment: find the offset that minimizes distance between
  // the first vertex of A and the corresponding vertex of B
  let first_a = pts_a[0];
  let mut best_offset = 0;
  let mut best_dist = f32::INFINITY;

  for offset in 0..pts_b.len() {
    let dist = (pts_b[offset] - first_a).norm_squared();
    if dist < best_dist {
      best_dist = dist;
      best_offset = offset;
    }
  }

  best_offset
}

/// Performs FKU DP stitching between two rings/strips of 3D points.
///
/// For closed rings (`CLOSED=true`), the algorithm naturally handles wrap-around
/// by extending the DP table - vertices at index n/m wrap to index 0, allowing
/// the algorithm to find the globally optimal triangulation including the seam.
///
/// The `CLOSED` const generic ensures specialized code generation for both cases,
/// eliminating runtime branching in the hot inner loop.
///
/// Returns a vector of (i, j, move) tuples representing the DP path.
/// These can be used to generate triangles connecting the two rings.
///
/// Note: For closed rings, pts_b should be pre-rotated using `rotate_ring` and
/// `find_best_ring_alignment` before calling this function.
pub fn dp_stitch_solve<const CLOSED: bool>(
  pts_a: &[Vec3],
  pts_b: &[Vec3],
) -> std::iter::Rev<<Vec<(usize, usize, DpMove)> as IntoIterator>::IntoIter> {
  let n = pts_a.len();
  let m = pts_b.len();

  if n == 0 || m == 0 {
    return Vec::new().into_iter().rev();
  }

  // For closed rings, we extend the table by 1 to handle wrap-around.
  // State (n, m) for closed rings means we've completed the loop.
  // Vertex access uses modulo to wrap: index n -> 0, index m -> 0.
  let table_n = if CLOSED { n + 1 } else { n };
  let table_m = if CLOSED { m + 1 } else { m };

  // Vertex accessors with wrap-around for closed rings
  let get_a = |i: usize| -> Vec3 { pts_a[if CLOSED && i == n { 0 } else { i }] };
  let get_b = |j: usize| -> Vec3 { pts_b[if CLOSED && j == m { 0 } else { j }] };

  // Allocate DP table as (table_n+1) x (table_m+1) grid using SoA layout
  // State (i, j) means we've processed vertices 0..i from A and 0..j from B
  let mut table = DpTable::new(table_n + 1, table_m + 1);

  table.set_cost(0, 0, 0.0);

  // Fill first row (only advancing on B)
  table.init_row_0(table_m, |j| {
    if j == 1 {
      EDGE_LEN_WEIGHT * (get_a(0) - get_b(0)).norm()
    } else {
      // Triangle: (A[0], B[j-2], B[j-1])
      dp_stitch_cost(get_b(j - 2), get_b(j - 1), get_a(0))
    }
  });

  // Fill first column (only advancing on A)
  table.init_col_0(table_n, |i| {
    if i == 1 {
      EDGE_LEN_WEIGHT * (get_a(0) - get_b(0)).norm()
    } else {
      // Triangle: (B[0], A[i-2], A[i-1])
      dp_stitch_cost(get_a(i - 2), get_a(i - 1), get_b(0))
    }
  });

  // Fill rest of the table
  for i in 1..=table_n {
    for j in 1..=table_m {
      // Option 1: Advance on A (horizontal move)
      // Triangle formed: (A[i-2], A[i-1], B[j-1])
      let cost_advance_a = {
        let prev_cost = table.get_cost(i - 1, j);
        if i == 1 {
          prev_cost
        } else {
          let edge_cost = dp_stitch_cost(get_a(i - 2), get_a(i - 1), get_b(j - 1));
          prev_cost + edge_cost
        }
      };

      // Option 2: Advance on B (vertical move)
      // Triangle formed: (A[i-1], B[j-2], B[j-1])
      let cost_advance_b = {
        let prev_cost = table.get_cost(i, j - 1);
        if j == 1 {
          prev_cost
        } else {
          let edge_cost = dp_stitch_cost(get_b(j - 2), get_b(j - 1), get_a(i - 1));
          prev_cost + edge_cost
        }
      };

      // Pick the cheaper option
      if cost_advance_a <= cost_advance_b {
        table.set(i, j, cost_advance_a, DpMove::AdvanceA);
      } else {
        table.set(i, j, cost_advance_b, DpMove::AdvanceB);
      }
    }
  }

  // Backtrack to build the triangle list
  let mut moves = Vec::with_capacity(table_n + table_m);
  let mut i = table_n;
  let mut j = table_m;

  while i > 0 || j > 0 {
    let came_from = table.get_came_from(i, j);
    moves.push((i, j, came_from));

    match came_from {
      DpMove::AdvanceA => {
        if i > 0 {
          i -= 1;
        }
      }
      DpMove::AdvanceB => {
        if j > 0 {
          j -= 1;
        }
      }
    }

    // Safety check to prevent infinite loops
    if i == 0 && j == 0 {
      break;
    }
  }

  moves.into_iter().rev()
}

/// Merges base samples with critical points, snapping nearby values together and ensuring
/// critical points take priority when overlaps occur.
///
/// This prevents creating very thin or degenerate triangles when critical points are nearly
/// coincident, while still preserving the baseline sampling distribution.
pub fn snap_critical_points(
  base_samples: &[f32],
  critical_points: &[f32],
  ring_resolution: usize,
) -> Vec<f32> {
  if base_samples.is_empty() && critical_points.is_empty() {
    return Vec::new();
  }

  let mut base: Vec<f32> = base_samples
    .iter()
    .copied()
    .filter(|v| v.is_finite())
    .map(|v| v.clamp(0., 1.))
    .filter(|v| *v < 1.)
    .collect();
  base.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));

  let mut min_step: Option<f32> = None;
  for window in base.windows(2) {
    let step = window[1] - window[0];
    if step > 0.0 {
      min_step = Some(min_step.map_or(step, |prev| prev.min(step)));
    }
  }

  let fallback_step = 1. / (ring_resolution.max(1) as f32);
  let step = min_step
    .filter(|v| v.is_finite())
    .unwrap_or(fallback_step)
    .max(fallback_step);

  // TODO: the base epsilon and this logic needs review
  // Critical-critical snapping uses a larger epsilon to avoid nearly coincident guides.
  let critical_snap_epsilon = step * 0.5;
  // Base-critical snapping is tighter so "extra" critical points can still be added.
  let base_snap_epsilon = step * 0.25;

  let mut critical: Vec<f32> = critical_points
    .iter()
    .copied()
    .filter(|v| v.is_finite())
    .map(|v| v.clamp(0., 1.))
    .filter(|v| *v < 1.)
    .collect();
  critical.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
  critical.dedup_by(|a, b| (*a - *b).abs() <= critical_snap_epsilon);

  #[derive(Clone, Copy)]
  struct SamplePoint {
    t: f32,
    is_critical: bool,
  }

  let mut points = Vec::with_capacity(base.len() + critical.len());
  points.extend(base.into_iter().map(|t| SamplePoint {
    t,
    is_critical: false,
  }));
  points.extend(critical.into_iter().map(|t| SamplePoint {
    t,
    is_critical: true,
  }));
  points.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(Ordering::Equal));

  let mut out = Vec::with_capacity(points.len());
  let mut idx = 0usize;
  while idx < points.len() {
    let mut chosen = points[idx];
    let mut has_critical = chosen.is_critical;
    let mut last_t = chosen.t;
    idx += 1;

    while idx < points.len() && (points[idx].t - last_t).abs() <= base_snap_epsilon {
      if points[idx].is_critical && !has_critical {
        chosen = points[idx];
        has_critical = true;
      }
      last_t = points[idx].t;
      idx += 1;
    }

    out.push(chosen.t);
  }

  out
}

/// Performs DP-based stitching between two rows/rings with pre-sampled vertex positions.
///
/// This function takes two rows of pre-sampled vertices and generates triangles to connect them
/// using the FKU DP algorithm.
pub fn dp_stitch_presampled(
  pts_a: &[Vec3],
  pts_b: &[Vec3],
  ring_a_base_idx: usize,
  ring_b_base_idx: usize,
  closed: bool,
  out_indices: &mut Vec<u32>,
) {
  let n = pts_a.len();
  let m = pts_b.len();

  if n == 0 || m == 0 {
    return;
  }

  // Find best alignment for ring B (only matters for closed rings, but harmless for open)
  let b_offset = if closed {
    find_best_ring_alignment(pts_a, pts_b)
  } else {
    0
  };

  // Pre-rotate ring B positions to avoid a bunch of modulo ops in the DP solver
  let rotated_pts_b = rotate_ring(pts_b, b_offset);

  let moves = if closed {
    dp_stitch_solve::<true>(pts_a, &rotated_pts_b)
  } else {
    dp_stitch_solve::<false>(pts_a, &rotated_pts_b)
  };

  // Map DP indices to actual vertex buffer indices.
  // Ring A: DP index i maps directly to vertex buffer (with wrap for closed rings)
  // Ring B: DP index j maps to rotated position, need to unrotate for vertex buffer
  let get_a_vtx_ix = |i: usize| -> u32 { (ring_a_base_idx + (i % n)) as u32 };
  let get_b_vtx_ix = |j: usize| -> u32 { (ring_b_base_idx + ((j + b_offset) % m)) as u32 };

  // Generate triangles from DP moves.
  // For closed rings, the solver includes wrap-around moves, so we generate all
  // triangles including the seam (no manual closing needed).
  for (i, j, mv) in moves {
    if i == 0 && j == 0 {
      continue;
    }

    match mv {
      DpMove::AdvanceA => {
        // Triangle: (A[i-2], A[i-1], B[j-1])
        // For open strips, skip when i <= 1 since A[i-2] would be out of bounds
        // For closed rings, i can go up to n+1 and wraps around
        if i > 1 {
          // If j=0, we match against B[0]. This covers the case where the path hugs the A-axis.
          // For closed rings, this stitches the seam.
          if closed || j > 0 {
            let idx_a_prev = get_a_vtx_ix(i - 2);
            let idx_a_curr = get_a_vtx_ix(i - 1);
            let b_idx_raw = if j == 0 { 0 } else { j - 1 };
            let idx_b = get_b_vtx_ix(b_idx_raw);
            out_indices.extend_from_slice(&[idx_a_prev, idx_a_curr, idx_b]);
          }
        }
      }
      DpMove::AdvanceB => {
        // Triangle: (A[i-1], B[j-1], B[j-2])
        // For open strips, skip when j <= 1 since B[j-2] would be out of bounds
        // For closed rings, j can go up to m+1 and wraps around
        if j > 1 {
          // If i=0, we match against A[0]. This covers the case where the path hugs the B-axis.
          if closed || i > 0 {
            let a_idx_raw = if i == 0 { 0 } else { i - 1 };
            let idx_a = get_a_vtx_ix(a_idx_raw);
            let idx_b_prev = get_b_vtx_ix(j - 2);
            let idx_b_curr = get_b_vtx_ix(j - 1);
            out_indices.extend_from_slice(&[idx_a, idx_b_curr, idx_b_prev]);
          }
        }
      }
    }
  }
}

/// Performs simple uniform stitching between two rows of equal vertex count
///
/// This is a fallback for when DP stitching is disabled or provides negligable benefits compared to
/// this simpler baseline
pub fn uniform_stitch_rows(
  row_a_base_idx: usize,
  row_b_base_idx: usize,
  count: usize,
  v_closed: bool,
  flip: bool,
  indices: &mut Vec<u32>,
) {
  let wrap_count = if v_closed {
    count
  } else {
    count.saturating_sub(1)
  };

  for j in 0..wrap_count {
    let j_next = (j + 1) % count;

    let a = (row_a_base_idx + j) as u32;
    let b = (row_a_base_idx + j_next) as u32;
    let c = (row_b_base_idx + j) as u32;
    let d = (row_b_base_idx + j_next) as u32;

    if flip {
      indices.extend_from_slice(&[a, b, c]);
      indices.extend_from_slice(&[b, d, c]);
    } else {
      indices.extend_from_slice(&[a, c, b]);
      indices.extend_from_slice(&[b, c, d]);
    }
  }
}

pub fn stitch_apex_to_row(
  apex_idx: usize,
  row_base_idx: usize,
  row_count: usize,
  v_closed: bool,
  apex_is_first: bool,
  flip: bool,
  indices: &mut Vec<u32>,
) {
  let wrap_count = if v_closed {
    row_count
  } else {
    row_count.saturating_sub(1)
  };

  let apex = apex_idx as u32;

  for j in 0..wrap_count {
    let b = (row_base_idx + j) as u32;
    let c = (row_base_idx + (j + 1) % row_count) as u32;

    if apex_is_first {
      if flip {
        indices.extend_from_slice(&[apex, c, b]);
      } else {
        indices.extend_from_slice(&[apex, b, c]);
      }
    } else {
      if flip {
        indices.extend_from_slice(&[b, apex, c]);
      } else {
        indices.extend_from_slice(&[b, c, apex]);
      }
    }
  }
}

pub fn should_use_fku(enable_fku: bool, count_a: usize, count_b: usize) -> bool {
  if !enable_fku {
    return false;
  }

  // The time and space complexity of DP stitching is O(N*M), so we have to cut it off at some point
  // to prevent crashes, OOM, or other unexpected behavior with very high-resolution rings
  if count_a > MAX_DP_STITCH_RESOLUTION || count_b > MAX_DP_STITCH_RESOLUTION {
    return false;
  }

  true
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_dp_stitch_solve_basic_open() {
    // Two identical strips (open) should produce a simple 1:1 stitching
    let pts_a = vec![
      Vec3::new(1., 0., 0.),
      Vec3::new(0., 1., 0.),
      Vec3::new(-1., 0., 0.),
      Vec3::new(0., -1., 0.),
    ];
    let pts_b = pts_a.clone();

    let moves = dp_stitch_solve::<false>(&pts_a, &pts_b);
    // For open strips: should have n + m moves total
    assert_eq!(moves.len(), 8);
  }

  #[test]
  fn test_dp_stitch_solve_basic_closed() {
    // Two identical rings (closed) should produce stitching with wrap-around
    let pts_a = vec![
      Vec3::new(1., 0., 0.),
      Vec3::new(0., 1., 0.),
      Vec3::new(-1., 0., 0.),
      Vec3::new(0., -1., 0.),
    ];
    let pts_b = pts_a.clone();

    let offset = find_best_ring_alignment(&pts_a, &pts_b);
    assert_eq!(offset, 0); // Should be aligned already

    let rotated_b = rotate_ring(&pts_b, offset);
    let moves = dp_stitch_solve::<true>(&pts_a, &rotated_b);
    // For closed rings: should have (n+1) + (m+1) moves total (includes wrap-around)
    assert_eq!(moves.len(), 10);
  }

  #[test]
  fn test_find_best_ring_alignment() {
    let pts_a = vec![
      Vec3::new(1., 0., 0.),
      Vec3::new(0., 1., 0.),
      Vec3::new(-1., 0., 0.),
      Vec3::new(0., -1., 0.),
    ];

    // Ring B is rotated by 2 positions
    let pts_b = vec![
      Vec3::new(-1., 0., 0.),
      Vec3::new(0., -1., 0.),
      Vec3::new(1., 0., 0.),
      Vec3::new(0., 1., 0.),
    ];

    let offset = find_best_ring_alignment(&pts_a, &pts_b);
    // The closest vertex in B to A[0]=(1,0,0) is B[2]=(1,0,0), so offset should be 2
    assert_eq!(offset, 2);
  }

  #[test]
  fn test_snap_critical_points() {
    // Points that are close together should be merged
    let points = vec![0.0, 0.001, 0.5, 0.501, 1.0];
    let snapped = snap_critical_points(&[], &points, 100);
    // With resolution 100, snap_epsilon = 0.005
    // 0.0 and 0.001 should merge, 0.5 and 0.501 should merge, 1.0 is excluded for open rings
    assert_eq!(snapped.len(), 2);
  }

  #[test]
  fn test_snap_critical_points_prefers_critical_over_base() {
    let base_samples = vec![0.0, 0.1, 0.2, 0.3];
    let critical_points = vec![0.200004];
    let snapped = snap_critical_points(&base_samples, &critical_points, 10);

    assert_eq!(snapped.len(), 4);
    assert!(snapped.iter().any(|v| (*v - 0.0).abs() < 1e-6));
    assert!(snapped.iter().any(|v| (*v - 0.1).abs() < 1e-6));
    assert!(snapped.iter().any(|v| (*v - 0.3).abs() < 1e-6));
    assert!(snapped.iter().any(|v| (*v - 0.200004).abs() < 1e-6));
    assert!(!snapped.iter().any(|v| (*v - 0.2).abs() < 1e-6));
  }

  #[test]
  fn test_uniform_stitch_rows() {
    let mut indices = Vec::new();
    uniform_stitch_rows(0, 4, 4, true, false, &mut indices);
    // 4 quads = 8 triangles = 24 indices
    assert_eq!(indices.len(), 24);
  }

  #[test]
  fn test_fku_stitch_repro_3() {
    // These were extracted using debug logs from an actual failure case.  I don't want to pollute
    // the source code with hundreds of lines of them, so they're in separate files.
    let ring0_pts: Vec<Vec3> = include!("test_data/ring0_pts.rs");
    let ring1_pts: Vec<Vec3> = include!("test_data/ring1_pts.rs");

    let mut verts = Vec::new();
    let ring_a_start = verts.len();
    verts.extend(ring0_pts.iter().copied());
    let ring_b_start = verts.len();
    verts.extend(ring1_pts.iter().copied());

    let mut indices = Vec::new();
    dp_stitch_presampled(
      &ring0_pts,
      &ring1_pts,
      ring_a_start,
      ring_b_start,
      true,
      &mut indices,
    );

    // a proper stitch will:
    // - use every edge in ring0 exactly once in a triangle with its tip in ring1
    // - use every edge in ring1 exactly once in a triangle with its tip in ring0

    let n = ring0_pts.len();
    let m = ring1_pts.len();

    let mut ring0_edges_used = vec![false; n];
    let mut ring1_edges_used = vec![false; m];

    for tri in indices.chunks(3) {
      let [a, b, c] = std::array::from_fn(|i| tri[i] as usize);
      let [a_ring, b_ring, c_ring] =
        std::array::from_fn(|i| if (tri[i] as usize) < n { 0 } else { 1 });

      for &(v0, v1, r0, r1) in &[
        (a, b, a_ring, b_ring),
        (b, c, b_ring, c_ring),
        (c, a, c_ring, a_ring),
      ] {
        if r0 == r1 {
          let (ring_idx, ring_size, used) = if r0 == 0 {
            (v0, n, &mut ring0_edges_used)
          } else {
            (v0 - n, m, &mut ring1_edges_used)
          };
          let other_idx = if r0 == 0 { v1 } else { v1 - n };

          let edge_idx = if (ring_idx + 1) % ring_size == other_idx {
            Some(ring_idx)
          } else if (other_idx + 1) % ring_size == ring_idx {
            Some(other_idx)
          } else {
            None
          };

          if let Some(idx) = edge_idx {
            used[idx] = true;
          }
        }
      }
    }

    let missing_ring0: Vec<_> = ring0_edges_used
      .iter()
      .enumerate()
      .filter(|(_, &used)| !used)
      .map(|(i, _)| i)
      .collect();

    let missing_ring1: Vec<_> = ring1_edges_used
      .iter()
      .enumerate()
      .filter(|(_, &used)| !used)
      .map(|(i, _)| i)
      .collect();

    assert!(
      missing_ring0.is_empty() && missing_ring1.is_empty(),
      "bad stitch: ring0 missing edges: {:?}; ring1 missing edges: {:?}",
      missing_ring0,
      missing_ring1
    );
  }
}
