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
use wide::f32x4;

/// Maximum ring/row resolution for DP-based stitching.  Beyond this, we fall back to uniform
/// stitching.  The binding constraint is the backtracking bitmap, the one structure that stays
/// O(N*M) rather than O(N * band); at one bit per cell it reaches 50MB here, and the scratch
/// keeps a session's high-water mark.
pub const MAX_DP_STITCH_RESOLUTION: usize = 20_000;

const AREA_WEIGHT: f32 = 0.85;
const EDGE_LEN_WEIGHT: f32 = 1.;

/// Weight for the t-value difference penalty. This encourages stitching together vertices with
/// similar t-values along the spine. This discourages large fans from getting created when not
/// necessary, which helps avoid large jumps in dihedral angles between triangles which can cause
/// shading artifacts.
const DT_WEIGHT: f32 = 2.5;

/// Cost multiplier applied when both endpoints of the connecting edge are critical points.
/// This biases the stitching to connect critical-to-critical vertices (e.g. sharp seam points)
/// rather than taking shortcuts across seams.
const CRITICAL_PAIR_MULTIPLIER: f32 = 0.5;

/// Cost function for DP stitching.
///
/// - `p1`, `p2`: The two vertices on the ring that is advancing (the "segment" being added)
/// - `p3`: The vertex on the opposite ring
/// - `inv_scale`, `inv_scale_sq`: Precomputed 1/scale and 1/scale^2 where scale is the
///   characteristic size of the ring pair (e.g. average radius).
/// - `t2`, `t3`: Parametric t-values for `p2` and `p3` respectively. When not available, callers
///   should pass 0.0 for both (making the dt term zero).
/// - `both_critical`: When true, both `p2` and `p3` are critical points (e.g. sharp seam vertices).
///   The cost is multiplied by `CRITICAL_PAIR_MULTIPLIER` to bias stitching towards connecting
///   critical vertices to each other.
#[inline]
pub fn dp_stitch_cost(
  p1: Vec3,
  p2: Vec3,
  p3: Vec3,
  inv_scale: f32,
  inv_scale_sq: f32,
  t2: f32,
  t3: f32,
  both_critical: bool,
) -> f32 {
  let edge1 = p2 - p1;
  let edge2 = p3 - p1;
  let area = edge1.cross(&edge2).norm() * 0.5;

  let connecting_edge = p3 - p2;
  let edge_len = connecting_edge.norm();

  let mut dt = (t2 - t3).abs();
  if dt > 0.5 {
    // Wrap around for closed loops
    dt = 1.0 - dt;
  }

  let cost =
    AREA_WEIGHT * area * inv_scale_sq + EDGE_LEN_WEIGHT * edge_len * inv_scale + DT_WEIGHT * dt;

  if both_critical {
    cost * CRITICAL_PAIR_MULTIPLIER
  } else {
    cost
  }
}

/// Computes the average distance of a set of points from their centroid.
/// Used as a characteristic scale for non-dimensionalizing the DP cost function.
fn ring_average_radius(pts: &[Vec3]) -> f32 {
  if pts.is_empty() {
    return 0.;
  }

  let n = pts.len() as f32;
  let centroid = pts.iter().copied().sum::<Vec3>() / n;
  pts.iter().map(|p| (*p - centroid).norm()).sum::<f32>() / n
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

/// Backtracking moves, one bit per DP cell (0 = `AdvanceA`, 1 = `AdvanceB`), rows padded to
/// whole `u64` words so the scan can accumulate a row's bits in a register.
#[derive(Default)]
struct MoveBits {
  words: Vec<u64>,
  row_words: usize,
}

impl MoveBits {
  /// Only cells inside the scanned band are ever read back, so the buffer needs sizing but
  /// not clearing.
  fn reset(&mut self, rows: usize, cols: usize) {
    self.row_words = cols.div_ceil(64);
    let len = rows * self.row_words;
    if self.words.len() < len {
      self.words = Vec::new();
      self.words.resize(len, 0);
    }
  }

  #[inline]
  fn get(&self, row: usize, col: usize) -> DpMove {
    let w = self.words[row * self.row_words + (col >> 6)];
    if (w >> (col & 63)) & 1 == 0 {
      DpMove::AdvanceA
    } else {
      DpMove::AdvanceB
    }
  }
}

/// Ring B in SoA form, padded so the cost kernel can run whole 4-lane chunks past the end.
#[derive(Default)]
struct SoaRing {
  x: Vec<f32>,
  y: Vec<f32>,
  z: Vec<f32>,
  t: Vec<f32>,
  /// `CRITICAL_PAIR_MULTIPLIER` at critical vertices, `1.` elsewhere.
  crit_mul: Vec<f32>,
}

impl SoaRing {
  fn fill(&mut self, pts: &[Vec3], ts: Option<&[f32]>, crit: Option<&BitSlice>, wrap: bool) {
    let n = pts.len() + wrap as usize;
    let cap = n.next_multiple_of(4) + 4;
    for v in [
      &mut self.x,
      &mut self.y,
      &mut self.z,
      &mut self.t,
      &mut self.crit_mul,
    ] {
      v.clear();
      v.reserve(cap);
    }

    for i in 0..n {
      let src = if i == pts.len() { 0 } else { i };
      let p = pts[src];
      self.x.push(p.x);
      self.y.push(p.y);
      self.z.push(p.z);
      self.t.push(ts.map_or(0., |ts| ts[src]));
      self.crit_mul.push(if crit.is_some_and(|c| c[src]) {
        CRITICAL_PAIR_MULTIPLIER
      } else {
        1.
      });
    }

    for v in [&mut self.x, &mut self.y, &mut self.z, &mut self.t] {
      v.resize(cap, 0.);
    }
    self.crit_mul.resize(cap, 1.);
  }
}

#[inline]
fn load4(s: &[f32], i: usize) -> f32x4 {
  f32x4::from(<[f32; 4]>::try_from(&s[i..i + 4]).unwrap())
}

type V3x4 = (f32x4, f32x4, f32x4);

#[inline]
fn cross_norm4(a: V3x4, b: V3x4) -> f32x4 {
  let cx = a.1 * b.2 - a.2 * b.1;
  let cy = a.2 * b.0 - a.0 * b.2;
  let cz = a.0 * b.1 - a.1 * b.0;
  (cx * cx + cy * cy + cz * cz).sqrt()
}

/// Fills one DP row's two cost arrays, four columns at a time.  Both candidates at a cell close
/// the same connecting edge, so its length, the t penalty and the critical multiplier are shared
/// and only the areas differ.  Column `j` lands at `ca[l]`/`cb[l]` for `j = base + 2 + l`.
#[allow(clippy::too_many_arguments)]
fn fill_row_costs<const CRIT: bool>(
  b: &SoaRing,
  base: usize,
  ap: Vec3,
  ac: Vec3,
  ta: f32,
  ka: f32,
  ke: f32,
  ca: &mut [f32],
  cb: &mut [f32],
) {
  debug_assert_eq!(ca.len(), cb.len());
  debug_assert_eq!(ca.len() % 4, 0, "ragged tail would be dropped silently");

  let ea = ac - ap;
  let eav = (f32x4::splat(ea.x), f32x4::splat(ea.y), f32x4::splat(ea.z));
  let apv = (f32x4::splat(ap.x), f32x4::splat(ap.y), f32x4::splat(ap.z));
  let acv = (f32x4::splat(ac.x), f32x4::splat(ac.y), f32x4::splat(ac.z));
  let (tav, kav, kev) = (f32x4::splat(ta), f32x4::splat(ka), f32x4::splat(ke));
  let (one, dtw) = (f32x4::splat(1.), f32x4::splat(DT_WEIGHT));

  let iter = ca.chunks_exact_mut(4).zip(cb.chunks_exact_mut(4));
  for (l, (oa, ob)) in iter.enumerate() {
    let i = base + l * 4;
    let prev = (load4(&b.x, i), load4(&b.y, i), load4(&b.z, i));
    let cur = (load4(&b.x, i + 1), load4(&b.y, i + 1), load4(&b.z, i + 1));

    let d = (cur.0 - acv.0, cur.1 - acv.1, cur.2 - acv.2);
    let edge = kev * (d.0 * d.0 + d.1 * d.1 + d.2 * d.2).sqrt();
    let dt = (tav - load4(&b.t, i + 1)).abs();
    let dt = dtw * dt.min(one - dt);

    let area_a = cross_norm4(eav, (cur.0 - apv.0, cur.1 - apv.1, cur.2 - apv.2));
    let eb = (cur.0 - prev.0, cur.1 - prev.1, cur.2 - prev.2);
    let area_b = cross_norm4(eb, (acv.0 - prev.0, acv.1 - prev.1, acv.2 - prev.2));

    let mul = if CRIT {
      load4(&b.crit_mul, i + 1)
    } else {
      f32x4::splat(1.)
    };
    oa.copy_from_slice(&(((kav * area_a + edge) + dt) * mul).to_array());
    ob.copy_from_slice(&(((kav * area_b + edge) + dt) * mul).to_array());
  }
}

/// The `j == 1` column, which the vectorized window can't cover: its `B[j-2]` is out of range.
fn cost_advance_a_at_b0(b: &SoaRing, ap: Vec3, ac: Vec3, ta: f32, ka: f32, ke: f32) -> f32 {
  let b0 = Vec3::new(b.x[0], b.y[0], b.z[0]);
  let dt = (ta - b.t[0]).abs();
  ka * (ac - ap).cross(&(b0 - ap)).norm() + ke * (b0 - ac).norm() + DT_WEIGHT * dt.min(1. - dt)
}

#[derive(Default)]
struct DpScratch {
  b: SoaRing,
  ca: Vec<f32>,
  cb: Vec<f32>,
  prev: Vec<f32>,
  cur: Vec<f32>,
  moves: MoveBits,
}

thread_local! {
  static SCRATCH: std::cell::RefCell<DpScratch> = std::cell::RefCell::new(DpScratch::default());
}

/// Number of evenly-spaced arc-length samples used during ring alignment cross-correlation.
///
/// K=64 captures enough geometric structure (corners, curves) to distinguish orientations
/// without being expensive. Cross-correlation is O(K²) = ~4096 operations.
const ALIGNMENT_RESAMPLE_K: usize = 64;

/// Computes cumulative arc lengths for a closed ring.
///
/// Returns a vector of length `pts.len() + 1` where entry `i` is the total
/// distance from `pts[0]` to `pts[i]` along the ring edges. The final entry
/// is the full perimeter length.
fn cumulative_arc_lengths(pts: &[Vec3]) -> Vec<f32> {
  let n = pts.len();
  let mut lens = Vec::with_capacity(n + 1);
  lens.push(0.0f32);
  let mut total = 0.0f32;
  for i in 0..n {
    total += (pts[(i + 1) % n] - pts[i]).norm();
    lens.push(total);
  }
  lens
}

// TODO: this duplicates some functionality from path_sampler; maybe we could re-use?
/// Samples the ring at normalized arc-length parameter `t` in [0, 1) by linearly
/// interpolating between adjacent vertices.
fn sample_ring_at(pts: &[Vec3], lens: &[f32], total_len: f32, t: f32) -> Vec3 {
  let target = t * total_len;

  if target <= 0.0 {
    return pts[0];
  }
  let idx = match lens.binary_search_by(|v| v.partial_cmp(&target).unwrap()) {
    Ok(i) => i.min(pts.len() - 1),
    Err(i) => (i - 1).min(pts.len() - 1),
  };

  let p0 = pts[idx];
  let p1 = pts[(idx + 1) % pts.len()];
  let seg_len = lens[idx + 1] - lens[idx];
  if seg_len < 1e-9 {
    return p0;
  }
  let alpha = (target - lens[idx]) / seg_len;
  p0.lerp(&p1, alpha)
}

/// Resamples a ring into `count` uniformly arc-length-spaced points.
fn resample_ring(pts: &[Vec3], count: usize) -> Vec<Vec3> {
  let cum = cumulative_arc_lengths(pts);
  let total_len = *cum.last().unwrap();
  (0..count)
    .map(|i| {
      let t = i as f32 / count as f32;
      sample_ring_at(pts, &cum, total_len, t)
    })
    .collect()
}

/// Find the best starting offset for ring B to minimize twist/misalignment with ring A.
///
/// Uses arc-length resampling + cyclic cross-correlation to compare the full shape of
/// both rings simultaneously.  This is robust to differences in vertex count and
/// non-uniform vertex density — problems that break single-vertex or index-scaled
/// approaches.
///
/// Algorithm:
/// 1. Resample both rings to K uniformly arc-length-spaced points.
/// 2. Try all K cyclic shifts of the resampled B ring; pick the shift that minimizes the sum of
///    squared distances to the resampled A ring.
/// 3. Map the winning normalized shift back to the nearest actual vertex index in pts_b.
pub fn find_best_ring_alignment(pts_a: &[Vec3], pts_b: &[Vec3]) -> usize {
  if pts_a.is_empty() || pts_b.is_empty() {
    return 0;
  }

  let k = ALIGNMENT_RESAMPLE_K;
  let res_a = resample_ring(pts_a, k);
  let res_b = resample_ring(pts_b, k);

  // Find the cyclic shift of res_b that best matches res_a.
  let mut best_shift = 0usize;
  let mut best_error = f32::MAX;
  for shift in 0..k {
    let mut error = 0.0f32;
    for i in 0..k {
      error += (res_a[i] - res_b[(i + shift) % k]).norm_squared();
    }
    if error < best_error {
      best_error = error;
      best_shift = shift;
    }
  }

  // Convert the winning normalized shift (best_shift / K) to the nearest actual
  // vertex index in pts_b using its arc-length parameterization.
  let best_t = best_shift as f32 / k as f32;
  let cum_b = cumulative_arc_lengths(pts_b);
  let total_len_b = *cum_b.last().unwrap();

  let mut best_real_idx = 0usize;
  let mut best_diff = f32::MAX;
  for (i, &d) in cum_b.iter().take(pts_b.len()).enumerate() {
    let t = if total_len_b > 1e-9 {
      d / total_len_b
    } else {
      0.0
    };
    let diff = (t - best_t).abs();
    let cyclic_diff = diff.min(1.0 - diff);
    if cyclic_diff < best_diff {
      best_diff = cyclic_diff;
      best_real_idx = i;
    }
  }

  best_real_idx
}

/// Above this many DP cells the exact solve is replaced by a coarse guide pass on subsampled
/// rings plus a banded refinement around the upscaled coarse path.
const MULTISCALE_CELL_LIMIT: usize = 16_384;
/// Ring subsampling factor for the coarse guide pass.
const COARSE_FACTOR: usize = 4;
/// Fine-grid half-width first tried around the upscaled coarse path: three coarse steps of
/// slack, enough for the guide's own resolution error.
const BAND_RADIUS: usize = 3 * COARSE_FACTOR;
/// Multiplier applied to the band radius each time the solved path proves the band was binding.
const BAND_WIDEN_FACTOR: u32 = 4;

/// Per-row inclusive column bounds of a banded DP.  `lo` and `hi` are both non-decreasing and
/// satisfy `lo[r] <= hi[r - 1]`, which is what guarantees a monotone staircase path from
/// `(0, 0)` to `(table_n, table_m)` exists inside the band.
type Band = [(u32, u32)];

/// `count` is clamped to the source length: sampling past it would emit duplicate points, and
/// the zero-length edges that follow make the guide meaningless.
fn subsample<T: Copy>(src: &[T], count: usize) -> Vec<T> {
  let count = count.min(src.len());
  (0..count).map(|i| src[i * src.len() / count]).collect()
}

/// The ring pair and its per-vertex metadata: every stage of the solve needs all of it, and
/// `inv_scale`/`inv_scale_sq` non-dimensionalize the cost function by the pair's average radius.
#[derive(Clone, Copy)]
pub struct Rings<'a> {
  pub a: &'a [Vec3],
  pub b: &'a [Vec3],
  pub ta: Option<&'a [f32]>,
  pub tb: Option<&'a [f32]>,
  pub crit_a: Option<&'a BitSlice>,
  pub crit_b: Option<&'a BitSlice>,
  pub inv_scale: f32,
  pub inv_scale_sq: f32,
}

/// Per-fine-row column span of the coarse guide path, `u32::MAX` in `lo` marking rows the
/// upscaled path doesn't land on.
struct Anchors {
  lo: Vec<u32>,
  hi: Vec<u32>,
}

/// Solves the stitch on subsampled rings and maps the resulting path onto the full grid.
fn coarse_anchors<const CLOSED: bool>(r: Rings, table_n: usize, table_m: usize) -> Anchors {
  let (pts_a, pts_b) = (r.a, r.b);
  let ka = (pts_a.len() / COARSE_FACTOR).max(4).min(pts_a.len());
  let kb = (pts_b.len() / COARSE_FACTOR).max(4).min(pts_b.len());
  let (mut ca_pts, mut cb_pts) = (subsample(pts_a, ka), subsample(pts_b, kb));
  let cta = r.ta.map(|t| subsample(t, ka));
  let ctb = r.tb.map(|t| subsample(t, kb));
  // Flags are OR-ed over each bucket rather than point-sampled, or the seam vertices
  // `CRITICAL_PAIR_MULTIPLIER` exists to capture would mostly be dropped.  The sampled position
  // moves to the flagged vertex too, so a coarse point's flag and position describe one vertex.
  let sub_crit = |pts: &mut [Vec3], src: &[Vec3], c: Option<&BitSlice>| {
    c.map(|c| {
      let k = pts.len();
      let mut out = bitvec![0; k];
      for i in 0..k {
        let lo = i * c.len() / k;
        let hi = ((i + 1) * c.len() / k).max(lo + 1).min(c.len());
        if let Some(ix) = (lo..hi).find(|&j| c[j]) {
          out.set(i, true);
          pts[i] = src[ix];
        }
      }
      out
    })
  };
  let cca = sub_crit(&mut ca_pts, pts_a, r.crit_a);
  let ccb = sub_crit(&mut cb_pts, pts_b, r.crit_b);

  let coarse = dp_stitch_solve::<CLOSED>(Rings {
    a: &ca_pts,
    b: &cb_pts,
    ta: cta.as_deref(),
    tb: ctb.as_deref(),
    crit_a: cca.as_deref(),
    crit_b: ccb.as_deref(),
    ..r
  });

  let (ka, kb) = (ca_pts.len(), cb_pts.len());
  let (ctn, ctm) = if CLOSED { (ka + 1, kb + 1) } else { (ka, kb) };
  let map_row = |k: usize| (k * table_n + ctn / 2) / ctn;
  let map_col = |l: usize| (l * table_m + ctm / 2) / ctm;

  let mut anchors = Anchors {
    lo: vec![u32::MAX; table_n + 1],
    hi: vec![0u32; table_n + 1],
  };
  for (k, l) in coarse
    .map(|(k, l, _)| (k, l))
    .chain(std::iter::once((0, 0)))
  {
    let (r, c) = (map_row(k), map_col(l) as u32);
    anchors.lo[r] = anchors.lo[r].min(c);
    anchors.hi[r] = anchors.hi[r].max(c);
  }
  anchors
}

/// Widens the guide path into a band `radius` columns to either side.  `hi[r]` comes from the
/// nearest anchor at or after `r` and `lo[r]` from the nearest at or before it, so between two
/// anchors the band spans the columns they bracket.
fn band_from_anchors(a: &Anchors, radius: u32, table_n: usize, table_m: usize) -> Vec<(u32, u32)> {
  let mut band = vec![(0u32, 0u32); table_n + 1];
  let mut next_hi = 0u32;
  for r in (0..=table_n).rev() {
    if a.lo[r] != u32::MAX {
      next_hi = a.hi[r];
    }
    band[r].1 = next_hi.saturating_add(radius).min(table_m as u32);
  }
  let mut last_lo = 0u32;
  for r in 0..=table_n {
    if a.lo[r] != u32::MAX {
      last_lo = a.lo[r];
    }
    band[r].0 = last_lo.saturating_sub(radius);
    if r > 0 {
      band[r].0 = band[r].0.min(band[r - 1].1);
    }
  }
  band
}

/// Whether the path pressed on a band edge that was actually restricting it (edges clamped to
/// the grid restrict nothing).  A heuristic, not a proof: a better path lying wholly outside the
/// band leaves the banded one strictly interior and goes undetected.
fn path_hit_band_edge(moves: &[(usize, usize, DpMove)], band: &Band, table_m: usize) -> bool {
  moves.iter().any(|&(i, j, _)| {
    let (lo, hi) = band[i];
    (j as u32 == lo && lo > 0) || (j as u32 == hi && (hi as usize) < table_m)
  })
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
/// Note: For closed rings, pts_b should be pre-rotated using `rotate_ring` and
/// `find_best_ring_alignment` before calling this function.
pub fn dp_stitch_solve<const CLOSED: bool>(
  r: Rings,
) -> std::iter::Rev<<Vec<(usize, usize, DpMove)> as IntoIterator>::IntoIter> {
  let (n, m) = (r.a.len(), r.b.len());
  if n == 0 || m == 0 {
    return Vec::new().into_iter().rev();
  }

  // For closed rings we extend the table by 1 to handle wrap-around: state (n, m) means the
  // loop has been completed, with vertex index n/m wrapping back to 0.
  let table_n = if CLOSED { n + 1 } else { n };
  let table_m = if CLOSED { m + 1 } else { m };

  // Below the limit the exact solve is already cheap; a band as wide as the grid would have
  // nothing to constrain.  Either way the guide would be wasted work.
  let cells = (table_n as u64 + 1) * (table_m as u64 + 1);
  if cells <= MULTISCALE_CELL_LIMIT as u64 || BAND_RADIUS >= table_m {
    return solve_banded::<CLOSED>(r, None).into_iter().rev();
  }

  // The guide recurses back through here, so it has to finish before `solve_banded` takes the
  // shared scratch buffers; nesting the two would double-borrow it.
  let anchors = coarse_anchors::<CLOSED>(r, table_n, table_m);

  // A row advances `table_m / table_n` columns on average, so a guide error of a few coarse rows
  // is that many times as wide in columns; a fixed column radius collapses on lopsided pairs.
  // Start there and widen only when the solved path proves the band was binding.
  let slope = (table_m / table_n).max(1);
  let mut radius = (BAND_RADIUS * slope).min(table_m) as u32;
  loop {
    let full = radius as usize >= table_m;
    let band = band_from_anchors(&anchors, radius, table_n, table_m);
    let moves = solve_banded::<CLOSED>(r, Some(&band));
    if full || !path_hit_band_edge(&moves, &band, table_m) {
      return moves.into_iter().rev();
    }
    radius = radius.saturating_mul(BAND_WIDEN_FACTOR);
  }
}

fn solve_banded<const CLOSED: bool>(r: Rings, band: Option<&Band>) -> Vec<(usize, usize, DpMove)> {
  let (pts_a, ts_a, crit_a) = (r.a, r.ta, r.crit_a);
  let (n, m) = (r.a.len(), r.b.len());
  let table_n = if CLOSED { n + 1 } else { n };
  let table_m = if CLOSED { m + 1 } else { m };

  let get_a = |i: usize| -> Vec3 { pts_a[if CLOSED && i == n { 0 } else { i }] };
  let get_ta = |i: usize| -> f32 { ts_a.map_or(0., |ts| ts[if CLOSED && i == n { 0 } else { i }]) };
  let is_crit_a =
    |i: usize| -> bool { crit_a.is_some_and(|c| c[if CLOSED && i == n { 0 } else { i }]) };
  let bounds = |i: usize| -> (usize, usize) {
    band.map_or((0, table_m), |b| (b[i].0 as usize, b[i].1 as usize))
  };
  // A monotone staircase from (0,0) to (table_n, table_m) has to exist inside the band, and the
  // row scan relies on never revisiting a column the previous row didn't reach.
  debug_assert!(band.is_none_or(|b| {
    b.len() == table_n + 1
      && b[0].0 == 0
      && b[table_n].1 as usize == table_m
      && b
        .windows(2)
        .all(|w| w[0].0 <= w[1].0 && w[0].1 <= w[1].1 && w[1].0 <= w[0].1)
  }));

  // Fold the cost function's constant factors into two multipliers.  `ka` absorbs the 0.5 that
  // turns the cross-product norm into a triangle area.
  let ka = AREA_WEIGHT * 0.5 * r.inv_scale_sq;
  let ke = EDGE_LEN_WEIGHT * r.inv_scale;

  SCRATCH.with(|scratch| {
    let scratch = &mut *scratch.borrow_mut();
    scratch.b.fill(r.b, r.tb, r.crit_b, CLOSED);
    let b = &scratch.b;

    let pad = table_m.next_multiple_of(4);
    scratch.ca.clear();
    scratch.ca.resize(pad + 2, 0.);
    scratch.cb.clear();
    scratch.cb.resize(pad + 2, 0.);
    scratch.prev.clear();
    scratch.prev.resize(table_m + 1, f32::INFINITY);
    scratch.cur.clear();
    scratch.cur.resize(table_m + 1, f32::INFINITY);
    scratch.moves.reset(table_n + 1, table_m + 1);

    let ca = &mut scratch.ca;
    let cb = &mut scratch.cb;
    let row_words = scratch.moves.row_words;
    let words = &mut scratch.moves.words;

    let a0 = get_a(0);
    let b0 = Vec3::new(b.x[0], b.y[0], b.z[0]);
    let e0 = ke * (a0 - b0).norm();
    let (_, hi0) = bounds(0);

    // Row 0 doubles as the i=1 row's B-advance costs: both are the triangle (B[j-2], B[j-1], A[0]).
    // The A-advance half is discarded here; row 1 has no A-advance cost at all.
    let row0 = if is_crit_a(0) {
      fill_row_costs::<true>
    } else {
      fill_row_costs::<false>
    };
    row0(
      b,
      0,
      a0,
      a0,
      get_ta(0),
      ka,
      ke,
      &mut ca[2..pad + 2],
      &mut cb[2..pad + 2],
    );

    let prev = &mut scratch.prev;
    prev[0] = 0.;
    let mut acc = e0;
    for j in 1..=hi0 {
      if j > 1 {
        acc += cb[j];
      }
      prev[j] = acc;
    }
    // Backtracking through row 0 can only advance on B.
    words[..row_words].fill(u64::MAX);
    words[0] &= !1;

    let cur = &mut scratch.cur;
    let mut col0 = e0;
    let mut prev_hi = hi0;

    for i in 1..=table_n {
      let ac = get_a(i - 1);
      let ta = get_ta(i - 1);
      let (lo, hi) = bounds(i);
      let jstart = lo.max(1);

      // Columns the previous row never reached are unreachable from it.
      for slot in &mut prev[(prev_hi + 1)..=hi] {
        *slot = f32::INFINITY;
      }
      prev_hi = hi;

      // The vectorized window covers columns `k0 + 2 ..= k1 + 1`; `j == 1` needs `B[-1]` and is
      // filled by hand below.
      let k0 = (jstart.max(2) - 2) & !3;
      let k1 = (hi.max(1) - 1).next_multiple_of(4).min(pad);
      if i == 1 {
        ca[jstart..=hi].fill(0.);
      } else {
        let ap = get_a(i - 2);
        let crit = is_crit_a(i - 1);
        let fill = if crit {
          fill_row_costs::<true>
        } else {
          fill_row_costs::<false>
        };
        fill(
          b,
          k0,
          ap,
          ac,
          ta,
          ka,
          ke,
          &mut ca[k0 + 2..k1 + 2],
          &mut cb[k0 + 2..k1 + 2],
        );
        if jstart == 1 {
          let c = cost_advance_a_at_b0(b, ap, ac, ta, ka, ke);
          ca[1] = if crit { c * b.crit_mul[0] } else { c };
        }
        // Column 0's step closes the same triangle as column 1's A-advance, so it reuses that
        // value rather than re-deriving it and rounding differently.
        if lo == 0 {
          col0 += ca[1];
        }
      }

      // Min-plus scan along the row.  The A-advance term reads the previous row, the B-advance
      // term the cell just written, so only this cheap step is serially dependent.
      let base = i * row_words;
      let mut running = if lo == 0 { col0 } else { f32::INFINITY };
      let mut word = 0u64;
      for j in jstart..=hi {
        let va = prev[j] + ca[j];
        let vb = running + cb[j];
        let take_b = vb < va;
        running = if take_b { vb } else { va };
        cur[j] = running;
        word |= (take_b as u64) << (j & 63);
        if j & 63 == 63 {
          words[base + (j >> 6)] = word;
          word = 0;
        }
      }
      if hi & 63 != 63 {
        words[base + (hi >> 6)] = word;
      }

      std::mem::swap(prev, cur);
    }

    let mut moves = Vec::with_capacity(table_n + table_m);
    let (mut i, mut j) = (table_n, table_m);
    while i > 0 || j > 0 {
      let came_from = scratch.moves.get(i, j);
      moves.push((i, j, came_from));
      // Row 0 records `AdvanceB` everywhere and column 0 `AdvanceA`, so a well-formed table
      // never steps off either edge.  The bitmap is reused without clearing, though, so clamp
      // rather than wrap: a stale bit should degrade the stitch, not trap.
      match came_from {
        DpMove::AdvanceA if i > 0 => i -= 1,
        DpMove::AdvanceB if j > 0 => j -= 1,
        _ => {
          debug_assert!(false, "backtrack stepped off the table at ({i}, {j})");
          break;
        }
      }
    }

    moves
  })
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
  for &[a, b] in base.array_windows::<2>() {
    let step = b - a;
    if step > 0. {
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
  ts_a: Option<&[f32]>,
  ts_b: Option<&[f32]>,
  crit_a: Option<&BitSlice>,
  crit_b: Option<&BitSlice>,
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

  // Compute characteristic scale from average ring radius for non-dimensionalization.
  // This makes the cost function weights behave consistently regardless of mesh size.
  let scale = ((ring_average_radius(pts_a) + ring_average_radius(pts_b)) * 0.5).max(1e-6);
  let inv_scale = 1. / scale;
  let inv_scale_sq = inv_scale * inv_scale;

  // Find best alignment for ring B (only matters for closed rings, but harmless for open)
  let b_offset = if closed {
    find_best_ring_alignment(pts_a, pts_b)
  } else {
    0
  };

  // Pre-rotate ring B positions and t-values to avoid modulo ops in the DP solver.
  let rotated_pts_b = rotate_ring(pts_b, b_offset);
  let rotated_ts_b = ts_b.map(|ts: &[f32]| {
    let m = ts.len();
    if b_offset == 0 || m == 0 {
      ts.to_vec()
    } else {
      // re-normalize t-values so the parametric origin aligns with the new spatial origin.  Without
      // this, the `DT_WEIGHT` penalty fights against the spatial alignment, causing the DP to
      // create large vertex fans as it tries to reconcile conflicting objectives.
      let t_shift = ts[b_offset % m];
      (0..m)
        .map(|i| {
          let t = ts[(i + b_offset) % m] - t_shift;
          if t < 0. {
            t + 1.
          } else {
            t
          }
        })
        .collect()
    }
  });

  // Pre-rotate critical mask for ring B to match the rotated positions/t-values.
  let rotated_crit_b = crit_b.map(|c| {
    let m = c.len();
    if b_offset == 0 || m == 0 {
      c.to_bitvec()
    } else {
      let mut rotated = bitvec![0; m];
      for i in 0..m {
        rotated.set(i, c[(i + b_offset) % m]);
      }
      rotated
    }
  });

  let solve_impl = if closed {
    dp_stitch_solve::<true>
  } else {
    dp_stitch_solve::<false>
  };
  let moves = solve_impl(Rings {
    a: pts_a,
    b: &rotated_pts_b,
    ta: ts_a,
    tb: rotated_ts_b.as_deref(),
    crit_a,
    crit_b: rotated_crit_b.as_deref(),
    inv_scale,
    inv_scale_sq,
  });

  // Map DP indices to actual vertex buffer indices.
  // Ring A: DP index i maps directly to vertex buffer (with wrap for closed rings)
  // Ring B: DP index j maps to rotated position, need to unrotate for vertex buffer
  let get_a_vtx_ix = |i: usize| -> u32 { (ring_a_base_idx + (i % n)) as u32 };
  let get_b_vtx_ix = |j: usize| -> u32 { (ring_b_base_idx + ((j + b_offset) % m)) as u32 };

  // Generate triangles from DP moves, collecting stats in the same pass.
  // For closed rings, the solver includes wrap-around moves, so we generate all
  // triangles including the seam (no manual closing needed).
  for (i, j, mv) in moves {
    if i == 0 && j == 0 {
      continue;
    }

    match mv {
      DpMove::AdvanceA => {
        // Triangle: (A[i-2], A[i-1], B[j-1]). Skip when i <= 1 (A[i-2] out of bounds). When j=0
        // the apex is B[0] — the bottom-edge fan of an open strip / the seam of a closed ring.
        if i > 1 {
          let idx_a_prev = get_a_vtx_ix(i - 2);
          let idx_a_curr = get_a_vtx_ix(i - 1);
          let b_idx_raw = if j == 0 { 0 } else { j - 1 };
          let idx_b = get_b_vtx_ix(b_idx_raw);
          out_indices.extend_from_slice(&[idx_a_prev, idx_a_curr, idx_b]);
        }
      }
      DpMove::AdvanceB => {
        // Triangle: (A[i-1], B[j-1], B[j-2]). Skip when j <= 1 (B[j-2] out of bounds). When i=0
        // the apex is A[0] — the left-edge fan of an open strip / the seam of a closed ring.
        if j > 1 {
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

  /// Re-evaluates the DP objective for a produced move sequence, independent of how the solver
  /// found it.
  #[allow(clippy::too_many_arguments)]
  fn score_moves(
    moves: &[(usize, usize, DpMove)],
    pts_a: &[Vec3],
    pts_b: &[Vec3],
    ts_a: Option<&[f32]>,
    ts_b: Option<&[f32]>,
    crit_a: Option<&BitSlice>,
    crit_b: Option<&BitSlice>,
    closed: bool,
    inv_scale: f32,
    inv_scale_sq: f32,
  ) -> f64 {
    let (n, m) = (pts_a.len(), pts_b.len());
    let wrap_a = |i: usize| if closed && i == n { 0 } else { i };
    let wrap_b = |j: usize| if closed && j == m { 0 } else { j };
    let ta = |i: usize| ts_a.map_or(0., |t| t[wrap_a(i)]);
    let tb = |j: usize| ts_b.map_or(0., |t| t[wrap_b(j)]);
    let ca = |i: usize| crit_a.is_some_and(|c| c[wrap_a(i)]);
    let cb = |j: usize| crit_b.is_some_and(|c| c[wrap_b(j)]);
    let e0 = EDGE_LEN_WEIGHT * (pts_a[0] - pts_b[0]).norm() * inv_scale;

    let mut total = 0f64;
    for &(i, j, mv) in moves {
      let c = match mv {
        DpMove::AdvanceA if i == 1 => {
          if j == 0 {
            e0
          } else {
            0.
          }
        }
        DpMove::AdvanceA => {
          let bj = if j == 0 { 0 } else { j - 1 };
          dp_stitch_cost(
            pts_a[wrap_a(i - 2)],
            pts_a[wrap_a(i - 1)],
            pts_b[wrap_b(bj)],
            inv_scale,
            inv_scale_sq,
            ta(i - 1),
            tb(bj),
            ca(i - 1) && cb(bj),
          )
        }
        DpMove::AdvanceB if j == 1 => {
          if i == 0 {
            e0
          } else {
            0.
          }
        }
        DpMove::AdvanceB => {
          let ai = if i == 0 { 0 } else { i - 1 };
          dp_stitch_cost(
            pts_b[wrap_b(j - 2)],
            pts_b[wrap_b(j - 1)],
            pts_a[wrap_a(ai)],
            inv_scale,
            inv_scale_sq,
            tb(j - 1),
            ta(ai),
            cb(j - 1) && ca(ai),
          )
        }
      };
      total += c as f64;
    }
    total
  }

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

    let moves = dp_stitch_solve::<false>(plain_rings(&pts_a, &pts_b));
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
    let moves = dp_stitch_solve::<true>(plain_rings(&pts_a, &rotated_b));
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
    // B is rotated by 2 positions from A; arc-length cross-correlation should recover this
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

  /// Deterministic lobed rings with per-vertex jitter, non-uniform spacing, and a relative
  /// twist -- the shapes `rail_sweep` actually feeds the solver, at sizes that trigger banding.
  fn gen_ring(seed: u32, count: usize, lobes: f32, radius: f32, z: f32, phase: f32) -> Vec<Vec3> {
    let mut rng = seed.wrapping_mul(0x9E37_79B9) | 1;
    let mut next = || {
      rng ^= rng << 13;
      rng ^= rng >> 17;
      rng ^= rng << 5;
      (rng >> 8) as f32 / (1 << 24) as f32
    };
    (0..count)
      .map(|i| {
        // squared parameter spacing makes vertex density vary around the ring
        let u = i as f32 / count as f32;
        let a = (u + 0.35 * u * (1. - u)) * std::f32::consts::TAU + phase;
        let r = radius * (1. + 0.35 * (a * lobes).sin() + 0.08 * (next() - 0.5));
        Vec3::new(r * a.cos(), r * a.sin(), z + 0.15 * (next() - 0.5))
      })
      .collect()
  }

  fn plain_rings<'a>(a: &'a [Vec3], b: &'a [Vec3]) -> Rings<'a> {
    Rings {
      a,
      b,
      ta: None,
      tb: None,
      crit_a: None,
      crit_b: None,
      inv_scale: 1.,
      inv_scale_sq: 1.,
    }
  }

  /// Marks every `stride`-th vertex critical, the shape `snap_critical_points` produces.
  fn crit_mask(n: usize, stride: usize) -> BitVec {
    let mut m = bitvec![0; n];
    for i in (0..n).step_by(stride) {
      m.set(i, true);
    }
    m
  }

  /// Runs one ring pair through both the production (banded) path and a full-width band that
  /// forces the exact DP through the same code, and returns `banded_cost / exact_cost`.
  fn banded_vs_exact<const CLOSED: bool>(
    pts_a: &[Vec3],
    pts_b: &[Vec3],
    crit_a: Option<&BitSlice>,
    crit_b_unrotated: Option<&BitSlice>,
  ) -> f64 {
    let scale = ((ring_average_radius(pts_a) + ring_average_radius(pts_b)) * 0.5).max(1e-6);
    let (inv, inv_sq) = (1. / scale, 1. / (scale * scale));
    let offset = if CLOSED {
      find_best_ring_alignment(pts_a, pts_b)
    } else {
      0
    };
    let rot_b = rotate_ring(pts_b, offset);
    let m = rot_b.len();
    let ts_a: Vec<f32> = (0..pts_a.len())
      .map(|i| i as f32 / pts_a.len() as f32)
      .collect();
    // Rotating ring B re-origins its t-values too, exactly as `dp_stitch_presampled` does; the
    // shift is read out of the source array rather than recomputed, so a non-uniform t
    // distribution stays faithful.
    let raw_tb: Vec<f32> = (0..m).map(|i| i as f32 / m as f32).collect();
    let t_shift = raw_tb[offset % m];
    let ts_b: Vec<f32> = (0..m)
      .map(|i| {
        let t = raw_tb[(i + offset) % m] - t_shift;
        if t < 0. {
          t + 1.
        } else {
          t
        }
      })
      .collect();
    let crit_b = crit_b_unrotated.map(|c| {
      let mut r = bitvec![0; m];
      for i in 0..m {
        r.set(i, c[(i + offset) % m]);
      }
      r
    });

    let rings = Rings {
      a: pts_a,
      b: &rot_b,
      ta: Some(&ts_a),
      tb: Some(&ts_b),
      crit_a,
      crit_b: crit_b.as_deref(),
      inv_scale: inv,
      inv_scale_sq: inv_sq,
    };
    let (table_n, table_m) = if CLOSED {
      (pts_a.len() + 1, m + 1)
    } else {
      (pts_a.len(), m)
    };
    let full = vec![(0, table_m as u32); table_n + 1];
    let score = |mv: &[(usize, usize, DpMove)]| {
      score_moves(
        mv,
        pts_a,
        &rot_b,
        Some(&ts_a),
        Some(&ts_b),
        crit_a,
        crit_b.as_deref(),
        CLOSED,
        inv,
        inv_sq,
      )
    };

    let banded: Vec<_> = dp_stitch_solve::<CLOSED>(rings).collect();
    let exact = solve_banded::<CLOSED>(rings, Some(&full));
    assert_monotone_path(&banded, table_n, table_m);
    score(&banded) / score(&exact)
  }

  /// The move list must be a contiguous monotone staircase from the origin to
  /// (table_n, table_m) -- one step per entry, never revisiting or skipping a state.
  fn assert_monotone_path(moves: &[(usize, usize, DpMove)], table_n: usize, table_m: usize) {
    assert_eq!(moves.len(), table_n + table_m, "path length");
    let (mut i, mut j) = (0usize, 0usize);
    for &(mi, mj, mv) in moves {
      match mv {
        DpMove::AdvanceA => i += 1,
        DpMove::AdvanceB => j += 1,
      }
      assert_eq!((mi, mj), (i, j), "path is not contiguous");
    }
    assert_eq!(
      (i, j),
      (table_n, table_m),
      "path does not reach the far corner"
    );
  }

  /// The banded multiscale solve is compared against the exact DP over the shapes that stress
  /// the guide: mismatched vertex counts, extreme aspect ratios (where a row advances thousands
  /// of columns), open strips, and rings whose lobe count approaches the coarse sampling rate.
  ///
  /// The bound is what the scheme achieves, not a claim of optimality: a subsampled guide cannot
  /// see structure finer than its own sample rate, so a ring whose lobe count nears that rate is
  /// where any residual slack shows up.
  #[test]
  fn test_banded_solve_matches_exact() {
    // rings whose lobe count nears the coarse sample rate keep a little slack; everything else
    // is expected to come out exactly optimal
    const PERIODIC_TOL: f64 = 1.002;
    // (ring A, ring B, lobes, closed, tolerated cost ratio)
    let cases: &[(usize, usize, f32, bool, f64)] = &[
      (150, 150, 3., true, 1.001),
      (400, 400, 3., true, 1.001),
      (400, 137, 3., true, 1.001),
      (850, 850, 3., true, 1.001),
      (850, 811, 3., true, 1.001),
      (601, 900, 3., true, 1.001),
      // lopsided pairs: the band is measured in columns, the error scales with columns-per-row
      (44, 812, 3., true, 1.001),
      (12, 16000, 3., true, 1.001),
      (2, 5000, 3., true, 1.001),
      (5000, 3, 3., true, 1.001),
      (200, 1600, 7., true, 1.001),
      // open strips take the CLOSED=false index mapping through the same machinery
      (3, 5000, 3., false, 1.001),
      (400, 400, 3., false, 1.001),
      (137, 900, 5., false, 1.001),
      (1024, 997, 97., true, 1.001),
      (1024, 997, 41., true, 1.001),
      (300, 900, 31., true, 1.001),
      (2048, 2048, 151., true, PERIODIC_TOL),
    ];
    for (case, &(na, nb, lobes, closed, tol)) in cases.iter().enumerate() {
      let seed = case as u32 + 1;
      let a = gen_ring(seed, na, lobes, 1., 0., 0.);
      let b = gen_ring(seed + 100, nb, lobes, 1.25, 0.6, 0.9);
      let ratio = if closed {
        banded_vs_exact::<true>(&a, &b, None, None)
      } else {
        banded_vs_exact::<false>(&a, &b, None, None)
      };
      assert!(
        ratio >= 1.0 - 1e-6,
        "case {case}: banded scored {ratio} below the exact optimum; solver and scorer disagree"
      );
      assert!(
        ratio < tol,
        "case {case} ({na}x{nb}, {lobes} lobes, closed={closed}): {:.2}% worse than exact \
         (tolerance {:.1}%)",
        (ratio - 1.) * 100.,
        (tol - 1.) * 100.
      );
    }
  }

  /// Same differential check with critical masks populated, which `rail_sweep` always supplies
  /// in production: the mask drives `CRITICAL_PAIR_MULTIPLIER` through the SIMD kernel, the
  /// column-0 accumulator, and the OR-over-buckets subsampling in the coarse guide.
  #[test]
  fn test_banded_solve_matches_exact_with_critical_points() {
    for (case, &(na, nb, sa, sb, closed)) in [
      (400usize, 400usize, 37usize, 37usize, true),
      (850, 850, 11, 13, true),
      (850, 811, 64, 61, true),
      (601, 900, 7, 9, true),
      (1024, 997, 3, 5, true),
      (400, 900, 17, 23, false),
    ]
    .iter()
    .enumerate()
    {
      let seed = case as u32 + 41;
      let a = gen_ring(seed, na, 3., 1., 0., 0.);
      let b = gen_ring(seed + 100, nb, 3., 1.25, 0.6, 0.9);
      let (ca, cb) = (crit_mask(na, sa), crit_mask(nb, sb));
      let ratio = if closed {
        banded_vs_exact::<true>(&a, &b, Some(&ca), Some(&cb))
      } else {
        banded_vs_exact::<false>(&a, &b, Some(&ca), Some(&cb))
      };
      // Dense critical points bias the cost landscape enough that the guide can settle a
      // fraction of a percent off the optimum without ever pressing on the band edge.
      assert!(
        (1.0 - 1e-6..1.005).contains(&ratio),
        "case {case} ({na}x{nb}, crit stride {sa}/{sb}, closed={closed}): ratio {ratio:.5}"
      );
    }
  }

  /// Every ring edge must appear in exactly one stitch triangle.  4096 is past the point where
  /// the coarse guide recurses three levels deep, without tying the test's allocation to
  /// `MAX_DP_STITCH_RESOLUTION`.
  #[test]
  fn test_banded_stitch_is_manifold() {
    for (na, nb) in [(700, 640), (4096, 4096)] {
      let a = gen_ring(7, na, 4., 1., 0., 0.);
      let b = gen_ring(9, nb, 2., 1.3, 0.5, 1.7);
      let mut indices = Vec::new();
      dp_stitch_presampled(
        &a,
        &b,
        None,
        None,
        None,
        None,
        0,
        a.len(),
        true,
        &mut indices,
      );
      assert_missing_edges_empty(&a, &b, &indices);
    }
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
      None,
      None,
      None,
      None,
      ring_a_start,
      ring_b_start,
      true,
      &mut indices,
    );

    assert_missing_edges_empty(&ring0_pts, &ring1_pts, &indices);
  }

  /// A proper stitch uses every edge of each ring exactly once, in a triangle whose tip is on
  /// the opposite ring.
  fn assert_missing_edges_empty(ring0_pts: &[Vec3], ring1_pts: &[Vec3], indices: &[u32]) {
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
