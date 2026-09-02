//! 2D raster kernels over flattened polylines, free of interpreter types so other builtins can
//! reuse them: AA fill coverage / per-texel winding (`EdgeList`, texel space) and exact
//! nearest-segment queries (`SegmentField`, path space on the unit window).

use wide::{f32x4, CmpLt};

use crate::builtins::trace_path::FillRule;
use crate::Vec2;

pub const SUB_SCANLINES: usize = 16;

struct Edge {
  x0: f32,
  y0: f32,
  y1: f32,
  dxdy: f32,
  dir: i32,
}

pub struct EdgeList {
  edges: Vec<Edge>,
  w: usize,
  h: usize,
}

impl EdgeList {
  /// Texel-space polylines. Open polylines contribute nothing unless `implicit_close`.
  pub fn new(polylines: &[(Vec<Vec2>, bool)], implicit_close: bool, w: usize, h: usize) -> Self {
    let mut edges = Vec::new();
    for (pts, closed) in polylines {
      let n = pts.len();
      if n < 2 || !(*closed || implicit_close) {
        continue;
      }
      let edge_count = if n >= 3 { n } else { n - 1 };
      for i in 0..edge_count {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        if a.y == b.y {
          continue;
        }
        // Winding +1 for a CCW loop (Cartesian): a leftward ray crosses its descending side.
        let (top, bot, dir) = if a.y < b.y { (a, b, -1) } else { (b, a, 1) };
        edges.push(Edge {
          x0: top.x,
          y0: top.y,
          y1: bot.y,
          dxdy: (bot.x - top.x) / (bot.y - top.y),
          dir,
        });
      }
    }
    edges.sort_by(|p, q| p.y0.total_cmp(&q.y0));
    EdgeList { edges, w, h }
  }

  fn crossings(&self, active: &[usize], y: f32, xs: &mut Vec<(f32, i32)>) {
    xs.clear();
    for &ix in active {
      let e = &self.edges[ix];
      if e.y0 <= y && y < e.y1 {
        xs.push((e.x0 + (y - e.y0) * e.dxdy, e.dir));
      }
    }
    xs.sort_by(|a, b| a.0.total_cmp(&b.0));
  }

  /// Fraction of each texel inside the fill: exact along x, `SUB_SCANLINES` levels along y.
  pub fn coverage(&self, rule: FillRule) -> Vec<f32> {
    let (w, h) = (self.w, self.h);
    let mut out = vec![0f32; w * h];
    let (mut active, mut xs, mut next) = (Vec::new(), Vec::new(), 0);
    let wt = 1. / SUB_SCANLINES as f32;
    for j in 0..h {
      let (top, bot) = (j as f32, j as f32 + 1.);
      while next < self.edges.len() && self.edges[next].y0 < bot {
        active.push(next);
        next += 1;
      }
      active.retain(|&ix| self.edges[ix].y1 > top);
      if active.is_empty() {
        continue;
      }
      let row = &mut out[j * w..(j + 1) * w];
      for s in 0..SUB_SCANLINES {
        self.crossings(&active, top + (s as f32 + 0.5) * wt, &mut xs);
        let (mut wn, mut span) = (0, None);
        for &(x, dir) in &xs {
          wn += dir;
          match (rule.accepts(wn), span) {
            (true, None) => span = Some(x),
            (false, Some(x0)) => {
              accumulate_span(row, x0, x, wt);
              span = None;
            }
            _ => {}
          }
        }
      }
      for v in row.iter_mut() {
        *v = v.min(1.);
      }
    }
    out
  }

  /// Winding number at every texel center.
  pub fn winding_at_centers(&self) -> Vec<i32> {
    let (w, h) = (self.w, self.h);
    let mut out = vec![0i32; w * h];
    let (mut active, mut xs, mut next) = (Vec::new(), Vec::new(), 0);
    for j in 0..h {
      let y = j as f32 + 0.5;
      while next < self.edges.len() && self.edges[next].y0 <= y {
        active.push(next);
        next += 1;
      }
      active.retain(|&ix| self.edges[ix].y1 > y);
      self.crossings(&active, y, &mut xs);
      if xs.is_empty() {
        continue;
      }
      let (mut wn, mut k) = (0, 0);
      for (i, v) in out[j * w..(j + 1) * w].iter_mut().enumerate() {
        while k < xs.len() && xs[k].0 < i as f32 + 0.5 {
          wn += xs[k].1;
          k += 1;
        }
        *v = wn;
      }
    }
    out
  }
}

fn accumulate_span(row: &mut [f32], xa: f32, xb: f32, wt: f32) {
  let (xa, xb) = (xa.max(0.), xb.min(row.len() as f32));
  if xb <= xa {
    return;
  }
  let (ia, ib) = (xa as usize, xb as usize);
  if ia == ib {
    row[ia] += (xb - xa) * wt;
    return;
  }
  row[ia] += (ia as f32 + 1. - xa) * wt;
  for v in &mut row[ia + 1..ib] {
    *v += wt;
  }
  if ib < row.len() {
    row[ib] += (xb - ib as f32) * wt;
  }
}

pub const MAX_TILED_POINTS: usize = 1 << 23;

/// Copies of every polyline at each `(kx, ky) * period` offset whose bbox meets the unit
/// window expanded by one period (enough for exact periodic nearest queries), tagged with the
/// source index. `Err` carries the point count when it exceeds `MAX_TILED_POINTS`.
pub fn replicate_tiled(
  polylines: &[(Vec<Vec2>, bool)],
  period: f32,
) -> Result<Vec<(Vec<Vec2>, bool, u32)>, usize> {
  let mut out = Vec::new();
  let mut total = 0usize;
  for (si, (pts, closed)) in polylines.iter().enumerate() {
    let (mut lo, mut hi) = (pts[0], pts[0]);
    for p in pts {
      lo = lo.inf(p);
      hi = hi.sup(p);
    }
    let range = |min: f32, max: f32| {
      let k0 = ((-period - max) / period).ceil() as i32;
      let k1 = ((1. + period - min) / period).floor() as i32;
      k0..=k1
    };
    for ky in range(lo.y, hi.y) {
      for kx in range(lo.x, hi.x) {
        total += pts.len();
        if total > MAX_TILED_POINTS {
          return Err(total);
        }
        let off = Vec2::new(kx as f32 * period, ky as f32 * period);
        out.push((pts.iter().map(|p| p + off).collect(), *closed, si as u32));
      }
    }
  }
  Ok(out)
}

#[derive(Clone, Copy, Debug)]
pub struct Segment {
  pub a: Vec2,
  pub b: Vec2,
  pub subpath: u32,
  /// Arc length along the subpath's polyline at `a`.
  pub len_before: f32,
}

impl Segment {
  #[inline]
  pub fn len(&self) -> f32 {
    (self.b - self.a).norm()
  }

  /// Distance from `p` and the clamped projection parameter along `a→b`.
  #[inline]
  pub fn nearest(&self, p: Vec2) -> (f32, f32) {
    let ab = self.b - self.a;
    let len2 = ab.norm_squared();
    let s = if len2 > 0. {
      ((p - self.a).dot(&ab) / len2).clamp(0., 1.)
    } else {
      0.
    };
    ((p - (self.a + ab * s)).norm(), s)
  }

  /// +1 when `p` is left of the direction of travel, -1 right, 0 on the line.
  #[inline]
  pub fn side(&self, p: Vec2) -> f32 {
    let (ab, ap) = (self.b - self.a, p - self.a);
    (ab.x * ap.y - ab.y * ap.x).signum()
  }
}

#[derive(Clone, Copy, Debug)]
pub struct NearestHit {
  pub dist: f32,
  pub seg: u32,
  pub s: f32,
}

/// Exact nearest-segment queries at texel centers. A quadtree descent over the texel grid
/// narrows a candidate list per node: a segment can only be nearest somewhere in a node if it
/// is within `2 * half_diag` of the node center's own nearest distance. Leaves run a SIMD texel
/// loop over what survives.
pub struct SegmentField {
  segs: Vec<Segment>,
  ax: Vec<f32>,
  ay: Vec<f32>,
  abx: Vec<f32>,
  aby: Vec<f32>,
  inv_len2: Vec<f32>,
  w: usize,
  h: usize,
}

const LEAF: usize = 4;

struct Descent {
  out: Vec<NearestHit>,
  /// Per depth, the four sibling candidate lists.
  lists: Vec<[Vec<u32>; 4]>,
  d2: Vec<f32x4>,
}

impl SegmentField {
  pub fn new(segs: Vec<Segment>, w: usize, h: usize) -> Self {
    let n = segs.len();
    let (mut ax, mut ay, mut abx, mut aby, mut inv_len2) = (
      Vec::with_capacity(n),
      Vec::with_capacity(n),
      Vec::with_capacity(n),
      Vec::with_capacity(n),
      Vec::with_capacity(n),
    );
    for s in &segs {
      let ab = s.b - s.a;
      let len2 = ab.norm_squared();
      ax.push(s.a.x);
      ay.push(s.a.y);
      abx.push(ab.x);
      aby.push(ab.y);
      inv_len2.push(if len2 > 0. { 1. / len2 } else { 0. });
    }
    SegmentField {
      segs,
      ax,
      ay,
      abx,
      aby,
      inv_len2,
      w,
      h,
    }
  }

  pub fn segments(&self) -> &[Segment] {
    &self.segs
  }

  /// Squared distance and clamped projection parameter from four points to segment `ix`.
  #[inline(always)]
  fn nearest4(&self, ix: usize, px: f32x4, py: f32x4) -> (f32x4, f32x4) {
    let (apx, apy) = (
      px - f32x4::splat(self.ax[ix]),
      py - f32x4::splat(self.ay[ix]),
    );
    let (abx, aby) = (f32x4::splat(self.abx[ix]), f32x4::splat(self.aby[ix]));
    let s = ((apx * abx + apy * aby) * f32x4::splat(self.inv_len2[ix]))
      .max(f32x4::ZERO)
      .min(f32x4::ONE);
    let (dx, dy) = (apx - abx * s, apy - aby * s);
    (dx * dx + dy * dy, s)
  }

  /// Nearest segment for every texel center `((i+.5)/w, (j+.5)/h)`, row-major.
  pub fn nearest_at_centers(&self) -> Vec<NearestHit> {
    let (w, h) = (self.w, self.h);
    let none = NearestHit {
      dist: f32::INFINITY,
      seg: 0,
      s: 0.,
    };
    let mut st = Descent {
      out: vec![none; w * h],
      lists: Vec::new(),
      d2: Vec::new(),
    };
    if self.segs.is_empty() {
      return st.out;
    }
    let size = w.max(h).next_power_of_two().max(LEAF);
    let depth = (size / LEAF).trailing_zeros() as usize;
    st.lists = (0..=depth).map(|_| Default::default()).collect();
    st.lists[0][0] = (0..self.segs.len() as u32).collect();
    self.descend(&mut st, 0, 0, 0, 0, size);
    st.out
  }

  fn descend(
    &self,
    st: &mut Descent,
    depth: usize,
    slot: usize,
    x0: usize,
    y0: usize,
    size: usize,
  ) {
    let (tw, th) = (1. / self.w as f32, 1. / self.h as f32);
    let list = std::mem::take(&mut st.lists[depth][slot]);
    if size == LEAF {
      let px = (f32x4::splat(x0 as f32) + f32x4::new([0.5, 1.5, 2.5, 3.5])) * f32x4::splat(tw);
      let nx = (self.w - x0).min(LEAF);
      for ty in y0..(y0 + LEAF).min(self.h) {
        let py = f32x4::splat((ty as f32 + 0.5) * th);
        let mut best_d2 = f32x4::splat(f32::INFINITY);
        let mut best_ix = f32x4::ZERO;
        let mut best_s = f32x4::ZERO;
        for &ix in &list {
          let (d2, s) = self.nearest4(ix as usize, px, py);
          let m = d2.cmp_lt(best_d2);
          best_d2 = m.blend(d2, best_d2);
          best_ix = m.blend(f32x4::splat(ix as f32), best_ix);
          best_s = m.blend(s, best_s);
        }
        let (d, ix, s) = (
          best_d2.sqrt().to_array(),
          best_ix.to_array(),
          best_s.to_array(),
        );
        let row = &mut st.out[ty * self.w + x0..ty * self.w + x0 + nx];
        for (l, o) in row.iter_mut().enumerate() {
          *o = NearestHit {
            dist: d[l],
            seg: ix[l] as u32,
            s: s[l],
          };
        }
      }
      st.lists[depth][slot] = list;
      return;
    }
    let half = size / 2;
    let q = half as f32 * 0.5;
    let (fx, fy) = (x0 as f32, y0 as f32);
    let cx = f32x4::new([fx + q, fx + 3. * q, fx + q, fx + 3. * q]) * f32x4::splat(tw);
    let cy = f32x4::new([fy + q, fy + q, fy + 3. * q, fy + 3. * q]) * f32x4::splat(th);
    let diag = ((half as f32 * tw).powi(2) + (half as f32 * th).powi(2)).sqrt();
    st.d2.clear();
    let mut min = f32x4::splat(f32::INFINITY);
    for &ix in &list {
      let (d2, _) = self.nearest4(ix as usize, cx, cy);
      st.d2.push(d2);
      min = min.min(d2);
    }
    let bound = min.sqrt() + f32x4::splat(diag);
    let keep2 = (bound * bound).to_array();
    for l in &mut st.lists[depth + 1] {
      l.clear();
    }
    for (i, &ix) in list.iter().enumerate() {
      let d = st.d2[i].to_array();
      for c in 0..4 {
        if d[c] <= keep2[c] {
          st.lists[depth + 1][c].push(ix);
        }
      }
    }
    st.lists[depth][slot] = list;
    for c in 0..4 {
      let (x, y) = (x0 + (c & 1) * half, y0 + (c >> 1) * half);
      if x < self.w && y < self.h {
        self.descend(st, depth + 1, c, x, y, half);
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn square(x0: f32, y0: f32, x1: f32, y1: f32) -> (Vec<Vec2>, bool) {
    (
      vec![
        Vec2::new(x0, y0),
        Vec2::new(x1, y0),
        Vec2::new(x1, y1),
        Vec2::new(x0, y1),
      ],
      true,
    )
  }

  #[test]
  fn coverage_is_exact_on_aligned_and_quarter_offset_squares() {
    let cov = EdgeList::new(&[square(2., 2., 6., 6.)], false, 8, 8).coverage(FillRule::NonZero);
    assert!((cov.iter().sum::<f32>() - 16.).abs() < 1e-4);
    assert_eq!(cov[3 * 8 + 3], 1.);
    assert_eq!(cov[0], 0.);

    let cov =
      EdgeList::new(&[square(2.25, 2.25, 6.25, 6.25)], false, 8, 8).coverage(FillRule::NonZero);
    assert!((cov.iter().sum::<f32>() - 16.).abs() < 1e-4);
    assert!((cov[2 * 8 + 2] - 0.5625).abs() < 1e-4, "{}", cov[2 * 8 + 2]);
    assert!((cov[3 * 8 + 2] - 0.75).abs() < 1e-4);
    assert!((cov[3 * 8 + 6] - 0.25).abs() < 1e-4);
  }

  #[test]
  fn fill_rules_on_nested_and_overlapping_squares() {
    let outer = square(0., 0., 8., 8.);
    let inner_ccw = square(2., 2., 6., 6.);
    let mut inner_cw = inner_ccw.clone();
    inner_cw.0.reverse();
    let mid = |cov: &[f32]| cov[4 * 8 + 4];
    let polys = [outer.clone(), inner_ccw.clone()];
    assert_eq!(
      mid(&EdgeList::new(&polys, false, 8, 8).coverage(FillRule::NonZero)),
      1.
    );
    assert_eq!(
      mid(&EdgeList::new(&polys, false, 8, 8).coverage(FillRule::EvenOdd)),
      0.
    );
    let polys = [outer.clone(), inner_cw];
    assert_eq!(
      mid(&EdgeList::new(&polys, false, 8, 8).coverage(FillRule::NonZero)),
      0.
    );
    assert_eq!(
      mid(&EdgeList::new(&polys, false, 8, 8).coverage(FillRule::Positive)),
      0.
    );
    assert_eq!(
      EdgeList::new(&polys, false, 8, 8).coverage(FillRule::Negative)[0],
      0.
    );
    // CCW outer alone is positive winding
    let polys = [outer];
    assert_eq!(
      mid(&EdgeList::new(&polys, false, 8, 8).coverage(FillRule::Positive)),
      1.
    );
    assert_eq!(
      mid(&EdgeList::new(&polys, false, 8, 8).coverage(FillRule::Negative)),
      0.
    );
    // Overlapping same-direction squares: nonzero union with no dip on the shared interior edge
    let polys = [square(0., 0., 5.5, 8.), square(2.5, 0., 8., 8.)];
    let cov = EdgeList::new(&polys, false, 8, 8).coverage(FillRule::NonZero);
    assert!(cov.iter().all(|&v| (v - 1.).abs() < 1e-5), "{cov:?}");
    let wn = EdgeList::new(&polys, false, 8, 8).winding_at_centers();
    assert_eq!(wn[4 * 8 + 4], 2);
    assert_eq!(wn[4 * 8 + 1], 1);
  }

  #[test]
  fn open_polylines_only_fill_when_implicitly_closed() {
    let tri = (
      vec![Vec2::new(0., 0.), Vec2::new(8., 0.), Vec2::new(0., 8.)],
      false,
    );
    assert_eq!(
      EdgeList::new(&[tri.clone()], false, 8, 8)
        .coverage(FillRule::NonZero)
        .iter()
        .sum::<f32>(),
      0.
    );
    let cov = EdgeList::new(&[tri], true, 8, 8).coverage(FillRule::NonZero);
    assert!((cov.iter().sum::<f32>() - 32.).abs() < 0.1);
  }

  #[test]
  fn nearest_matches_brute_force() {
    let mut segs = Vec::new();
    let n = 37;
    for i in 0..n {
      let a = 6.2832 * i as f32 / n as f32;
      let b = 6.2832 * (i + 1) as f32 / n as f32;
      segs.push(Segment {
        a: Vec2::new(0.5 + 0.3 * a.cos(), 0.5 + 0.3 * a.sin()),
        b: Vec2::new(0.5 + 0.3 * b.cos(), 0.5 + 0.3 * b.sin()),
        subpath: 0,
        len_before: 0.,
      });
    }
    segs.push(Segment {
      a: Vec2::new(-0.2, 1.3),
      b: Vec2::new(0.1, 0.9),
      subpath: 1,
      len_before: 0.,
    });
    let (w, h) = (40, 24);
    let field = SegmentField::new(segs.clone(), w, h);
    let hits = field.nearest_at_centers();
    for j in 0..h {
      for i in 0..w {
        let p = Vec2::new((i as f32 + 0.5) / w as f32, (j as f32 + 0.5) / h as f32);
        let brute = segs
          .iter()
          .map(|s| s.nearest(p).0)
          .fold(f32::INFINITY, f32::min);
        let hit = hits[j * w + i];
        assert!(
          (hit.dist - brute).abs() < 1e-6,
          "({i},{j}) {} vs {brute}",
          hit.dist
        );
        assert!((segs[hit.seg as usize].nearest(p).0 - brute).abs() < 1e-6);
      }
    }
  }
}
