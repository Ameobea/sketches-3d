use std::{cell::RefCell, cmp::Ordering, rc::Rc};

use fxhash::FxHashMap;
use mesh::{linked_mesh::Vec3, LinkedMesh};
use nalgebra::Matrix4;

use crate::{
  builtins::trace_path::{PathTracerCallable, SegmentInterval},
  ArgRef, Callable, ErrorStack, EvalCtx, ManifoldHandle, MeshHandle, Sym, Value, Vec2,
  EMPTY_KWARGS,
};

const FRAME_EPSILON: f32 = 1e-6;
const COLLAPSE_EPSILON: f32 = 1e-5;
const GUIDE_EPSILON: f32 = 1e-6;

#[derive(Clone, Copy, Debug)]
pub enum FrameMode {
  Rmf,
  Up(Vec3),
}

#[derive(Clone, Copy, Debug)]
pub struct SpineFrame {
  pub center: Vec3,
  pub tangent: Vec3,
  pub normal: Vec3,
  pub binormal: Vec3,
}

fn calculate_tangents(points: &[Vec3]) -> Vec<Vec3> {
  let mut tangents = Vec::with_capacity(points.len());
  for i in 0..points.len() {
    let dir = if i == points.len() - 1 {
      points[i] - points[i - 1]
    } else {
      points[i + 1] - points[i]
    };
    tangents.push(dir.normalize());
  }
  tangents
}

pub(crate) fn calculate_spine_frames(
  points: &[Vec3],
  frame_mode: FrameMode,
) -> Result<Vec<SpineFrame>, ErrorStack> {
  if points.len() < 2 {
    return Err(ErrorStack::new(format!(
      "`rail_sweep` requires at least two points in the spine, found: {}",
      points.len()
    )));
  }

  let tangents = calculate_tangents(points);
  let mut frames = Vec::with_capacity(points.len());

  match frame_mode {
    FrameMode::Rmf => {
      let t0 = tangents[0];
      let mut up = Vec3::new(0., 1., 0.);
      if t0.dot(&up).abs() > 0.999 {
        up = Vec3::new(1., 0., 0.);
      }
      let mut normal = t0.cross(&up).normalize();
      let mut binormal = t0.cross(&normal).normalize();

      frames.push(SpineFrame {
        center: points[0],
        tangent: t0,
        normal,
        binormal,
      });

      for i in 1..points.len() {
        let ti = tangents[i];
        let dot = ti.dot(&normal);
        let mut proj = normal - ti * dot;
        if proj.norm_squared() < FRAME_EPSILON {
          proj = ti.cross(&binormal);
          if proj.norm_squared() < FRAME_EPSILON {
            let arbitrary = if ti.dot(&Vec3::new(0., 1., 0.)).abs() > 0.999 {
              Vec3::new(1., 0., 0.)
            } else {
              Vec3::new(0., 1., 0.)
            };
            proj = ti.cross(&arbitrary);
          }
        }
        normal = proj.normalize();
        binormal = ti.cross(&normal).normalize();

        frames.push(SpineFrame {
          center: points[i],
          tangent: ti,
          normal,
          binormal,
        });
      }
    }
    FrameMode::Up(up) => {
      if up.norm_squared() < FRAME_EPSILON {
        return Err(ErrorStack::new(
          "Invalid up vector for `rail_sweep`; expected non-zero length",
        ));
      }

      let mut prev_normal: Option<Vec3> = None;
      for (i, ti) in tangents.iter().enumerate() {
        let mut normal = ti.cross(&up);
        if normal.norm_squared() < FRAME_EPSILON {
          if let Some(prev) = prev_normal {
            let dot = ti.dot(&prev);
            let proj = prev - *ti * dot;
            if proj.norm_squared() >= FRAME_EPSILON {
              normal = proj;
            }
          }
          if normal.norm_squared() < FRAME_EPSILON {
            let fallback = if ti.dot(&Vec3::new(0., 1., 0.)).abs() > 0.999 {
              Vec3::new(1., 0., 0.)
            } else {
              Vec3::new(0., 1., 0.)
            };
            normal = ti.cross(&fallback);
          }
        }
        let normal = normal.normalize();
        let binormal = ti.cross(&normal).normalize();

        frames.push(SpineFrame {
          center: points[i],
          tangent: *ti,
          normal,
          binormal,
        });
        prev_normal = Some(normal);
      }
    }
  }

  Ok(frames)
}

fn apply_twist(normal: Vec3, binormal: Vec3, twist: f32) -> (Vec3, Vec3) {
  let (sin, cos) = twist.sin_cos();
  let rotated_normal = normal * cos + binormal * sin;
  let rotated_binormal = binormal * cos - normal * sin;
  (rotated_normal, rotated_binormal)
}

fn ring_is_collapsed(ring: &[Vec3]) -> bool {
  let first = ring[0];
  let epsilon_sq = COLLAPSE_EPSILON * COLLAPSE_EPSILON;
  ring
    .iter()
    .all(|v| (*v - first).norm_squared() <= epsilon_sq)
}

fn ring_center(ring: &[Vec3]) -> Vec3 {
  ring.iter().fold(Vec3::new(0., 0., 0.), |acc, v| acc + *v) / (ring.len() as f32)
}

fn normalize_guides(guides: &[f32]) -> Vec<f32> {
  let mut out: Vec<f32> = guides
    .iter()
    .copied()
    .filter(|v| v.is_finite())
    .map(|v| v.clamp(0.0, 1.0))
    .collect();
  out.push(0.0);
  out.push(1.0);
  out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
  out.dedup_by(|a, b| (*a - *b).abs() <= GUIDE_EPSILON);
  out
}

fn uniform_v_samples(count: usize) -> Vec<f32> {
  let denom = count as f32;
  (0..count).map(|i| i as f32 / denom).collect()
}

fn build_v_samples(
  ring_resolution: usize,
  guides: Option<&[f32]>,
  interval_weights: Option<&[f32]>,
) -> Vec<f32> {
  let Some(guides) = guides else {
    return uniform_v_samples(ring_resolution);
  };

  let guide_points = normalize_guides(guides);

  if guide_points.len() < 2 {
    return uniform_v_samples(ring_resolution);
  }

  let guide_count = guide_points.len() - 1;
  let target_count = ring_resolution.max(guide_count);
  let remaining = target_count - guide_count;
  if remaining == 0 {
    return guide_points[..guide_points.len() - 1].to_vec();
  }

  let weights = interval_weights.filter(|weights| weights.len() == guide_count);
  let mut spans = Vec::with_capacity(guide_count);
  let mut total_effective = 0.0;
  for (ix, window) in guide_points.windows(2).enumerate() {
    let span = (window[1] - window[0]).max(0.0);
    let weight = weights.map(|weights| weights[ix]).unwrap_or(1.0).max(0.0);
    let effective = span * weight;
    spans.push((span, effective));
    total_effective += effective;
  }

  if total_effective <= GUIDE_EPSILON {
    total_effective = 0.0;
    for (span, effective) in spans.iter_mut() {
      *effective = *span;
      total_effective += *effective;
    }
  }

  let mut allocations = Vec::with_capacity(guide_count);
  let mut remainders: Vec<(f32, usize)> = Vec::with_capacity(guide_count);
  let mut assigned = 0usize;

  // Apportion interior samples across intervals by length, keeping guide points fixed.
  for (ix, (span, effective)) in spans.iter().enumerate() {
    if *span <= 0.0 {
      allocations.push(0);
      remainders.push((0.0, ix));
      continue;
    }
    let exact = if total_effective > 0.0 {
      (remaining as f32) * (effective / total_effective)
    } else {
      0.0
    };
    let count = exact.floor() as usize;
    assigned += count;
    allocations.push(count);
    remainders.push((exact - count as f32, ix));
  }

  let mut leftover = remaining.saturating_sub(assigned);
  remainders.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));
  for (_, ix) in remainders {
    if leftover == 0 {
      break;
    }
    allocations[ix] += 1;
    leftover -= 1;
  }

  let mut samples = Vec::with_capacity(target_count);
  for (ix, window) in guide_points.windows(2).enumerate() {
    let start = window[0];
    let end = window[1];
    samples.push(start);

    let count = allocations[ix];
    if count == 0 {
      continue;
    }
    let step = (end - start) / ((count + 1) as f32);
    for j in 1..=count {
      samples.push(start + step * (j as f32));
    }
  }

  samples
}

fn build_interval_weights(
  guides: &[f32],
  sampler_intervals: &[Vec<SegmentInterval>],
) -> Option<Vec<f32>> {
  if guides.len() < 2 || sampler_intervals.is_empty() {
    return None;
  }

  let mut indices = vec![0usize; sampler_intervals.len()];
  let mut weights = Vec::with_capacity(guides.len() - 1);
  for window in guides.windows(2) {
    let start = window[0];
    let end = window[1];
    let mid = (start + end) * 0.5;
    let mut all_detail = true;

    for (sampler_ix, intervals) in sampler_intervals.iter().enumerate() {
      if intervals.is_empty() {
        all_detail = false;
        continue;
      }

      let mut idx = indices[sampler_ix];
      while idx + 1 < intervals.len() && mid > intervals[idx].end + GUIDE_EPSILON {
        idx += 1;
      }
      indices[sampler_ix] = idx;

      if !intervals[idx].has_detail {
        all_detail = false;
      }
    }

    weights.push(if all_detail { 1.0 } else { 0.0 });
  }

  Some(weights)
}

struct RingInfo {
  start: usize,
  count: usize,
}

fn stitch_rings(
  indices: &mut Vec<u32>,
  ring_a: &RingInfo,
  ring_b: &RingInfo,
  ring_resolution: usize,
) {
  match (ring_a.count, ring_b.count) {
    (1, 1) => {}
    (1, _) => {
      let apex = ring_a.start as u32;
      for j in 0..ring_resolution {
        let b = (ring_b.start + j) as u32;
        let c = (ring_b.start + (j + 1) % ring_resolution) as u32;
        indices.push(apex);
        indices.push(c);
        indices.push(b);
      }
    }
    (_, 1) => {
      let apex = ring_b.start as u32;
      for j in 0..ring_resolution {
        let a = (ring_a.start + j) as u32;
        let b = (ring_a.start + (j + 1) % ring_resolution) as u32;
        indices.push(a);
        indices.push(b);
        indices.push(apex);
      }
    }
    _ => {
      for j in 0..ring_resolution {
        let a = (ring_a.start + j) as u32;
        let b = (ring_a.start + (j + 1) % ring_resolution) as u32;
        let c = (ring_b.start + j) as u32;
        let d = (ring_b.start + (j + 1) % ring_resolution) as u32;

        indices.push(a);
        indices.push(b);
        indices.push(c);

        indices.push(b);
        indices.push(d);
        indices.push(c);
      }
    }
  }
}

pub fn rail_sweep(
  spine_points: &[Vec3],
  ring_resolution: usize,
  frame_mode: FrameMode,
  closed: bool,
  capped: bool,
  twist: impl Fn(usize, Vec3) -> Result<f32, ErrorStack>,
  profile: impl Fn(f32, f32, usize, usize, Vec3) -> Result<Vec2, ErrorStack>,
  profile_guides: Option<&[f32]>,
  profile_interval_weights: Option<&[f32]>,
) -> Result<LinkedMesh<()>, ErrorStack> {
  if ring_resolution < 3 {
    return Err(ErrorStack::new(
      "`rail_sweep` requires a ring resolution of at least 3",
    ));
  }

  if spine_points.len() < 2 {
    return Err(ErrorStack::new(format!(
      "`rail_sweep` requires at least two spine points, found: {}",
      spine_points.len()
    )));
  }

  let frames = calculate_spine_frames(spine_points, frame_mode)?;
  let v_samples = build_v_samples(ring_resolution, profile_guides, profile_interval_weights);
  let ring_resolution = v_samples.len();
  let mut verts: Vec<Vec3> = Vec::with_capacity(spine_points.len() * ring_resolution + 2);
  let mut ring_infos: Vec<RingInfo> = Vec::with_capacity(spine_points.len());

  let u_denom = (frames.len() - 1) as f32;

  for (u_ix, frame) in frames.iter().enumerate() {
    let u_norm = if u_denom > 0.0 {
      u_ix as f32 / u_denom
    } else {
      0.0
    };
    let twist_angle = twist(u_ix, frame.center)?;
    let (normal, binormal) = apply_twist(frame.normal, frame.binormal, twist_angle);

    let mut ring = Vec::with_capacity(ring_resolution);
    for (v_ix, v_norm) in v_samples.iter().enumerate() {
      let offset = profile(u_norm, *v_norm, u_ix, v_ix, frame.center)?;
      ring.push(frame.center + normal * offset.x + binormal * offset.y);
    }

    let is_end = u_ix == 0 || u_ix + 1 == frames.len();
    if is_end && ring_is_collapsed(&ring) {
      let start = verts.len();
      verts.push(ring_center(&ring));
      ring_infos.push(RingInfo { start, count: 1 });
    } else {
      let start = verts.len();
      verts.extend(ring);
      ring_infos.push(RingInfo {
        start,
        count: ring_resolution,
      });
    }
  }

  let mut indices: Vec<u32> = Vec::with_capacity(spine_points.len() * ring_resolution * 6);

  for i in 0..(ring_infos.len() - 1) {
    stitch_rings(
      &mut indices,
      &ring_infos[i],
      &ring_infos[i + 1],
      ring_resolution,
    );
  }

  if closed {
    stitch_rings(
      &mut indices,
      &ring_infos[ring_infos.len() - 1],
      &ring_infos[0],
      ring_resolution,
    );
  }

  if capped && !closed {
    for (ring_ix, reverse_winding) in [(0usize, true), (ring_infos.len() - 1, false)] {
      let ring_info = &ring_infos[ring_ix];
      if ring_info.count != ring_resolution {
        continue;
      }

      let center = ring_center(&verts[ring_info.start..(ring_info.start + ring_resolution)]);
      let center_ix = verts.len();
      verts.push(center);

      for v_ix in 0..ring_resolution {
        let a = center_ix as u32;
        let b = (ring_info.start + v_ix) as u32;
        let c = (ring_info.start + (v_ix + 1) % ring_resolution) as u32;

        if reverse_winding {
          indices.push(c);
          indices.push(b);
          indices.push(a);
        } else {
          indices.push(a);
          indices.push(b);
          indices.push(c);
        }
      }
    }
  }

  Ok(LinkedMesh::from_indexed_vertices(
    &verts, &indices, None, None,
  ))
}

pub(crate) fn rail_sweep_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let spine_resolution = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
      let ring_resolution = arg_refs[1].resolve(args, kwargs).as_int().unwrap();
      if spine_resolution < 2 {
        return Err(ErrorStack::new(format!(
          "Invalid spine_resolution for `rail_sweep`; expected >= 2, found: {spine_resolution}"
        )));
      }
      if ring_resolution < 3 {
        return Err(ErrorStack::new(format!(
          "Invalid ring_resolution for `rail_sweep`; expected >= 3, found: {ring_resolution}"
        )));
      }
      let spine_resolution = spine_resolution as usize;
      let ring_resolution = ring_resolution as usize;

      let spine = arg_refs[2].resolve(args, kwargs);
      let profile = arg_refs[3].resolve(args, kwargs).as_callable().unwrap();
      let frame_mode_val = arg_refs[4].resolve(args, kwargs);
      let twist_val = arg_refs[5].resolve(args, kwargs);
      let closed = arg_refs[6].resolve(args, kwargs).as_bool().unwrap();
      let capped = arg_refs[7].resolve(args, kwargs).as_bool().unwrap();
      let profile_samplers_val = arg_refs[8].resolve(args, kwargs);

      struct ProfileGuideData {
        guides: Vec<f32>,
        interval_weights: Option<Vec<f32>>,
      }

      fn collect_profile_guides(
        ctx: &EvalCtx,
        value: &Value,
      ) -> Result<Option<ProfileGuideData>, ErrorStack> {
        struct SamplerData {
          guides: Vec<f32>,
          intervals: Vec<SegmentInterval>,
        }

        fn sampler_data(callable: &Callable) -> Option<SamplerData> {
          match callable {
            Callable::Dynamic { inner, .. } => inner
              .as_any()
              .downcast_ref::<PathTracerCallable>()
              .map(|tracer| SamplerData {
                guides: tracer.critical_t_values(),
                intervals: tracer.segment_intervals(),
              }),
            _ => None,
          }
        }

        let err_expected = || {
          ErrorStack::new(format!(
            "Invalid profile_samplers argument for `rail_sweep`; expected a trace_path sampler or \
             sequence of samplers",
          ))
        };

        let mut samplers = Vec::new();
        match value {
          Value::Nil => return Ok(None),
          Value::Callable(callable) => {
            let data = sampler_data(callable).ok_or_else(err_expected)?;
            if data.guides.is_empty() {
              return Ok(None);
            }
            samplers.push(data);
          }
          Value::Sequence(seq) => {
            for (ix, res) in seq.consume(ctx).enumerate() {
              let val = res?;
              let cb = val.as_callable().ok_or_else(|| {
                ErrorStack::new(format!(
                  "Expected trace_path sampler in profile_samplers sequence for `rail_sweep` at \
                   index {ix}, found: {val:?}"
                ))
              })?;
              let data = sampler_data(cb).ok_or_else(err_expected)?;
              if data.guides.is_empty() {
                return Ok(None);
              }
              samplers.push(data);
            }
          }
          _ => {
            return Err(ErrorStack::new(format!(
              "Invalid profile_samplers argument for `rail_sweep`; expected a trace_path sampler \
               or sequence of samplers, found: {value:?}"
            )))
          }
        }

        if samplers.is_empty() {
          return Ok(None);
        }

        let mut raw_guides = Vec::new();
        let mut intervals = Vec::new();
        for sampler in samplers {
          raw_guides.extend(sampler.guides);
          intervals.push(sampler.intervals);
        }

        let guides = normalize_guides(&raw_guides);
        let interval_weights = build_interval_weights(&guides, &intervals);

        Ok(Some(ProfileGuideData {
          guides,
          interval_weights,
        }))
      }

      let profile_guide_data = collect_profile_guides(ctx, &profile_samplers_val)?;

      fn resample_spine_points(
        points: &[Vec3],
        spine_resolution: usize,
      ) -> Result<Vec<Vec3>, ErrorStack> {
        if points.len() < 2 {
          return Err(ErrorStack::new(format!(
            "`rail_sweep` requires at least two spine points, found: {}",
            points.len()
          )));
        }
        if points.len() == spine_resolution {
          return Ok(points.to_vec());
        }

        let mut cumulative = Vec::with_capacity(points.len());
        cumulative.push(0.0);
        for i in 1..points.len() {
          let seg_len = (points[i] - points[i - 1]).norm();
          cumulative.push(cumulative[i - 1] + seg_len);
        }

        let total = *cumulative.last().unwrap_or(&0.0);
        if total <= 0.0 {
          return Err(ErrorStack::new(
            "Cannot resample `rail_sweep` spine with zero length",
          ));
        }

        let mut out = Vec::with_capacity(spine_resolution);
        for i in 0..spine_resolution {
          let target = total * (i as f32) / ((spine_resolution - 1) as f32);
          let mut seg_ix = 0;
          while seg_ix + 1 < cumulative.len() && cumulative[seg_ix + 1] < target {
            seg_ix += 1;
          }
          let seg_len = cumulative[seg_ix + 1] - cumulative[seg_ix];
          let local_t = if seg_len <= 0.0 {
            0.0
          } else {
            (target - cumulative[seg_ix]) / seg_len
          };
          out.push(points[seg_ix].lerp(&points[seg_ix + 1], local_t));
        }

        Ok(out)
      }

      let spine_points = if let Some(seq) = spine.as_sequence() {
        let raw_points: Vec<Vec3> = seq
          .consume(ctx)
          .map(|res| match res {
            Ok(Value::Vec3(v)) => Ok(v),
            Ok(val) => Err(ErrorStack::new(format!(
              "Expected Vec3 in spine sequence passed to `rail_sweep`, found: {val:?}"
            ))),
            Err(err) => Err(err),
          })
          .collect::<Result<_, _>>()?;

        if raw_points.len() < 2 {
          return Err(ErrorStack::new(format!(
            "`rail_sweep` requires at least two spine points, found: {}",
            raw_points.len()
          )));
        }

        if raw_points.len() != spine_resolution {
          resample_spine_points(&raw_points, spine_resolution)?
        } else {
          raw_points
        }
      } else if let Some(cb) = spine.as_callable() {
        let mut points = Vec::with_capacity(spine_resolution);
        let denom = (spine_resolution - 1) as f32;
        for i in 0..spine_resolution {
          let u = if denom > 0.0 { i as f32 / denom } else { 0.0 };
          let out = ctx
            .invoke_callable(cb, &[Value::Float(u)], EMPTY_KWARGS)
            .map_err(|err| {
              err.wrap("Error calling user-provided cb passed to `spine` arg in `rail_sweep`")
            })?;
          let v = out.as_vec3().ok_or_else(|| {
            ErrorStack::new(format!(
              "Expected Vec3 from user-provided cb passed to `spine` arg in `rail_sweep`, found: \
               {out:?}"
            ))
          })?;
          points.push(*v);
        }
        points
      } else {
        return Err(ErrorStack::new(format!(
          "Invalid spine argument for `rail_sweep`; expected Sequence or Callable, found: \
           {spine:?}"
        )));
      };

      let frame_mode = if let Some(v) = frame_mode_val.as_vec3() {
        FrameMode::Up(*v)
      } else if let Some(mode) = frame_mode_val.as_str() {
        if mode.eq_ignore_ascii_case("rmf") {
          FrameMode::Rmf
        } else {
          return Err(ErrorStack::new(format!(
            "Invalid frame_mode argument for `rail_sweep`; expected \"rmf\" or Vec3, found: \
             {mode:?}"
          )));
        }
      } else {
        return Err(ErrorStack::new(format!(
          "Invalid frame_mode argument for `rail_sweep`; expected \"rmf\" or Vec3, found: \
           {frame_mode_val:?}"
        )));
      };

      enum Twist<'a> {
        Const(f32),
        Dyn(&'a Rc<Callable>),
      }

      let twist = if let Some(f) = twist_val.as_float() {
        Twist::Const(f)
      } else if let Some(cb) = twist_val.as_callable() {
        Twist::Dyn(cb)
      } else {
        return Err(ErrorStack::new(format!(
          "Invalid twist argument for `rail_sweep`; expected Numeric or Callable, found: \
           {twist_val:?}"
        )));
      };

      fn build_twist_callable<'a>(
        ctx: &'a EvalCtx,
        get_twist: &'a Rc<Callable>,
      ) -> impl Fn(usize, Vec3) -> Result<f32, ErrorStack> + 'a {
        move |i, pos| {
          let out = ctx
            .invoke_callable(
              get_twist,
              &[Value::Int(i as i64), Value::Vec3(pos)],
              EMPTY_KWARGS,
            )
            .map_err(|err| {
              err.wrap("Error calling user-provided cb passed to `twist` arg in `rail_sweep`")
            })?;
          out.as_float().ok_or_else(|| {
            ErrorStack::new(format!(
              "Expected Float from user-provided cb passed to `twist` arg in `rail_sweep`, found: \
               {out:?}"
            ))
          })
        }
      }

      fn build_profile_callable<'a>(
        ctx: &'a EvalCtx,
        profile: &'a Rc<Callable>,
      ) -> impl Fn(f32, f32, usize, usize, Vec3) -> Result<Vec2, ErrorStack> + 'a {
        move |u, v, u_ix, v_ix, center| {
          let out = ctx
            .invoke_callable(
              profile,
              &[
                Value::Float(u),
                Value::Float(v),
                Value::Int(u_ix as i64),
                Value::Int(v_ix as i64),
                Value::Vec3(center),
              ],
              EMPTY_KWARGS,
            )
            .map_err(|err| {
              err.wrap("Error calling user-provided cb passed to `profile` arg in `rail_sweep`")
            })?;
          out.as_vec2().copied().ok_or_else(|| {
            ErrorStack::new(format!(
              "Expected Vec2 from user-provided cb passed to `profile` arg in `rail_sweep`, \
               found: {out:?}"
            ))
          })
        }
      }

      let (profile_guides, profile_weights) = match profile_guide_data.as_ref() {
        Some(data) => (
          Some(data.guides.as_slice()),
          data.interval_weights.as_deref(),
        ),
        None => (None, None),
      };

      let mesh = match twist {
        Twist::Const(twist) => rail_sweep(
          &spine_points,
          ring_resolution,
          frame_mode,
          closed,
          capped,
          |_, _| Ok(twist),
          build_profile_callable(ctx, profile),
          profile_guides,
          profile_weights,
        )?,
        Twist::Dyn(get_twist) => rail_sweep(
          &spine_points,
          ring_resolution,
          frame_mode,
          closed,
          capped,
          build_twist_callable(ctx, get_twist),
          build_profile_callable(ctx, profile),
          profile_guides,
          profile_weights,
        )?,
      };

      Ok(Value::Mesh(Rc::new(MeshHandle {
        mesh: Rc::new(mesh),
        transform: Matrix4::identity(),
        manifold_handle: Rc::new(ManifoldHandle::new_empty()),
        aabb: RefCell::new(None),
        trimesh: RefCell::new(None),
        material: None,
      })))
    }
    _ => unimplemented!(),
  }
}

#[cfg(test)]
mod tests {
  use super::{build_v_samples, rail_sweep, FrameMode};
  use mesh::linked_mesh::Vec3;

  use crate::Vec2;

  #[test]
  fn test_rail_sweep_basic_counts() {
    let spine = vec![Vec3::new(0., 0., 0.), Vec3::new(0., 0., 2.)];
    let mesh = rail_sweep(
      &spine,
      4,
      FrameMode::Rmf,
      false,
      false,
      |_, _| Ok(0.0),
      |_, v_norm, _, v_ix, _| {
        let angle = v_norm * std::f32::consts::TAU;
        let radius = if v_ix % 2 == 0 { 1.0 } else { 0.5 };
        Ok(Vec2::new(angle.cos() * radius, angle.sin() * radius))
      },
      None,
      None,
    )
    .unwrap();

    assert_eq!(mesh.vertices.len(), 8);
    assert_eq!(mesh.faces.len(), 8);
  }

  #[test]
  fn test_rail_sweep_collapsed_endpoints() {
    let spine = vec![
      Vec3::new(0., 0., 0.),
      Vec3::new(0., 0., 1.),
      Vec3::new(0., 0., 2.),
    ];
    let ring_resolution = 6;
    let mesh = rail_sweep(
      &spine,
      ring_resolution,
      FrameMode::Rmf,
      false,
      true,
      |_, _| Ok(0.0),
      |_, v_norm, u_ix, _v_ix, _| {
        if u_ix == 0 || u_ix == 2 {
          Ok(Vec2::new(0.0, 0.0))
        } else {
          let angle = v_norm * std::f32::consts::TAU;
          Ok(Vec2::new(angle.cos(), angle.sin()))
        }
      },
      None,
      None,
    )
    .unwrap();

    assert_eq!(mesh.vertices.len(), ring_resolution + 2);
    assert_eq!(mesh.faces.len(), ring_resolution * 2);
  }

  #[test]
  fn test_rail_sweep_guide_sampling_includes_critical_points() {
    let guides = vec![0.5];
    let samples = build_v_samples(6, Some(&guides), None);

    assert_eq!(samples.len(), 6);
    assert!((samples[0] - 0.0).abs() < 1e-6);
    assert!((samples[3] - 0.5).abs() < 1e-6);
    assert!(samples.iter().all(|v| *v >= 0.0 && *v < 1.0));
  }

  #[test]
  fn test_rail_sweep_guide_sampling_skips_straight_segments() {
    let guides = vec![0.0, 0.5, 1.0];
    let weights = vec![0.0, 1.0];
    let samples = build_v_samples(6, Some(&guides), Some(&weights));

    assert_eq!(samples.len(), 6);
    assert!((samples[0] - 0.0).abs() < 1e-6);
    assert!((samples[1] - 0.5).abs() < 1e-6);
    assert!((samples[2] - 0.6).abs() < 1e-6);
    assert!((samples[3] - 0.7).abs() < 1e-6);
    assert!((samples[4] - 0.8).abs() < 1e-6);
    assert!((samples[5] - 0.9).abs() < 1e-6);
  }
}
