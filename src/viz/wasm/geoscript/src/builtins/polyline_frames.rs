use std::rc::Rc;

use fxhash::FxHashMap;
use mesh::linked_mesh::Vec3;

use crate::{seq::EagerSeq, ArgRef, ErrorStack, EvalCtx, Sym, Value, Vec2};

const DEGEN_EPS: f32 = 1e-6;

/// Arc-length table over a polyline's segments. Sample `t` in [0, 1] maps to arc length, so
/// evenly-spaced `t` gives evenly-spaced points regardless of vertex distribution.
struct ArcTable {
  seg_len: Vec<f32>,
  cum: Vec<f32>,
  closed: bool,
}

impl ArcTable {
  fn new(seg_len: Vec<f32>, closed: bool) -> Self {
    let mut cum = Vec::with_capacity(seg_len.len() + 1);
    let mut acc = 0.;
    cum.push(0.);
    for &l in &seg_len {
      acc += l;
      cum.push(acc);
    }
    ArcTable {
      seg_len,
      cum,
      closed,
    }
  }

  fn segs(&self) -> usize {
    self.seg_len.len()
  }

  fn total(&self) -> f32 {
    self.cum[self.segs()]
  }

  /// Maps `t` to `(segment index, position within that segment in [0, 1])`.
  fn locate(&self, t: f32) -> (usize, f32) {
    let s = t.clamp(0., 1.) * self.total();
    let seg = match self.cum.binary_search_by(|c| c.partial_cmp(&s).unwrap()) {
      Ok(i) => i.min(self.segs() - 1),
      Err(i) => (i - 1).min(self.segs() - 1),
    };
    ((seg), (s - self.cum[seg]) / self.seg_len[seg])
  }

  fn prev_seg(&self, seg: usize) -> usize {
    (seg + self.segs() - 1) % self.segs()
  }

  /// Half-width of the smoothing band at the junction entering `seg`, clamped so neighbouring
  /// bands can never overlap (and so a band never runs past a polyline endpoint).
  fn band(&self, seg: usize, smooth: f32) -> f32 {
    if seg == 0 && !self.closed {
      return 0.;
    }
    smooth
      .min(self.seg_len[seg] * 0.5)
      .min(self.seg_len[self.prev_seg(seg)] * 0.5)
  }

  /// Resolves a located sample to the two segment frames it sits between and the blend weight.
  /// `None` means the sample is outside every smoothing band and takes its segment's frame as-is.
  fn blend(&self, seg: usize, local: f32, smooth: f32) -> Option<(usize, usize, f32)> {
    if smooth <= 0. {
      return None;
    }
    let d_start = local * self.seg_len[seg];
    let r_start = self.band(seg, smooth);
    if d_start < r_start {
      return Some((self.prev_seg(seg), seg, 0.5 + d_start / (2. * r_start)));
    }

    let next = (seg + 1) % self.segs();
    if next == 0 && !self.closed {
      return None;
    }
    let d_end = (1. - local) * self.seg_len[seg];
    let r_end = self.band(next, smooth);
    if d_end < r_end {
      return Some((seg, next, 0.5 - d_end / (2. * r_end)));
    }
    None
  }
}

fn unit(v: Vec3) -> Option<Vec3> {
  let n = v.norm();
  (n > DEGEN_EPS).then(|| v / n)
}

fn nlerp3(a: Vec3, b: Vec3, w: f32) -> Option<Vec3> {
  unit(a * (1. - w) + b * w)
}

/// Collapses runs of coincident vertices; a polyline with a repeated point has no direction
/// across it and would put a zero-length span in the arc-length table.
fn dedup<V: Copy>(pts: Vec<V>, dist_sq: impl Fn(&V, &V) -> f32) -> Vec<V> {
  let mut out: Vec<V> = Vec::with_capacity(pts.len());
  for p in pts {
    if out
      .last()
      .is_some_and(|q| dist_sq(q, &p) < DEGEN_EPS * DEGEN_EPS)
    {
      continue;
    }
    out.push(p);
  }
  out
}

fn frame_map(t: f32, pos: Value, tangent: Value, normal: Value, binormal: Option<Value>) -> Value {
  let mut map: FxHashMap<String, Value> = FxHashMap::default();
  map.insert("t".to_owned(), Value::Float(t));
  map.insert("pos".to_owned(), pos);
  map.insert("tangent".to_owned(), tangent);
  map.insert("normal".to_owned(), normal);
  if let Some(b) = binormal {
    map.insert("binormal".to_owned(), b);
  }
  Value::Map(Rc::new(map))
}

fn frames_2d(
  pts: &[Vec2],
  closed: bool,
  smooth: f32,
  inward_normal: bool,
  ts: &[f32],
) -> Vec<Value> {
  let n = pts.len();
  let segs = if closed { n } else { n - 1 };
  let dirs: Vec<Vec2> = (0..segs)
    .map(|i| (pts[(i + 1) % n] - pts[i]).normalize())
    .collect();
  let table = ArcTable::new(
    (0..segs)
      .map(|i| (pts[(i + 1) % n] - pts[i]).norm())
      .collect(),
    closed,
  );

  // The filled region of a closed 2D path lies to the left of the direction of travel, so for a
  // CW loop the left-perpendicular points outward and has to be flipped.
  let flip = inward_normal && closed && {
    let area: f32 = (0..n)
      .map(|i| {
        let (p, q) = (pts[i], pts[(i + 1) % n]);
        p.x * q.y - q.x * p.y
      })
      .sum();
    area < 0.
  };

  ts.iter()
    .map(|&t| {
      let (seg, local) = table.locate(t);
      let pos = pts[seg] + (pts[(seg + 1) % n] - pts[seg]) * local;
      let tangent = match table.blend(seg, local, smooth) {
        Some((a, b, w)) => {
          let v = dirs[a] * (1. - w) + dirs[b] * w;
          let len = v.norm();
          if len > DEGEN_EPS {
            v / len
          } else {
            dirs[seg]
          }
        }
        None => dirs[seg],
      };
      let mut normal = Vec2::new(-tangent.y, tangent.x);
      if flip {
        normal = -normal;
      }
      frame_map(
        t,
        Value::Vec2(pos),
        Value::Vec2(tangent),
        Value::Vec2(normal),
        None,
      )
    })
    .collect()
}

/// Per-segment normals. With `up` this is a fixed-reference frame; without, normals are
/// parallel-transported segment to segment (rotation-minimizing), which is the only choice that
/// stays coherent on a spine that turns out of any single plane.
fn segment_normals(dirs: &[Vec3], closed: bool, up: Option<Vec3>) -> Vec<Vec3> {
  let segs = dirs.len();
  let mut normals: Vec<Vec3> = Vec::with_capacity(segs);

  let seed = |t: Vec3| -> Vec3 {
    let fallback = if t.dot(&Vec3::new(0., 1., 0.)).abs() > 0.999 {
      Vec3::new(1., 0., 0.)
    } else {
      Vec3::new(0., 1., 0.)
    };
    t.cross(&fallback).normalize()
  };

  for (i, &t) in dirs.iter().enumerate() {
    let n = match up {
      Some(up) => {
        let c = t.cross(&up);
        if c.norm() > DEGEN_EPS {
          c.normalize()
        } else if i == 0 {
          seed(t)
        } else {
          // spine momentarily parallel to `up`: transport rather than error out
          let prev: Vec3 = normals[i - 1];
          let proj = prev - t * t.dot(&prev);
          if proj.norm() > DEGEN_EPS {
            proj.normalize()
          } else {
            seed(t)
          }
        }
      }
      None if i == 0 => seed(t),
      None => {
        let prev: Vec3 = normals[i - 1];
        let proj = prev - t * t.dot(&prev);
        if proj.norm() > DEGEN_EPS {
          proj.normalize()
        } else {
          seed(t)
        }
      }
    };
    normals.push(n);
  }

  // Transporting around a closed loop returns twisted by the holonomy angle rather than back to
  // the start frame. Spread that correction evenly over the loop so no single junction carries
  // the whole jump.
  if closed && up.is_none() && segs >= 3 {
    let (t0, n0) = (dirs[0], normals[0]);
    let last = normals[segs - 1];
    let wrap = last - t0 * t0.dot(&last);
    if wrap.norm() > DEGEN_EPS {
      let wrap = wrap.normalize();
      let theta = n0.cross(&wrap).dot(&t0).atan2(n0.dot(&wrap));
      for (i, n) in normals.iter_mut().enumerate().skip(1) {
        let angle = -theta * i as f32 / segs as f32;
        let t = dirs[i];
        let (s, c) = angle.sin_cos();
        *n = *n * c + t.cross(n) * s;
      }
    }
  }

  normals
}

fn frames_3d(pts: &[Vec3], closed: bool, smooth: f32, up: Option<Vec3>, ts: &[f32]) -> Vec<Value> {
  let n = pts.len();
  let segs = if closed { n } else { n - 1 };
  let dirs: Vec<Vec3> = (0..segs)
    .map(|i| (pts[(i + 1) % n] - pts[i]).normalize())
    .collect();
  let table = ArcTable::new(
    (0..segs)
      .map(|i| (pts[(i + 1) % n] - pts[i]).norm())
      .collect(),
    closed,
  );
  let normals = segment_normals(&dirs, closed, up);

  ts.iter()
    .map(|&t| {
      let (seg, local) = table.locate(t);
      let pos = pts[seg] + (pts[(seg + 1) % n] - pts[seg]) * local;

      // Blending the two junction normals after projecting both onto the blended tangent lands
      // exactly on each segment's own frame at the band edges, so the result is continuous
      // regardless of how the normals were derived.
      let (tangent, normal) = match table.blend(seg, local, smooth) {
        Some((a, b, w)) => nlerp3(dirs[a], dirs[b], w)
          .and_then(|tan| {
            let proj = |v: Vec3| unit(v - tan * tan.dot(&v));
            let blended = nlerp3(proj(normals[a])?, proj(normals[b])?, w)?;
            Some((tan, unit(blended - tan * tan.dot(&blended))?))
          })
          .unwrap_or((dirs[seg], normals[seg])),
        None => (dirs[seg], normals[seg]),
      };
      let binormal = tangent.cross(&normal);

      frame_map(
        t,
        Value::Vec3(pos),
        Value::Vec3(tangent),
        Value::Vec3(normal),
        Some(Value::Vec3(binormal)),
      )
    })
    .collect()
}

enum Points {
  D2(Vec<Vec2>),
  D3(Vec<Vec3>),
}

fn collect_points(ctx: &EvalCtx, seq: &Rc<dyn crate::Sequence>) -> Result<Points, ErrorStack> {
  let vals: Vec<Value> = seq.consume(ctx).collect::<Result<_, _>>()?;
  match vals.first() {
    Some(Value::Vec2(_)) => {
      let pts = vals
        .iter()
        .map(|v| {
          v.as_vec2().copied().ok_or_else(|| {
            ErrorStack::new(format!(
              "polyline_frames: `points` mixes vec2 and other values; found {v:?}"
            ))
          })
        })
        .collect::<Result<Vec<_>, _>>()?;
      Ok(Points::D2(dedup(pts, |a, b| (a - b).norm_squared())))
    }
    Some(Value::Vec3(_)) => {
      let pts = vals
        .iter()
        .map(|v| {
          v.as_vec3().copied().ok_or_else(|| {
            ErrorStack::new(format!(
              "polyline_frames: `points` mixes vec3 and other values; found {v:?}"
            ))
          })
        })
        .collect::<Result<Vec<_>, _>>()?;
      Ok(Points::D3(dedup(pts, |a, b| (a - b).norm_squared())))
    }
    Some(other) => Err(ErrorStack::new(format!(
      "polyline_frames: `points` must be a sequence of vec2 or vec3, found: {other:?}"
    ))),
    None => Err(ErrorStack::new("polyline_frames: `points` is empty")),
  }
}

pub fn polyline_frames_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let points_seq = arg_refs[0]
    .resolve(args, kwargs)
    .as_sequence()
    .ok_or_else(|| ErrorStack::new("polyline_frames: `points` must be a sequence"))?;
  let closed = arg_refs[2].resolve(args, kwargs).as_bool().unwrap();
  let smooth = arg_refs[3].resolve(args, kwargs).as_float().unwrap();
  let up_val = arg_refs[4].resolve(args, kwargs);
  let inward_normal = arg_refs[5].resolve(args, kwargs).as_bool().unwrap();

  if smooth < 0. {
    return Err(ErrorStack::new("polyline_frames: `smooth` must be >= 0"));
  }

  let mut points = collect_points(ctx, &points_seq)?;
  // An explicitly-closed point list (last == first) would otherwise contribute a zero-length
  // wrap segment on top of the implicit one.
  if closed {
    match &mut points {
      Points::D2(p) => {
        if p.len() > 1 && (p[0] - p[p.len() - 1]).norm_squared() < DEGEN_EPS * DEGEN_EPS {
          p.pop();
        }
      }
      Points::D3(p) => {
        if p.len() > 1 && (p[0] - p[p.len() - 1]).norm_squared() < DEGEN_EPS * DEGEN_EPS {
          p.pop();
        }
      }
    }
  }
  let n = match &points {
    Points::D2(p) => p.len(),
    Points::D3(p) => p.len(),
  };
  let min = if closed { 3 } else { 2 };
  if n < min {
    return Err(ErrorStack::new(format!(
      "polyline_frames: need at least {min} distinct points{}, found {n}",
      if closed { " when `closed=true`" } else { "" }
    )));
  }

  let ts: Vec<f32> = match def_ix {
    0 => {
      let count = arg_refs[1].resolve(args, kwargs).as_int().unwrap();
      if count < 1 {
        return Err(ErrorStack::new("polyline_frames: `n` must be >= 1"));
      }
      let count = count as usize;
      // Closed loops sample half-open so the wrap-around sample doesn't duplicate the first.
      if closed {
        (0..count).map(|i| i as f32 / count as f32).collect()
      } else if count == 1 {
        vec![0.]
      } else {
        (0..count).map(|i| i as f32 / (count - 1) as f32).collect()
      }
    }
    _ => {
      let t_seq = arg_refs[1]
        .resolve(args, kwargs)
        .as_sequence()
        .ok_or_else(|| ErrorStack::new("polyline_frames: `t` must be a sequence"))?;
      t_seq
        .consume(ctx)
        .map(|res| {
          let v = res?;
          v.as_float().ok_or_else(|| {
            ErrorStack::new(format!(
              "polyline_frames: `t` must be a sequence of numbers, found: {v:?}"
            ))
          })
        })
        .collect::<Result<Vec<_>, _>>()?
    }
  };

  let frames = match points {
    Points::D2(pts) => {
      if !up_val.is_nil() {
        return Err(ErrorStack::new(
          "polyline_frames: `up` is only meaningful for vec3 polylines",
        ));
      }
      frames_2d(&pts, closed, smooth, inward_normal, &ts)
    }
    Points::D3(pts) => {
      let up = match up_val {
        Value::Nil => None,
        v => {
          let up = *v.as_vec3().ok_or_else(|| {
            ErrorStack::new(format!(
              "polyline_frames: `up` must be a vec3, found: {v:?}"
            ))
          })?;
          if up.norm() < DEGEN_EPS {
            return Err(ErrorStack::new("polyline_frames: `up` must be non-zero"));
          }
          Some(up.normalize())
        }
      };
      frames_3d(&pts, closed, smooth, up, &ts)
    }
  };

  Ok(Value::Sequence(Rc::new(EagerSeq {
    inner: Rc::new(frames),
  })))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::parse_and_eval_program;

  fn eval_frames(src: &str) -> Vec<Rc<FxHashMap<String, Value>>> {
    let ctx = parse_and_eval_program(src).unwrap();
    let Some(Value::Sequence(seq)) = ctx.get_global("f") else {
      panic!("expected `f` to be a sequence");
    };
    seq
      .consume(&ctx)
      .map(|v| match v.unwrap() {
        Value::Map(m) => m,
        other => panic!("expected frame map, found {other:?}"),
      })
      .collect()
  }

  fn v2(frame: &FxHashMap<String, Value>, key: &str) -> Vec2 {
    *frame.get(key).unwrap().as_vec2().unwrap()
  }

  fn v3(frame: &FxHashMap<String, Value>, key: &str) -> Vec3 {
    *frame.get(key).unwrap().as_vec3().unwrap()
  }

  /// L-shaped polyline of total length 20; `n` samples must be evenly spaced in arc length and
  /// each must carry the direction of the segment it landed on.
  #[test]
  fn evenly_spaced_2d_samples_follow_arc_length() {
    let frames = eval_frames(
      r#"
f = polyline_frames([v2(0,0), v2(10,0), v2(10,10)], 5)
"#,
    );
    assert_eq!(frames.len(), 5);

    let expected_pos = [
      Vec2::new(0., 0.),
      Vec2::new(5., 0.),
      Vec2::new(10., 0.),
      Vec2::new(10., 5.),
      Vec2::new(10., 10.),
    ];
    for (i, f) in frames.iter().enumerate() {
      assert!(
        (v2(f, "pos") - expected_pos[i]).norm() < 1e-4,
        "sample {i}: pos {:?} != {:?}",
        v2(f, "pos"),
        expected_pos[i]
      );
    }

    // No smoothing: every sample takes its segment's exact direction. The corner sample at
    // t=0.5 sits at the start of the second segment, so it reads +y.
    let tangents: Vec<Vec2> = frames.iter().map(|f| v2(f, "tangent")).collect();
    for (i, expected) in [
      Vec2::new(1., 0.),
      Vec2::new(1., 0.),
      Vec2::new(0., 1.),
      Vec2::new(0., 1.),
      Vec2::new(0., 1.),
    ]
    .iter()
    .enumerate()
    {
      assert!(
        (tangents[i] - expected).norm() < 1e-4,
        "sample {i}: tangent {:?} != {expected:?}",
        tangents[i]
      );
    }

    // normal is the left-perpendicular of the tangent
    for f in &frames {
      let (t, n) = (v2(f, "tangent"), v2(f, "normal"));
      assert!((n - Vec2::new(-t.y, t.x)).norm() < 1e-4);
    }
  }

  #[test]
  fn explicit_t_sequence_is_returned_in_order_given() {
    let frames = eval_frames(
      r#"
f = polyline_frames([v2(0,0), v2(10,0), v2(10,10)], [0.75, 0.0, 0.25, 2.0, -1.0])
"#,
    );
    let xs: Vec<f32> = frames.iter().map(|f| v2(f, "pos").y).collect();
    // t values map to arc length; out-of-range values clamp to the endpoints.
    assert!((xs[0] - 5.).abs() < 1e-4, "t=0.75 -> y=5, got {}", xs[0]);
    assert!((xs[1] - 0.).abs() < 1e-4);
    assert!((xs[2] - 0.).abs() < 1e-4);
    assert!((xs[3] - 10.).abs() < 1e-4, "t=2 clamps to the end");
    assert!((xs[4] - 0.).abs() < 1e-4, "t=-1 clamps to the start");
    assert!((frames[0].get("t").unwrap().as_float().unwrap() - 0.75).abs() < 1e-6);
  }

  /// `smooth` must sweep the tangent monotonically across the corner and land exactly on the
  /// neighbouring segment frames at the band edges (no discontinuity where smoothing kicks in).
  #[test]
  fn smoothing_blends_tangent_continuously_across_a_corner() {
    let frames = eval_frames(
      r#"
// L-shape, length 20, corner at t=0.5. smooth=2 -> band covers t in [0.4, 0.6].
f = polyline_frames([v2(0,0), v2(10,0), v2(10,10)], 21, smooth=2)
"#,
    );
    let angles: Vec<f32> = frames
      .iter()
      .map(|f| {
        let t = v2(f, "tangent");
        t.y.atan2(t.x).to_degrees()
      })
      .collect();

    // outside the band the tangent is still exactly the segment direction
    assert!(
      angles[8].abs() < 1e-3,
      "t=0.40 is the band edge: {}",
      angles[8]
    );
    assert!((angles[7] - 0.).abs() < 1e-3);
    assert!(
      (angles[12] - 90.).abs() < 1e-3,
      "t=0.60 band edge: {}",
      angles[12]
    );
    assert!((angles[13] - 90.).abs() < 1e-3);

    // strictly increasing through the band, hitting the bisector exactly at the corner
    for i in 8..12 {
      assert!(
        angles[i + 1] > angles[i],
        "angle must increase across the band: {} -> {}",
        angles[i],
        angles[i + 1]
      );
    }
    assert!(
      (angles[10] - 45.).abs() < 1e-3,
      "corner tangent should bisect: {}",
      angles[10]
    );

    // pos stays exactly on the polyline — smoothing only affects orientation
    assert!((v2(&frames[10], "pos") - Vec2::new(10., 0.)).norm() < 1e-4);
  }

  /// The pillar case: an upright reference frame that never rolls, whatever the path does.
  #[test]
  fn up_vector_gives_a_roll_free_3d_frame() {
    let frames = eval_frames(
      r#"
f = polyline_frames(
  [v3(0,0,0), v3(10,0,0), v3(10,0,10), v3(10,5,20)],
  12,
  up=v3(0,1,0),
)
"#,
    );
    assert_eq!(frames.len(), 12);
    let up = Vec3::new(0., 1., 0.);
    for (i, f) in frames.iter().enumerate() {
      let (t, n, b) = (v3(f, "tangent"), v3(f, "normal"), v3(f, "binormal"));
      assert!((t.norm() - 1.).abs() < 1e-4 && (n.norm() - 1.).abs() < 1e-4);
      assert!(t.dot(&n).abs() < 1e-4, "frame {i} not orthogonal");
      assert!((b - t.cross(&n)).norm() < 1e-4, "frame {i} binormal");
      // with a fixed up reference the normal stays horizontal
      assert!(n.dot(&up).abs() < 1e-4, "frame {i} normal rolled: {n:?}");
    }
  }

  /// The 3D blend has to land exactly on each segment's own frame at the band edges, otherwise
  /// smoothing trades a snap at the corner for two smaller snaps at the band boundaries.
  #[test]
  fn smoothing_keeps_the_3d_frame_continuous() {
    let smoothed = eval_frames(
      r#"
f = polyline_frames([v3(0,0,0), v3(10,0,0), v3(10,0,10), v3(10,10,10)], 101, smooth=2)
"#,
    );
    let exact = eval_frames(
      r#"
f = polyline_frames([v3(0,0,0), v3(10,0,0), v3(10,0,10), v3(10,10,10)], 101)
"#,
    );

    for (i, f) in smoothed.iter().enumerate() {
      let (t, n, b) = (v3(f, "tangent"), v3(f, "normal"), v3(f, "binormal"));
      assert!((t.norm() - 1.).abs() < 1e-4, "frame {i} tangent not unit");
      assert!(t.dot(&n).abs() < 1e-4, "frame {i} not orthogonal");
      assert!((b - t.cross(&n)).norm() < 1e-4, "frame {i} binormal");
    }

    // no jumps anywhere: consecutive samples are 0.3 units apart on a 30-unit path, and the
    // widest blend band turns 90 degrees over 4 units
    for i in 1..smoothed.len() {
      let dn = (v3(&smoothed[i], "normal") - v3(&smoothed[i - 1], "normal")).norm();
      let dt = (v3(&smoothed[i], "tangent") - v3(&smoothed[i - 1], "tangent")).norm();
      assert!(
        dn < 0.2 && dt < 0.2,
        "discontinuity at sample {i}: dn={dn}, dt={dt}"
      );
    }

    // outside every band the smoothed frame is identical to the unsmoothed one
    for i in [0, 10, 20, 45, 55, 80, 100] {
      let d = (v3(&smoothed[i], "tangent") - v3(&exact[i], "tangent")).norm();
      assert!(
        d < 1e-4,
        "sample {i} should be untouched by smoothing, got {d}"
      );
    }
  }

  #[test]
  fn closed_loop_samples_half_open_and_wraps() {
    let frames = eval_frames(
      r#"
f = polyline_frames([v2(0,0), v2(10,0), v2(10,10), v2(0,10)], 4, closed=true)
"#,
    );
    assert_eq!(frames.len(), 4);
    // half-open sampling: 4 samples over a 4-segment loop land on the 4 corners, and the last
    // sample is not a duplicate of the first
    let expected = [
      Vec2::new(0., 0.),
      Vec2::new(10., 0.),
      Vec2::new(10., 10.),
      Vec2::new(0., 10.),
    ];
    for (i, f) in frames.iter().enumerate() {
      assert!((v2(f, "pos") - expected[i]).norm() < 1e-4, "corner {i}");
    }
    // CCW square -> left-perpendicular already points inward, and every normal should
    for f in &frames {
      let (pos, n) = (v2(f, "pos"), v2(f, "normal"));
      let toward_center = Vec2::new(5., 5.) - pos;
      assert!(
        n.dot(&toward_center) > 0.,
        "normal {n:?} not inward at {pos:?}"
      );
    }
  }

  #[test]
  fn cw_closed_loop_still_gets_inward_normals() {
    let frames = eval_frames(
      r#"
f = polyline_frames([v2(0,0), v2(0,10), v2(10,10), v2(10,0)], 4, closed=true)
"#,
    );
    for f in &frames {
      let (pos, n) = (v2(f, "pos"), v2(f, "normal"));
      let toward_center = Vec2::new(5., 5.) - pos;
      assert!(
        n.dot(&toward_center) > 0.,
        "normal {n:?} not inward at {pos:?}"
      );
    }
  }

  #[test]
  fn rejects_degenerate_input() {
    for (src, needle) in [
      ("f = polyline_frames([], 4)", "empty"),
      ("f = polyline_frames([v2(0,0)], 4)", "at least 2"),
      (
        "f = polyline_frames([v2(0,0), v2(0,0), v2(0,0)], 4)",
        "at least 2",
      ),
      (
        "f = polyline_frames([v2(0,0), v2(1,0), v2(1,1)], 4, up=v3(0,1,0))",
        "only meaningful for vec3",
      ),
      (
        "f = polyline_frames([v2(0,0), v2(1,0), v2(1,1)], 4, closed=true, smooth=-1)",
        "must be >= 0",
      ),
      ("f = polyline_frames([v2(0,0), v3(1,0,0)], 4)", "mixes vec2"),
    ] {
      let err = parse_and_eval_program(src).unwrap_err().to_string();
      assert!(
        err.contains(needle),
        "expected error containing {needle:?} for {src:?}, got: {err}"
      );
    }
  }
}
