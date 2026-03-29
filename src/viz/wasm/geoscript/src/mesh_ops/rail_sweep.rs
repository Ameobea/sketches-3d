use std::{cell::RefCell, f32::consts::PI, rc::Rc};

use bitvec::prelude::*;

use fxhash::FxHashMap;
use mesh::{linked_mesh::Vec3, slotmap_utils::vkey, LinkedMesh};
use nalgebra::Matrix4;

use crate::{
  builtins::trace_path::{
    as_path_sampler, build_topology_samples, normalize_guides, normalize_path_sampler_guides,
  },
  ArgRef, Callable, DynamicCallable, ErrorStack, EvalCtx, ManifoldHandle, MeshHandle, Sym, Value,
  Vec2, EMPTY_KWARGS,
};

use super::adaptive_sampler::adaptive_sample_fallible;
use super::fku_stitch::{
  dp_stitch_presampled, should_use_fku, snap_critical_points, stitch_apex_to_row,
  uniform_stitch_rows,
};
use super::helpers::{compute_centroid, vertices_are_collapsed};

const FRAME_EPSILON: f32 = 1e-6;
const COLLAPSE_EPSILON: f32 = 1e-5;
/// Epsilon for comparing twist values to determine if they're effectively constant.
const TWIST_CONST_EPSILON: f32 = 1e-6;

enum Twist {
  Const(f32),
  Presampled(Vec<f32>),
}

impl Twist {
  fn value_at(&self, index: usize) -> f32 {
    match self {
      Twist::Const(val) => *val,
      Twist::Presampled(values) => values[index],
    }
  }
}

#[derive(Clone, Copy, Debug)]
pub enum FrameMode {
  /// Rotation-minimizing frames
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

struct RingInfo {
  start: usize,
  count: usize,
  /// Plane frame for capping, only stored for first and last rings when capping is enabled.
  cap_frame: Option<super::tessellate_polygon::PlaneFrame>,
  /// Whether the edges connecting vertices within this ring should be marked sharp.
  sharp: bool,
  /// The parametric t-values used to sample this ring's profile.
  /// Used by the DP stitcher to penalize connecting vertices with dissimilar t-values.
  t_values: Option<Vec<f32>>,
  /// Bitmask indicating which t-values correspond to critical points (e.g. sharp seam vertices).
  /// Used by the DP stitcher to bias towards connecting critical vertices to each other.
  critical_mask: Option<BitVec>,
}

struct DynamicProfileData {
  sampler: Rc<Callable>,
  critical_points: Vec<f32>,
  sharp: bool,
  /// When Some(true), forces adaptive sampling for this ring.
  /// When Some(false), disables adaptive sampling even if global is enabled.
  /// When None, uses the global `adaptive_profile_sampling` setting.
  adaptive: Option<bool>,
  /// Non-zero when the profile's t-parameterization has been rotated so that the first
  /// real critical point aligns to t=0. The sampler must be called at
  /// `(t + rotation_offset).rem_euclid(1.0)` to recover the original t.
  rotation_offset: f32,
}

struct RingContext {
  center: Vec3,
  normal: Vec3,
  binormal: Vec3,
  profile_data: DynamicProfileData,
  cap_frame: Option<super::tessellate_polygon::PlaneFrame>,
  collapsed: bool,
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

/// Collects critical t-values (guide points) from path samplers for use with dynamic profiles.
/// Only extracts the guide points, not segment intervals (which are only used in the static
/// profile_samplers path for straight-segment optimization).
fn collect_path_sampler_guides(
  ctx: &EvalCtx,
  value: &Value,
  label: &str,
) -> Result<Option<Vec<f32>>, ErrorStack> {
  fn sampler_guides(callable: &Callable) -> Option<Vec<f32>> {
    as_path_sampler(callable).map(|s| s.critical_t_values())
  }

  let err_expected = || {
    ErrorStack::new(format!(
      "Invalid {label}; expected a trace_path sampler or sequence of samplers",
    ))
  };

  let mut all_guides = Vec::new();
  match value {
    Value::Nil => return Ok(None),
    Value::Callable(callable) => {
      let guides = sampler_guides(callable).ok_or_else(err_expected)?;
      if guides.is_empty() {
        return Ok(None);
      }
      all_guides.extend(guides);
    }
    Value::Sequence(seq) => {
      for (ix, res) in seq.consume(ctx).enumerate() {
        let val = res?;
        let cb = val.as_callable().ok_or_else(|| {
          ErrorStack::new(format!(
            "Expected trace_path sampler in {label} sequence at index {ix}, found: {val:?}"
          ))
        })?;
        let guides = sampler_guides(cb).ok_or_else(err_expected)?;
        if guides.is_empty() {
          return Ok(None);
        }
        all_guides.extend(guides);
      }
    }
    _ => {
      return Err(ErrorStack::new(format!(
        "Invalid {label}; expected a trace_path sampler or sequence of samplers, found: {value:?}"
      )))
    }
  }

  if all_guides.is_empty() {
    return Ok(None);
  }

  Ok(Some(normalize_guides(&all_guides)))
}

fn extract_dynamic_profile_data(
  ctx: &EvalCtx,
  value: Value,
) -> Result<DynamicProfileData, ErrorStack> {
  if let Some(callable) = value.as_callable() {
    if let Some(sampler) = as_path_sampler(callable) {
      let (critical_points, rotation_offset) = normalize_path_sampler_guides(sampler);
      return Ok(DynamicProfileData {
        sampler: Rc::clone(callable),
        critical_points,
        sharp: false,
        adaptive: None,
        rotation_offset,
      });
    }
    return Ok(DynamicProfileData {
      sampler: Rc::clone(callable),
      critical_points: vec![0., 1.],
      sharp: false,
      adaptive: None,
      rotation_offset: 0.0,
    });
  }

  if let Some(map) = value.as_map() {
    let valid_keys = &["sampler", "path_samplers", "sharp", "adaptive"];
    for key in map.keys() {
      if !valid_keys.contains(&key.as_str()) {
        return Err(ErrorStack::new(format!(
          "dynamic_profile map contains unexpected key '{key}'; expected one of: {}",
          valid_keys.join(", ")
        )));
      }
    }

    let sampler_val = map.get("sampler").ok_or_else(|| {
      ErrorStack::new("dynamic_profile map requires 'sampler' key with callable value")
    })?;
    let sampler = sampler_val.as_callable().ok_or_else(|| {
      ErrorStack::new("dynamic_profile map requires 'sampler' key with callable value")
    })?;

    let (critical_points, rotation_offset) = match map.get("path_samplers") {
      Some(val) => (
        collect_path_sampler_guides(ctx, val, "dynamic_profile path_samplers")?
          .unwrap_or_else(|| vec![0., 1.]),
        0.0,
      ),
      None => {
        // check if the sampler itself is a path sampler with guides
        if let Some(path_samp) = as_path_sampler(sampler) {
          normalize_path_sampler_guides(path_samp)
        } else {
          (vec![0., 1.], 0.0)
        }
      }
    };

    let sharp = match map.get("sharp") {
      Some(val) => val
        .as_bool()
        .ok_or_else(|| ErrorStack::new("dynamic_profile 'sharp' key must be a boolean value"))?,
      None => false,
    };

    let adaptive = match map.get("adaptive") {
      Some(val) => Some(val.as_bool().ok_or_else(|| {
        ErrorStack::new(format!(
          "dynamic_profile 'adaptive' key must be a boolean; found: {val:?}"
        ))
      })?),
      None => None,
    };

    return Ok(DynamicProfileData {
      sampler: Rc::clone(sampler),
      critical_points,
      sharp,
      adaptive,
      rotation_offset,
    });
  }

  Err(ErrorStack::new(
    "dynamic_profile must return a callable or a map with 'sampler' key",
  ))
}

/// Validates that a dynamic profile sampler's path topology is suitable for rail_sweep.
///
/// Rail sweep profiles must be a single continuous path (one subpath). Multiple subpaths
/// indicate disconnected curves or a shape with holes, which would produce a garbled mesh
/// since the ring stitching treats all sampled points as a single closed loop.
///
/// Only checks path samplers that expose topology info; black-box callables are skipped.
fn validate_profile_topology(sampler: &Callable, u_ix: usize, u: f32) -> Result<(), ErrorStack> {
  let Some(path_sampler) = as_path_sampler(sampler) else {
    return Ok(());
  };
  let Some(topology) = path_sampler.subpath_topology() else {
    return Ok(());
  };

  if topology.len() <= 1 {
    return Ok(());
  }

  let mut detail = String::new();
  for (i, sp) in topology.iter().enumerate() {
    let status = if sp.closed { "closed" } else { "open" };
    detail.push_str(&format!(
      "\n  subpath {i}: {status}, {seg_count} segment{s}",
      seg_count = sp.segment_count,
      s = if sp.segment_count == 1 { "" } else { "s" },
    ));
  }

  Err(ErrorStack::new(format!(
    "Invalid dynamic_profile at spine index u_ix={u_ix} (u={u:.4}): path has {n} subpaths, but \
     rail_sweep profiles must be a single continuous path (1 subpath).\nMultiple subpaths \
     typically indicate a shape with holes or disconnected curves, which cannot be swept into a \
     valid mesh.{detail}",
    n = topology.len(),
  )))
}

/// Samples the 2D profile offset at the given t value.
fn sample_profile_offset(ctx: &EvalCtx, ring: &RingContext, v: f32) -> Result<Vec2, ErrorStack> {
  // If the critical points were rotated to align t=0 with a real feature, undo the rotation
  // before calling the sampler so it receives t-values in its original parameterization.
  let v = if ring.profile_data.rotation_offset != 0.0 {
    (v + ring.profile_data.rotation_offset).rem_euclid(1.0)
  } else {
    v
  };
  let offset_2d = ctx
    .invoke_callable(&ring.profile_data.sampler, &[Value::Float(v)], EMPTY_KWARGS)
    .map_err(|err| err.wrap("Error calling user-provided sampler returned by `dynamic_profile`"))?;
  offset_2d.as_vec2().copied().ok_or_else(|| {
    ErrorStack::new(format!(
      "Profile sampler must return Vec2, found: {offset_2d:?}"
    ))
  })
}

/// Samples the 3D position by applying the profile offset to the ring's coordinate frame.
fn sample_profile_at(ctx: &EvalCtx, ring: &RingContext, v: f32) -> Result<Vec3, ErrorStack> {
  let offset = sample_profile_offset(ctx, ring, v)?;
  Ok(ring.center + ring.normal * offset.x + ring.binormal * offset.y)
}

fn ring_is_collapsed_dynamic(ctx: &EvalCtx, ring: &RingContext) -> Result<bool, ErrorStack> {
  let samples = [0.0, 0.25, 0.5, 0.75];
  let mut first: Option<Vec3> = None;
  let epsilon_sq = COLLAPSE_EPSILON * COLLAPSE_EPSILON;
  for t in samples {
    let p = sample_profile_at(ctx, ring, t)?;
    if let Some(first) = first {
      if (p - first).norm_squared() > epsilon_sq {
        return Ok(false);
      }
    } else {
      first = Some(p);
    }
  }
  Ok(true)
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
  spine_u_values: Option<&[f32]>,
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
  let v_samples = build_topology_samples(
    ring_resolution,
    profile_guides,
    profile_interval_weights,
    false,
  );
  let ring_resolution = v_samples.len();
  let mut verts: Vec<Vec3> = Vec::with_capacity(spine_points.len() * ring_resolution + 2);
  let mut ring_infos: Vec<RingInfo> = Vec::with_capacity(spine_points.len());

  let u_denom = (frames.len() - 1) as f32;

  for (u_ix, frame) in frames.iter().enumerate() {
    let u_norm = match spine_u_values {
      Some(values) => values[u_ix],
      None => {
        if u_denom > 0. {
          u_ix as f32 / u_denom
        } else {
          0.
        }
      }
    };
    let twist_angle = twist(u_ix, frame.center)?;
    let (normal, binormal) = apply_twist(frame.normal, frame.binormal, twist_angle);

    let mut ring = Vec::with_capacity(ring_resolution);
    for (v_ix, v_norm) in v_samples.iter().enumerate() {
      let offset = profile(u_norm, *v_norm, u_ix, v_ix, frame.center)?;
      ring.push(frame.center + normal * offset.x + binormal * offset.y);
    }

    let is_end = u_ix == 0 || u_ix + 1 == frames.len();

    let cap_frame = if capped && !closed && is_end {
      Some(super::tessellate_polygon::PlaneFrame {
        center: frame.center,
        u_axis: normal,
        v_axis: binormal,
      })
    } else {
      None
    };

    // TODO: in addition to handling the case where all vertices collapse to a point,
    // also handle cases where they collapse to a line segment.
    //
    // CGAL's triangulation can't handle these cases and returns an error, but we should be able to
    // do something intelligent for this case.

    if vertices_are_collapsed(&ring) {
      let start = verts.len();
      verts.push(compute_centroid(&ring));
      ring_infos.push(RingInfo {
        start,
        count: 1,
        cap_frame: None,
        sharp: false,
        t_values: None,
        critical_mask: None,
      });
    } else {
      let start = verts.len();
      verts.extend(ring);
      ring_infos.push(RingInfo {
        start,
        count: ring_resolution,
        cap_frame,
        sharp: false,
        t_values: None,
        critical_mask: None,
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
    let r_last = &ring_infos[ring_infos.len() - 1];
    let r_first = &ring_infos[0];
    if r_last.count > 1 && r_first.count > 1 {
      // Use DP stitching to find the optimal rotational alignment between the last and
      // first rings.  Without this, any RMF phase accumulated over the closed loop causes
      // twisted / backwards-facing triangles at the seam.
      let pts_last = &verts[r_last.start..r_last.start + r_last.count];
      let pts_first = &verts[r_first.start..r_first.start + r_first.count];
      dp_stitch_presampled(
        pts_last,
        pts_first,
        None,
        None,
        None,
        None,
        r_last.start,
        r_first.start,
        true,
        &mut indices,
      );
    } else {
      stitch_rings(&mut indices, r_last, r_first, ring_resolution);
    }
  }

  if capped && !closed {
    for (ring_ix, reverse_winding) in [(0usize, false), (ring_infos.len() - 1, true)] {
      let ring_info = &ring_infos[ring_ix];
      if ring_info.count != ring_resolution {
        continue;
      }

      let ring_slice = &verts[ring_info.start..(ring_info.start + ring_resolution)];
      let frame = ring_info
        .cap_frame
        .as_ref()
        .expect("cap_frame should always be set for end rings when capping is enabled");
      let cap_indices = super::tessellate_polygon::tessellate_ring_cap_with_frame(
        ring_slice,
        ring_info.start,
        reverse_winding,
        frame,
      )?;
      indices.extend(cap_indices);
    }
  }

  Ok(LinkedMesh::from_indexed_vertices(
    &verts, &indices, None, None,
  ))
}

fn rail_sweep_dynamic(
  ctx: &EvalCtx,
  spine_points: &[Vec3],
  ring_resolution: usize,
  frame_mode: FrameMode,
  closed: bool,
  capped: bool,
  twist: &Twist,
  dynamic_profile_cb: &Rc<Callable>,
  fku_stitching: bool,
  spine_u_values: Option<&[f32]>,
  adaptive_profile_sampling: bool,
) -> Result<LinkedMesh<()>, ErrorStack> {
  let frames = calculate_spine_frames(spine_points, frame_mode)?;
  let u_denom = (frames.len() - 1) as f32;

  let mut ring_contexts: Vec<RingContext> = Vec::with_capacity(frames.len());
  for (u_ix, frame) in frames.iter().enumerate() {
    let u = match spine_u_values {
      Some(values) => values[u_ix],
      None => {
        if u_denom > 0. {
          u_ix as f32 / u_denom
        } else {
          0.
        }
      }
    };
    let twist_angle = twist.value_at(u_ix);
    let (normal, binormal) = apply_twist(frame.normal, frame.binormal, twist_angle);

    let result = ctx.invoke_callable(
      dynamic_profile_cb,
      &[Value::Float(u), Value::Int(u_ix as i64)],
      EMPTY_KWARGS,
    )?;
    let mut profile_data = extract_dynamic_profile_data(ctx, result)?;
    validate_profile_topology(&profile_data.sampler, u_ix, u)?;
    if profile_data.critical_points.len() < 2 {
      profile_data.critical_points = vec![0.0, 1.0];
    }

    let is_end = u_ix == 0 || u_ix + 1 == frames.len();
    let cap_frame = if capped && !closed && is_end {
      Some(super::tessellate_polygon::PlaneFrame {
        center: frame.center,
        u_axis: normal,
        v_axis: binormal,
      })
    } else {
      None
    };

    let mut ring = RingContext {
      center: frame.center,
      normal,
      binormal,
      profile_data,
      cap_frame,
      collapsed: false,
    };
    ring.collapsed = ring_is_collapsed_dynamic(ctx, &ring)?;
    ring_contexts.push(ring);
  }

  let mut verts: Vec<Vec3> = Vec::new();
  let mut indices: Vec<u32> = Vec::new();

  let sampled_rings: Vec<_> = ring_contexts
    .into_iter()
    .map(|ring| {
      let start = verts.len();
      let (count, t_values, critical_mask) = if ring.collapsed {
        let apex = sample_profile_at(ctx, &ring, 0.0)?;
        verts.push(apex);
        (1, None, None)
      } else {
        let use_adaptive = ring
          .profile_data
          .adaptive
          .unwrap_or(adaptive_profile_sampling);

        let samples = if use_adaptive {
          // TODO: maybe define this in terms of perimeter / ring_resolution?
          let min_segment_length = 1e-5;

          adaptive_sample_fallible(
            ring_resolution,
            &ring.profile_data.critical_points,
            |t| sample_profile_offset(ctx, &ring, t),
            min_segment_length,
          )?
        } else {
          // Use existing topology-aware uniform sampling
          let use_fku = should_use_fku(fku_stitching, ring_resolution, ring_resolution);
          let base_samples = build_topology_samples(ring_resolution, None, None, false);
          if use_fku {
            snap_critical_points(
              &base_samples,
              &ring.profile_data.critical_points,
              ring_resolution,
            )
          } else {
            base_samples
          }
        };

        // TODO: this is gross; we should at least do a binary search instead of iterating every
        // sample for every critical point
        //
        // or better yet, track the critical status of each samples and plumb it through

        // Build critical mask: mark each sample that
        // coincides with a critical point.
        let crit_pts = &ring.profile_data.critical_points;
        let crit_mask = if crit_pts.is_empty() {
          None
        } else {
          let epsilon = 1e-6;
          let mut mask = bitvec![0; samples.len()];
          for (i, &t) in samples.iter().enumerate() {
            if crit_pts.iter().any(|&c| (t - c).abs() < epsilon) {
              mask.set(i, true);
            }
          }
          Some(mask)
        };

        let t_vals = samples.clone();
        for v in samples {
          verts.push(sample_profile_at(ctx, &ring, v)?);
        }
        (verts.len() - start, Some(t_vals), crit_mask)
      };

      Ok(RingInfo {
        start,
        count,
        cap_frame: ring.cap_frame.clone(),
        sharp: ring.profile_data.sharp,
        t_values,
        critical_mask,
      })
    })
    .collect::<Result<Vec<_>, ErrorStack>>()?;

  let stitch_pair = |idx_a: usize, idx_b: usize, indices: &mut Vec<u32>| {
    let r_a = &sampled_rings[idx_a];
    let r_b = &sampled_rings[idx_b];

    // apex to apex; super degenerate case we can't even represent in `LinkedMesh`
    if r_a.count == 1 && r_b.count == 1 {
      return;
    }

    // apex to ring
    if r_a.count == 1 {
      stitch_apex_to_row(r_a.start, r_b.start, r_b.count, true, true, true, indices);
      return;
    }
    if r_b.count == 1 {
      stitch_apex_to_row(r_b.start, r_a.start, r_a.count, true, false, false, indices);
      return;
    }

    // default case: ring to ring
    let use_fku = should_use_fku(fku_stitching, r_a.count, r_b.count);
    if use_fku {
      let pts_a = &verts[r_a.start..r_a.start + r_a.count];
      let pts_b = &verts[r_b.start..r_b.start + r_b.count];
      dp_stitch_presampled(
        pts_a,
        pts_b,
        r_a.t_values.as_deref(),
        r_b.t_values.as_deref(),
        r_a.critical_mask.as_deref(),
        r_b.critical_mask.as_deref(),
        r_a.start,
        r_b.start,
        true,
        indices,
      );
    } else {
      let count = r_a.count.min(r_b.count);
      uniform_stitch_rows(r_a.start, r_b.start, count, true, true, indices);
    }
  };

  if sampled_rings.len() >= 2 {
    for i in 0..(sampled_rings.len() - 1) {
      stitch_pair(i, i + 1, &mut indices);
    }
  }

  if closed && sampled_rings.len() >= 2 {
    let last_ix = sampled_rings.len() - 1;
    stitch_pair(last_ix, 0, &mut indices);
  }

  if capped && !closed && sampled_rings.len() >= 2 {
    for (ring_ix, reverse_winding) in [(0usize, false), (sampled_rings.len() - 1, true)] {
      let ring_info = &sampled_rings[ring_ix];
      if ring_info.count < 3 {
        continue;
      }

      let ring_slice = &verts[ring_info.start..(ring_info.start + ring_info.count)];
      let frame = ring_info
        .cap_frame
        .as_ref()
        .expect("cap_frame should always be set for end rings when capping is enabled");
      let cap_indices = super::tessellate_polygon::tessellate_ring_cap_with_frame(
        ring_slice,
        ring_info.start,
        reverse_winding,
        frame,
      )?;
      indices.extend(cap_indices);
    }
  }

  let mut mesh = LinkedMesh::from_indexed_vertices(&verts, &indices, None, None);

  // For any rings that were explicitly marked sharp, we annotate their edges to preserve that
  // information for when shading normals are eventually computed
  //
  // `from_indexed_vertices` guarantees that `VertexKey`s map back to original vtx indices as:
  // `{ ix: vtx_ix + 1, version: 1 }`
  for ring in sampled_rings.iter() {
    if !ring.sharp {
      continue;
    }

    for j in 0..ring.count {
      let v0_ix = ring.start + j;
      let v1_ix = ring.start + (j + 1) % ring.count;
      let vkeys = [vkey(v0_ix as u32 + 1, 1), vkey(v1_ix as u32 + 1, 1)];
      if let Some(edge_key) = mesh.get_edge_key(vkeys) {
        if let Some(edge) = mesh.edges.get_mut(edge_key) {
          edge.sharp = true;
        }
      }
    }
  }

  Ok(mesh)
}

fn uniform_nodes(n: usize) -> Vec<f32> {
  let denom = (n - 1) as f32;
  (0..n)
    .map(|i| if denom > 0.0 { i as f32 / denom } else { 0.0 })
    .collect()
}

fn is_adaptive_spine_scheme(scheme: &Value) -> bool {
  match scheme {
    Value::String(s) => s.eq_ignore_ascii_case("adaptive"),
    Value::Map(map) => {
      if let Some(type_val) = map.get("type") {
        if let Some(s) = type_val.as_str() {
          return s.eq_ignore_ascii_case("adaptive");
        }
      }
      false
    }
    _ => false,
  }
}

/// Computes Chebyshev nodes mapped to [0, 1].  These nodes are denser near the endpoints and
/// sparser in the middle.
fn chebyshev_nodes(n: usize) -> Vec<f32> {
  (0..n)
    .map(|k| 0.5 * (1. - (PI * (2. * (k as f32) + 1.) / (2. * (n as f32))).cos()))
    .collect()
}

/// Computes bevel-friendly sampling nodes for `n` points in [0, 1].
///
/// Splits the domain into three regions: a dense bevel zone at each end and a uniform middle.
/// Within each bevel zone, samples follow a power-curve distribution that concentrates them
/// toward the true endpoints (the corners of the bevel), giving a natural density falloff
/// toward the middle region.
///
/// The first and last samples are slightly offset from 0 and 1, which is important for
/// superellipse profiles where t=0 or t=1 would give zero radius.
///
/// Parameters:
/// - `n`: total number of samples
/// - `bevel_fraction`: fraction of domain at each end that is the bevel zone (0..0.5).
///    Default when called with just an exponent: `(0.5 / exponent).clamp(0.05, 0.25)`.
/// - `density`: how many times denser the bevel zones are vs the middle (>= 1.0).
///    Controls both sample allocation and the power-curve concentration within bevel zones.
///
/// Returns `None` if parameters are invalid or produce degenerate results.
fn superellipse_nodes_with_params(
  n: usize,
  bevel_fraction: f32,
  density: f32,
) -> Option<Vec<f32>> {
  if n == 0
    || !bevel_fraction.is_finite()
    || !density.is_finite()
    || bevel_fraction <= 0.
    || bevel_fraction >= 0.5
    || density < 1.
  {
    return None;
  }

  if n == 1 {
    return Some(vec![0.5]);
  }

  // Allocate samples across the three regions proportional to length * density
  let middle_length = 1. - 2. * bevel_fraction;
  let effective_length = 2. * bevel_fraction * density + middle_length;
  let bevel_count_f = (n as f32 * bevel_fraction * density / effective_length).round();
  let bevel_count = (bevel_count_f as usize).max(1).min((n - 1) / 2);
  let middle_count = n - 2 * bevel_count;

  // Power curve exponent for non-uniform distribution within bevel zones.
  // Tied to density so there's no extra parameter: higher density = more concentration at corners.
  let p = density.sqrt();

  let mut nodes = Vec::with_capacity(n);

  // Small inset to avoid the degenerate t=0 and t=1 endpoints where radius collapses to zero,
  // but kept tight so the bevel samples capture most of the curvature range.
  const ENDPOINT_INSET: f32 = 0.1;

  // Start bevel: bevel_count samples in [0, bevel_fraction], concentrated toward 0
  for k in 0..bevel_count {
    let local_t = (k as f32 + ENDPOINT_INSET) / bevel_count as f32;
    let curved_t = local_t.powf(p); // concentrate toward 0 (the corner)
    nodes.push(curved_t * bevel_fraction);
  }

  // Middle: middle_count samples uniformly in [bevel_fraction, 1 - bevel_fraction]
  if middle_count > 0 {
    for k in 0..middle_count {
      let t = bevel_fraction + middle_length * (k as f32 + 0.5) / middle_count as f32;
      nodes.push(t);
    }
  }

  // End bevel: bevel_count samples in [1 - bevel_fraction, 1], concentrated toward 1 (mirror)
  for k in 0..bevel_count {
    let local_t = (k as f32 + ENDPOINT_INSET) / bevel_count as f32;
    let curved_t = local_t.powf(p);
    nodes.push(1. - curved_t * bevel_fraction); // mirror of start: concentrate toward 1
  }

  // Verify all nodes are finite and in range
  for t in &mut nodes {
    if !t.is_finite() {
      return None;
    }
    *t = t.clamp(0., 1.);
  }

  // Sort to ensure monotonicity (the three regions should already be ordered, but be safe)
  nodes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

  Some(nodes)
}

/// Convenience wrapper: derives `bevel_fraction` and `density` from a superellipse exponent.
fn superellipse_nodes(n: usize, exponent: f32) -> Option<Vec<f32>> {
  if exponent <= 0. || !exponent.is_finite() {
    return None;
  }
  let bevel_fraction = (0.5 / exponent).clamp(0.05, 0.25);
  let density = 3.0_f32;
  superellipse_nodes_with_params(n, bevel_fraction, density)
}

fn compute_spine_t_values(
  ctx: &EvalCtx,
  spine_sampling_scheme: &Value,
  spine_resolution: usize,
) -> Result<Vec<f32>, ErrorStack> {
  match spine_sampling_scheme {
    Value::Nil => Ok(uniform_nodes(spine_resolution)),
    Value::String(scheme) => {
      let scheme_lower = scheme.to_lowercase();
      match scheme_lower.as_str() {
        "uniform" => Ok(uniform_nodes(spine_resolution)),
        "chebyshev" | "cos" | "cosine" => Ok(chebyshev_nodes(spine_resolution)),
        "superellipse" | "bevel" => superellipse_nodes(spine_resolution, 5.).ok_or_else(|| {
          ErrorStack::new(
            "spine_sampling_scheme \"superellipse\" failed to compute valid nodes; falling back \
             not possible in string form, use map form with explicit exponent",
          )
        }),
        // "adaptive" is handled separately in rail_sweep_impl before calling this function
        "adaptive" => Ok(uniform_nodes(spine_resolution)),
        _ => Err(ErrorStack::new(format!(
          "Invalid spine_sampling_scheme string for `rail_sweep`; expected \"uniform\", \
           \"chebyshev\", \"cos\", \"cosine\", \"superellipse\", \"bevel\", or \"adaptive\", \
           found: \"{scheme}\""
        ))),
      }
    }
    Value::Map(map) => {
      let type_val = map.get("type").ok_or_else(|| {
        ErrorStack::new(
          "spine_sampling_scheme map requires a 'type' key; expected { type: \"uniform\" | \
           \"chebyshev\" | \"superellipse\" | \"adaptive\", ... }",
        )
      })?;

      let type_str = type_val.as_str().ok_or_else(|| {
        ErrorStack::new(format!(
          "spine_sampling_scheme 'type' must be a string, found: {type_val:?}"
        ))
      })?;

      let type_lower = type_str.to_lowercase();
      match type_lower.as_str() {
        "uniform" => {
          for key in map.keys() {
            if key != "type" {
              return Err(ErrorStack::new(format!(
                "Unknown key '{key}' in spine_sampling_scheme map for type \"uniform\"; only \
                 'type' is allowed"
              )));
            }
          }
          Ok(uniform_nodes(spine_resolution))
        }
        "chebyshev" | "cos" | "cosine" => {
          for key in map.keys() {
            if key != "type" {
              return Err(ErrorStack::new(format!(
                "Unknown key '{key}' in spine_sampling_scheme map for type \"{type_str}\"; only \
                 'type' is allowed"
              )));
            }
          }
          Ok(chebyshev_nodes(spine_resolution))
        }
        "superellipse" | "bevel" => {
          for key in map.keys() {
            if key != "type" && key != "exponent" && key != "bevel_fraction" && key != "density" {
              return Err(ErrorStack::new(format!(
                "Unknown key '{key}' in spine_sampling_scheme map for type \"{type_str}\"; \
                 allowed keys are 'type', 'exponent', 'bevel_fraction', and 'density'"
              )));
            }
          }

          let exponent = match map.get("exponent") {
            Some(Value::Float(f)) => *f,
            Some(Value::Int(i)) => *i as f32,
            Some(other) => {
              return Err(ErrorStack::new(format!(
                "spine_sampling_scheme 'exponent' must be numeric (int or float), found: {other:?}"
              )))
            }
            None => 5.,
          };

          let bevel_fraction = match map.get("bevel_fraction") {
            Some(Value::Float(f)) => *f,
            Some(Value::Int(i)) => *i as f32,
            Some(other) => {
              return Err(ErrorStack::new(format!(
                "spine_sampling_scheme 'bevel_fraction' must be numeric, found: {other:?}"
              )))
            }
            None => (0.5 / exponent).clamp(0.05, 0.25),
          };

          let density = match map.get("density") {
            Some(Value::Float(f)) => *f,
            Some(Value::Int(i)) => *i as f32,
            Some(other) => {
              return Err(ErrorStack::new(format!(
                "spine_sampling_scheme 'density' must be numeric, found: {other:?}"
              )))
            }
            None => 3.0,
          };

          match superellipse_nodes_with_params(spine_resolution, bevel_fraction, density) {
            Some(nodes) => Ok(nodes),
            // fall back to uniform sampling for degenerate cases
            None => Ok(uniform_nodes(spine_resolution)),
          }
        }
        // "adaptive" is handled separately in rail_sweep_impl before calling this function
        "adaptive" => {
          for key in map.keys() {
            if key != "type" {
              return Err(ErrorStack::new(format!(
                "Unknown key '{key}' in spine_sampling_scheme map for type \"adaptive\"; only \
                 'type' is allowed"
              )));
            }
          }
          Ok(uniform_nodes(spine_resolution))
        }
        _ => Err(ErrorStack::new(format!(
          "Invalid spine_sampling_scheme type \"{type_str}\"; expected \"uniform\", \
           \"chebyshev\"/\"cos\"/\"cosine\", \"superellipse\", \"bevel\", or \"adaptive\""
        ))),
      }
    }
    Value::Sequence(seq) => {
      let mut t_values = Vec::with_capacity(spine_resolution);
      for (ix, res) in seq.consume(ctx).enumerate() {
        let val = res.map_err(|err| {
          err.wrap(format!(
            "Error evaluating spine_sampling_scheme sequence at index {ix}"
          ))
        })?;
        let t = match &val {
          Value::Float(f) => *f,
          Value::Int(i) => *i as f32,
          _ => {
            return Err(ErrorStack::new(format!(
              "Invalid value in spine_sampling_scheme sequence at index {ix}; expected numeric \
               (int or float), found: {val:?}"
            )))
          }
        };
        t_values.push(t);
      }

      if t_values.len() != spine_resolution {
        return Err(ErrorStack::new(format!(
          "spine_sampling_scheme sequence length ({}) does not match spine_resolution \
           ({spine_resolution}); these must be equal when providing explicit t values",
          t_values.len(),
        )));
      }

      Ok(t_values)
    }
    Value::Callable(cb) => {
      let mut t_values = Vec::with_capacity(spine_resolution);
      for i in 0..spine_resolution {
        let out = ctx
          .invoke_callable(cb, &[Value::Int(i as i64)], EMPTY_KWARGS)
          .map_err(|err| {
            err.wrap(format!(
              "Error calling spine_sampling_scheme callable at index {i}"
            ))
          })?;
        let t = match &out {
          Value::Float(f) => *f,
          Value::Int(i) => *i as f32,
          _ => {
            return Err(ErrorStack::new(format!(
              "spine_sampling_scheme callable returned invalid type at index {i}; expected \
               numeric (int or float), found: {out:?}"
            )))
          }
        };
        t_values.push(t);
      }
      Ok(t_values)
    }
    _ => Err(ErrorStack::new(format!(
      "Invalid spine_sampling_scheme for `rail_sweep`; expected string, sequence, map, or \
       callable, found: {spine_sampling_scheme:?}"
    ))),
  }
}

struct StaticProfileOuter {
  profile: Rc<Callable>,
  profile_samplers: Option<Rc<Callable>>,
}

impl DynamicCallable for StaticProfileOuter {
  fn as_any(&self) -> &dyn std::any::Any {
    self
  }
  fn is_side_effectful(&self) -> bool {
    false
  }
  fn is_rng_dependent(&self) -> bool {
    false
  }
  fn invoke(
    &self,
    args: &[Value],
    _kwargs: &FxHashMap<Sym, Value>,
    _ctx: &EvalCtx,
  ) -> Result<Value, ErrorStack> {
    let u = match args.first() {
      Some(Value::Float(f)) => *f,
      Some(Value::Int(i)) => *i as f32,
      other => {
        return Err(ErrorStack::new(format!(
          "static_profile_adapter: expected float `u`, found: {other:?}"
        )))
      }
    };
    // If profile is itself a path sampler (|v| -> vec2), use it directly as both sampler and
    // path_samplers so critical t values flow through to the dynamic profile machinery.
    if as_path_sampler(&self.profile).is_some() {
      let mut map = FxHashMap::default();
      map.insert(
        "sampler".to_owned(),
        Value::Callable(Rc::clone(&self.profile)),
      );
      map.insert(
        "path_samplers".to_owned(),
        Value::Callable(Rc::clone(&self.profile)),
      );
      return Ok(Value::Map(Rc::new(map)));
    }
    // Regular |u, v| -> vec2 callable: wrap in an inner sampler that captures u.
    let inner = Rc::new(Callable::Dynamic {
      name: "static_profile_inner".to_owned(),
      inner: Box::new(StaticProfileInner {
        profile: Rc::clone(&self.profile),
        u,
      }),
    });
    match &self.profile_samplers {
      Some(ps_cb) => {
        let mut map = FxHashMap::default();
        map.insert("sampler".to_owned(), Value::Callable(inner));
        map.insert(
          "path_samplers".to_owned(),
          Value::Callable(Rc::clone(ps_cb)),
        );
        Ok(Value::Map(Rc::new(map)))
      }
      None => Ok(Value::Callable(inner)),
    }
  }
  fn get_return_type_hint(&self) -> Option<crate::ArgType> {
    None
  }
}

struct StaticProfileInner {
  profile: Rc<Callable>,
  u: f32,
}

impl DynamicCallable for StaticProfileInner {
  fn as_any(&self) -> &dyn std::any::Any {
    self
  }
  fn is_side_effectful(&self) -> bool {
    false
  }
  fn is_rng_dependent(&self) -> bool {
    false
  }
  fn invoke(
    &self,
    args: &[Value],
    _kwargs: &FxHashMap<Sym, Value>,
    ctx: &EvalCtx,
  ) -> Result<Value, ErrorStack> {
    let v = match args.first() {
      Some(Value::Float(f)) => *f,
      Some(Value::Int(i)) => *i as f32,
      other => {
        return Err(ErrorStack::new(format!(
          "static_profile_adapter: expected float `v`, found: {other:?}"
        )))
      }
    };
    ctx
      .invoke_callable(
        &self.profile,
        &[Value::Float(self.u), Value::Float(v)],
        EMPTY_KWARGS,
      )
      .map_err(|err| err.wrap("Error calling user-provided `profile` callable in `rail_sweep`"))
  }
  fn get_return_type_hint(&self) -> Option<crate::ArgType> {
    Some(crate::ArgType::Vec2)
  }
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
      let profile_val = arg_refs[3].resolve(args, kwargs);
      let frame_mode_val = arg_refs[4].resolve(args, kwargs);
      let twist_val = arg_refs[5].resolve(args, kwargs);
      let closed = arg_refs[6].resolve(args, kwargs).as_bool().unwrap();
      let capped = arg_refs[7].resolve(args, kwargs).as_bool().unwrap();
      let profile_samplers_val = arg_refs[8].resolve(args, kwargs);
      let dynamic_profile_val = arg_refs[9].resolve(args, kwargs);
      let fku_stitching = arg_refs[10].resolve(args, kwargs).as_bool().unwrap();
      let spine_sampling_scheme_val = arg_refs[11].resolve(args, kwargs);
      let adaptive_profile_sampling = arg_refs[12].resolve(args, kwargs).as_bool().unwrap();

      let use_adaptive_spine = is_adaptive_spine_scheme(&spine_sampling_scheme_val);

      let has_profile = !matches!(profile_val, Value::Nil);
      let has_dynamic_profile = !matches!(dynamic_profile_val, Value::Nil);
      let has_profile_samplers = !matches!(profile_samplers_val, Value::Nil);

      if has_profile && has_dynamic_profile {
        return Err(ErrorStack::new(
          "Cannot specify both `profile` and `dynamic_profile` in `rail_sweep`",
        ));
      }

      if !has_profile && !has_dynamic_profile {
        return Err(ErrorStack::new(
          "Must specify either `profile` or `dynamic_profile` in `rail_sweep`",
        ));
      }

      if has_dynamic_profile && has_profile_samplers {
        return Err(ErrorStack::new(
          "Cannot use `profile_samplers` with `dynamic_profile` in `rail_sweep`; critical points \
           are extracted from the dynamic profile's return value",
        ));
      }

      /// Resamples spine points along the path at the specified t values.
      /// Each t value in [0, 1] represents a position along the arc length of the path.
      fn resample_spine_points_at_t(
        points: &[Vec3],
        t_values: &[f32],
      ) -> Result<Vec<Vec3>, ErrorStack> {
        if points.len() < 2 {
          return Err(ErrorStack::new(format!(
            "`rail_sweep` requires at least two spine points, found: {}",
            points.len()
          )));
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

        let mut out = Vec::with_capacity(t_values.len());
        for &t in t_values {
          let target = total * t;
          let mut seg_ix = 0;
          while seg_ix + 1 < cumulative.len() && cumulative[seg_ix + 1] < target {
            seg_ix += 1;
          }
          // Handle edge case where t=1.0 might exceed bounds
          if seg_ix + 1 >= cumulative.len() {
            seg_ix = cumulative.len() - 2;
          }
          let seg_len = cumulative[seg_ix + 1] - cumulative[seg_ix];
          let local_t = if seg_len <= 0. {
            0.
          } else {
            ((target - cumulative[seg_ix]) / seg_len).clamp(0., 1.)
          };
          out.push(points[seg_ix].lerp(&points[seg_ix + 1], local_t));
        }

        Ok(out)
      }

      // Compute spine_t_values and spine_points based on sampling scheme
      let (spine_t_values, spine_points) = if let Some(seq) = spine.as_sequence() {
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

        if use_adaptive_spine {
          // Adaptively resample the polyline based on curvature
          // Use the original point t-values as critical points so they're preserved
          let original_ts: Vec<f32> = (0..raw_points.len())
            .map(|i| i as f32 / (raw_points.len() - 1) as f32)
            .collect();

          // Build a sampler function that interpolates the polyline
          let sample_polyline = |t: f32| -> Vec3 {
            if t <= 0.0 {
              return raw_points[0];
            }
            if t >= 1.0 {
              return raw_points[raw_points.len() - 1];
            }
            // Find segment
            let scaled = t * (raw_points.len() - 1) as f32;
            let seg_ix = scaled.floor() as usize;
            let local_t = scaled - seg_ix as f32;
            if seg_ix + 1 >= raw_points.len() {
              raw_points[raw_points.len() - 1]
            } else {
              raw_points[seg_ix].lerp(&raw_points[seg_ix + 1], local_t)
            }
          };

          // Use adaptive sampling with Vec3 points
          use super::adaptive_sampler::adaptive_sample;
          let adaptive_ts =
            adaptive_sample::<Vec3, _>(spine_resolution, &original_ts, sample_polyline, 1e-5);

          // Resample at the adaptive t values
          let points = resample_spine_points_at_t(&raw_points, &adaptive_ts)?;
          (adaptive_ts, points)
        } else {
          // Use regular sampling scheme
          let spine_t_values =
            compute_spine_t_values(ctx, &spine_sampling_scheme_val, spine_resolution)?;

          // Always resample the spine path at the specified t values
          // This ensures the sampling scheme is respected even when the input
          // sequence happens to have the same length as spine_resolution
          let points = resample_spine_points_at_t(&raw_points, &spine_t_values)?;
          (spine_t_values, points)
        }
      } else if let Some(cb) = spine.as_callable() {
        if use_adaptive_spine {
          // Use adaptive sampling with the spine callable
          let sample_spine = |t: f32| -> Result<Vec3, ErrorStack> {
            let out = ctx
              .invoke_callable(cb, &[Value::Float(t)], EMPTY_KWARGS)
              .map_err(|err| {
                err.wrap("Error calling user-provided cb passed to `spine` arg in `rail_sweep`")
              })?;
            out.as_vec3().copied().ok_or_else(|| {
              ErrorStack::new(format!(
                "Expected Vec3 from user-provided cb passed to `spine` arg in `rail_sweep`, \
                 found: {out:?}"
              ))
            })
          };

          // Get adaptive t values using the spine callable
          let adaptive_ts = adaptive_sample_fallible::<Vec3, ErrorStack>(
            spine_resolution,
            &[0., 1.],
            sample_spine,
            1e-5,
          )?;

          // Sample spine at adaptive t values
          let mut points = Vec::with_capacity(spine_resolution);
          for &t in &adaptive_ts {
            let out = ctx
              .invoke_callable(cb, &[Value::Float(t)], EMPTY_KWARGS)
              .map_err(|err| {
                err.wrap("Error calling user-provided cb passed to `spine` arg in `rail_sweep`")
              })?;
            let v = out.as_vec3().ok_or_else(|| {
              ErrorStack::new(format!(
                "Expected Vec3 from user-provided cb passed to `spine` arg in `rail_sweep`, \
                 found: {out:?}"
              ))
            })?;
            points.push(*v);
          }
          (adaptive_ts, points)
        } else {
          // Use regular sampling scheme
          let spine_t_values =
            compute_spine_t_values(ctx, &spine_sampling_scheme_val, spine_resolution)?;

          // Sample the spine callable at the specified t values
          let mut points = Vec::with_capacity(spine_resolution);
          for &t in &spine_t_values {
            let out = ctx
              .invoke_callable(cb, &[Value::Float(t)], EMPTY_KWARGS)
              .map_err(|err| {
                err.wrap("Error calling user-provided cb passed to `spine` arg in `rail_sweep`")
              })?;
            let v = out.as_vec3().ok_or_else(|| {
              ErrorStack::new(format!(
                "Expected Vec3 from user-provided cb passed to `spine` arg in `rail_sweep`, \
                 found: {out:?}"
              ))
            })?;
            points.push(*v);
          }
          (spine_t_values, points)
        }
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

      let twist = if let Some(f) = twist_val.as_float() {
        Twist::Const(f)
      } else if let Some(cb) = twist_val.as_callable() {
        // Pre-sample twist values to check if they're effectively constant
        let mut twist_values = Vec::with_capacity(spine_points.len());
        for (i, pos) in spine_points.iter().enumerate() {
          let out = ctx
            .invoke_callable(cb, &[Value::Int(i as i64), Value::Vec3(*pos)], EMPTY_KWARGS)
            .map_err(|err| {
              err.wrap("Error calling user-provided cb passed to `twist` arg in `rail_sweep`")
            })?;
          let val = out.as_float().ok_or_else(|| {
            ErrorStack::new(format!(
              "Expected Numeric (int or float) from user-provided cb passed to `twist` arg in \
               `rail_sweep`, found: {out:?}"
            ))
          })?;
          twist_values.push(val);
        }

        let first = twist_values.first().copied().unwrap_or(0.0);
        let is_constant = twist_values
          .iter()
          .all(|v| (*v - first).abs() <= TWIST_CONST_EPSILON);

        if is_constant {
          Twist::Const(first)
        } else {
          Twist::Presampled(twist_values)
        }
      } else {
        return Err(ErrorStack::new(format!(
          "Invalid twist argument for `rail_sweep`; expected Numeric or Callable, found: \
           {twist_val:?}"
        )));
      };

      let dynamic_profile_cb = if has_dynamic_profile {
        Rc::clone(dynamic_profile_val.as_callable().ok_or_else(|| {
          ErrorStack::new(format!(
            "Invalid dynamic_profile argument for `rail_sweep`; expected Callable, found: \
             {dynamic_profile_val:?}"
          ))
        })?)
      } else {
        let profile_cb = profile_val.as_callable().ok_or_else(|| {
          ErrorStack::new(format!(
            "Invalid profile argument for `rail_sweep`; expected Callable, found: {profile_val:?}"
          ))
        })?;
        Rc::new(Callable::Dynamic {
          name: "static_profile_adapter".to_owned(),
          inner: Box::new(StaticProfileOuter {
            profile: Rc::clone(profile_cb),
            profile_samplers: profile_samplers_val.as_callable().map(Rc::clone),
          }),
        })
      };

      let mesh = rail_sweep_dynamic(
        ctx,
        &spine_points,
        ring_resolution,
        frame_mode,
        closed,
        capped,
        &twist,
        &dynamic_profile_cb,
        fku_stitching,
        Some(&spine_t_values),
        adaptive_profile_sampling,
      )?;

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
  use super::{
    chebyshev_nodes, rail_sweep, rail_sweep_dynamic, superellipse_nodes,
    superellipse_nodes_with_params, FrameMode, Twist,
  };
  use crate::builtins::trace_path::build_topology_samples;
  use crate::{Callable, DynamicCallable, ErrorStack, EvalCtx, Sym, Value, Vec2};
  use fxhash::FxHashMap;
  use mesh::linked_mesh::Vec3;
  use std::f32::consts::PI;
  use std::{any::Any, rc::Rc};

  #[test]
  fn test_rail_sweep_basic_counts() {
    let spine = vec![Vec3::new(0., 0., 0.), Vec3::new(0., 0., 2.)];
    let mesh = rail_sweep(
      &spine,
      4,
      FrameMode::Rmf,
      false,
      false,
      |_, _| Ok(0.),
      |_, v_norm, _, v_ix, _| {
        let angle = v_norm * std::f32::consts::TAU;
        let radius = if v_ix % 2 == 0 { 1. } else { 0.5 };
        Ok(Vec2::new(angle.cos() * radius, angle.sin() * radius))
      },
      None,
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
      None,
    )
    .unwrap();

    assert_eq!(mesh.vertices.len(), ring_resolution + 2);
    assert_eq!(mesh.faces.len(), ring_resolution * 2);
  }

  #[test]
  fn test_rail_sweep_guide_sampling_includes_critical_points() {
    let guides = vec![0.5];
    let samples = build_topology_samples(6, Some(&guides), None, false);

    assert_eq!(samples.len(), 6);
    assert!((samples[0] - 0.0).abs() < 1e-6);
    assert!((samples[3] - 0.5).abs() < 1e-6);
    assert!(samples.iter().all(|v| *v >= 0.0 && *v < 1.0));
  }

  #[test]
  fn test_rail_sweep_guide_sampling_skips_straight_segments() {
    let guides = vec![0.0, 0.5, 1.0];
    let weights = vec![0.0, 1.0];
    let samples = build_topology_samples(6, Some(&guides), Some(&weights), false);

    assert_eq!(samples.len(), 6);
    assert!((samples[0] - 0.0).abs() < 1e-6);
    assert!((samples[1] - 0.5).abs() < 1e-6);
    assert!((samples[2] - 0.6).abs() < 1e-6);
    assert!((samples[3] - 0.7).abs() < 1e-6);
    assert!((samples[4] - 0.8).abs() < 1e-6);
    assert!((samples[5] - 0.9).abs() < 1e-6);
  }

  #[test]
  fn test_chebyshev_nodes_properties() {
    // Test Chebyshev nodes have expected properties:
    // 1. First node is close to (but not exactly) 0
    // 2. Last node is close to (but not exactly) 1
    // 3. Nodes are denser near endpoints

    let nodes = chebyshev_nodes(10);
    assert_eq!(nodes.len(), 10);

    // First node should be close to but not exactly 0
    assert!(nodes[0] > 0.0);
    assert!(nodes[0] < 0.1);

    // Last node should be close to but not exactly 1
    assert!(nodes[9] < 1.0);
    assert!(nodes[9] > 0.9);

    // Verify nodes are strictly increasing
    for i in 1..nodes.len() {
      assert!(
        nodes[i] > nodes[i - 1],
        "Nodes should be strictly increasing"
      );
    }

    // Verify spacing is denser near endpoints:
    // The gap between first two nodes should be smaller than the gap around the middle
    let start_gap = nodes[1] - nodes[0];
    let mid_gap = nodes[5] - nodes[4];
    assert!(
      start_gap < mid_gap,
      "Chebyshev nodes should be denser near endpoints"
    );
  }

  #[test]
  fn test_superellipse_nodes_basic_properties() {
    let nodes = superellipse_nodes(10, 5.0).expect("exponent=5 should work");
    assert_eq!(nodes.len(), 10);

    // First/last nodes should be near but NOT exactly 0/1
    assert!(nodes[0] > 0.0, "First node should be > 0");
    assert!(nodes[0] < 0.05, "First node should be close to 0");
    assert!(*nodes.last().unwrap() < 1.0, "Last node should be < 1");
    assert!(*nodes.last().unwrap() > 0.95, "Last node should be close to 1");

    // All nodes in range and strictly increasing
    for (i, &node) in nodes.iter().enumerate() {
      assert!(node >= 0.0 && node <= 1.0, "Node {} out of range: {}", i, node);
      if i > 0 {
        assert!(node > nodes[i - 1], "Nodes should be strictly increasing at {i}");
      }
    }
  }

  #[test]
  fn test_superellipse_nodes_three_region_structure() {
    // With bevel_fraction=0.1, density=3, n=20:
    // expect ~4 samples in each bevel zone and ~12 in the middle
    let nodes = superellipse_nodes_with_params(20, 0.1, 3.0).unwrap();
    assert_eq!(nodes.len(), 20);

    let in_start_bevel = nodes.iter().filter(|&&t| t < 0.1).count();
    let in_end_bevel = nodes.iter().filter(|&&t| t > 0.9).count();
    let in_middle = nodes.iter().filter(|&&t| t >= 0.1 && t <= 0.9).count();

    assert!(in_start_bevel >= 2, "Should have multiple samples in start bevel, got {in_start_bevel}");
    assert!(in_end_bevel >= 2, "Should have multiple samples in end bevel, got {in_end_bevel}");
    assert!(in_middle >= in_start_bevel, "Middle should have at least as many samples as a bevel zone");
    assert_eq!(in_start_bevel + in_end_bevel + in_middle, 20);
  }

  #[test]
  fn test_superellipse_nodes_bevel_concentration() {
    // Within the start bevel zone, samples should be denser near 0 (the corner)
    let nodes = superellipse_nodes_with_params(20, 0.15, 4.0).unwrap();

    // Collect start bevel samples
    let bevel_samples: Vec<f32> = nodes.iter().copied().filter(|&t| t < 0.15).collect();
    assert!(bevel_samples.len() >= 3, "Need enough bevel samples to test concentration");

    // Gaps should increase as we move away from 0
    let gaps: Vec<f32> = bevel_samples.windows(2).map(|w| w[1] - w[0]).collect();
    for i in 1..gaps.len() {
      assert!(
        gaps[i] >= gaps[i - 1] * 0.9, // allow small tolerance
        "Bevel gaps should generally increase away from corner: gap[{}]={} vs gap[{}]={}",
        i - 1, gaps[i - 1], i, gaps[i]
      );
    }
  }

  #[test]
  fn test_superellipse_nodes_middle_is_uniform() {
    let nodes = superellipse_nodes_with_params(30, 0.1, 3.0).unwrap();

    // Collect middle samples
    let middle: Vec<f32> = nodes.iter().copied().filter(|&t| t >= 0.1 && t <= 0.9).collect();
    assert!(middle.len() >= 5);

    // Middle gaps should be approximately equal
    let gaps: Vec<f32> = middle.windows(2).map(|w| w[1] - w[0]).collect();
    let avg_gap = gaps.iter().sum::<f32>() / gaps.len() as f32;
    for (i, &gap) in gaps.iter().enumerate() {
      assert!(
        (gap - avg_gap).abs() < avg_gap * 0.15,
        "Middle gap {} = {} deviates too much from avg {}", i, gap, avg_gap
      );
    }
  }

  #[test]
  fn test_superellipse_nodes_exponent_affects_distribution() {
    // Higher exponent -> smaller bevel_fraction -> fewer samples in bevel, more in middle
    let nodes_exp2 = superellipse_nodes(20, 2.0).unwrap(); // bevel_fraction=0.25
    let nodes_exp10 = superellipse_nodes(20, 10.0).unwrap(); // bevel_fraction=0.05

    // With exp=10, the bevel zone is [0, 0.05] ∪ [0.95, 1], much smaller than exp=2's [0, 0.25] ∪ [0.75, 1]
    // So exp=10 should have more samples concentrated in the middle region [0.25, 0.75]
    let mid_count_exp2 = nodes_exp2.iter().filter(|&&t| t >= 0.25 && t <= 0.75).count();
    let mid_count_exp10 = nodes_exp10.iter().filter(|&&t| t >= 0.25 && t <= 0.75).count();
    assert!(
      mid_count_exp10 > mid_count_exp2,
      "Higher exponent should have more samples in center: exp10={} vs exp2={}",
      mid_count_exp10, mid_count_exp2
    );
  }

  #[test]
  fn test_superellipse_nodes_symmetry() {
    let nodes = superellipse_nodes_with_params(21, 0.1, 3.0).unwrap();
    // Distribution should be symmetric around 0.5
    for i in 0..nodes.len() {
      let mirror = nodes.len() - 1 - i;
      let sum = nodes[i] + nodes[mirror];
      assert!(
        (sum - 1.0).abs() < 0.01,
        "Nodes should be symmetric: nodes[{}]={} + nodes[{}]={} = {} (expected ~1.0)",
        i, nodes[i], mirror, nodes[mirror], sum
      );
    }
  }

  #[test]
  fn test_superellipse_nodes_small_n() {
    // Should work with very few samples
    let nodes3 = superellipse_nodes(3, 5.0).unwrap();
    assert_eq!(nodes3.len(), 3);
    assert!(nodes3[0] > 0.0 && nodes3[0] < 0.2);
    assert!(nodes3[2] > 0.8 && nodes3[2] < 1.0);

    let nodes2 = superellipse_nodes(2, 5.0).unwrap();
    assert_eq!(nodes2.len(), 2);
    assert!(nodes2[0] > 0.0 && nodes2[1] < 1.0);

    let nodes1 = superellipse_nodes(1, 5.0).unwrap();
    assert_eq!(nodes1.len(), 1);
    assert!((nodes1[0] - 0.5).abs() < 0.01);
  }

  #[test]
  fn test_superellipse_nodes_invalid_params() {
    // Invalid exponent
    assert!(superellipse_nodes(10, 0.0).is_none());
    assert!(superellipse_nodes(10, -1.0).is_none());
    assert!(superellipse_nodes(10, f32::INFINITY).is_none());
    assert!(superellipse_nodes(10, f32::NAN).is_none());

    // Invalid bevel_fraction
    assert!(superellipse_nodes_with_params(10, 0.0, 3.0).is_none());
    assert!(superellipse_nodes_with_params(10, 0.5, 3.0).is_none());
    assert!(superellipse_nodes_with_params(10, -0.1, 3.0).is_none());

    // Invalid density
    assert!(superellipse_nodes_with_params(10, 0.1, 0.5).is_none());

    // n=0
    assert!(superellipse_nodes_with_params(0, 0.1, 3.0).is_none());

    // Very high exponent should still work
    let nodes = superellipse_nodes(10, 50.0).expect("exponent=50 should work");
    assert_eq!(nodes.len(), 10);
    for &node in &nodes {
      assert!(node >= 0.0 && node <= 1.0);
    }
  }

  #[test]
  fn test_rail_sweep_with_custom_spine_u_values() {
    // Test that custom spine_u_values are correctly passed through to the profile
    // by using a profile that records the u values it receives.
    use std::cell::RefCell;

    let received_u_values: RefCell<Vec<f32>> = RefCell::new(Vec::new());
    let spine = vec![
      Vec3::new(0., 0., 0.),
      Vec3::new(0., 0., 1.),
      Vec3::new(0., 0., 2.),
    ];

    // Use non-uniform u values: [0.1, 0.5, 0.9]
    let custom_u_values = vec![0.1, 0.5, 0.9];

    let mesh = rail_sweep(
      &spine,
      4, // ring_resolution
      FrameMode::Rmf,
      false,
      false,
      |_, _| Ok(0.0),
      |u, v_norm, _, _, _| {
        received_u_values.borrow_mut().push(u);
        let angle = v_norm * std::f32::consts::TAU;
        Ok(Vec2::new(angle.cos(), angle.sin()))
      },
      None,
      None,
      Some(&custom_u_values),
    )
    .unwrap();

    // The profile is called 4 times per spine point (ring_resolution = 4),
    // so we should have 12 recorded u values total (3 spine points * 4 ring vertices)
    let u_values = received_u_values.borrow();
    assert_eq!(u_values.len(), 12);

    // First 4 calls should all have u=0.1 (first spine point)
    for i in 0..4 {
      assert!(
        (u_values[i] - 0.1).abs() < 1e-6,
        "Expected u=0.1 for first spine point, got {}",
        u_values[i]
      );
    }

    // Next 4 calls should have u=0.5 (second spine point)
    for i in 4..8 {
      assert!(
        (u_values[i] - 0.5).abs() < 1e-6,
        "Expected u=0.5 for second spine point, got {}",
        u_values[i]
      );
    }

    // Last 4 calls should have u=0.9 (third spine point)
    for i in 8..12 {
      assert!(
        (u_values[i] - 0.9).abs() < 1e-6,
        "Expected u=0.9 for third spine point, got {}",
        u_values[i]
      );
    }

    // Also verify mesh was created successfully
    assert!(mesh.vertices.len() > 0);
    assert!(mesh.faces.len() > 0);
  }

  #[test]
  fn test_rail_sweep_capped_cylinder_is_2_manifold() {
    let spine = vec![
      Vec3::new(0., 0., 0.),
      Vec3::new(0.5, 0., 0.),
      Vec3::new(1., 0., 0.),
    ];
    let ring_resolution = 4;

    let mesh = rail_sweep(
      &spine,
      ring_resolution,
      FrameMode::Rmf,
      false,
      true,
      |_, _| Ok(0.),
      |_, v_norm, _, _, _| {
        let t = v_norm * PI * 2.;
        Ok(Vec2::new(t.cos(), t.sin()))
      },
      None,
      None,
      None,
    )
    .unwrap();

    mesh
      .check_is_manifold::<true>()
      .expect("Capped cylinder should be 2-manifold");
  }

  #[test]
  fn test_rail_sweep_dynamic_multi_ring_is_2_manifold() {
    struct CircleSampler;

    impl DynamicCallable for CircleSampler {
      fn as_any(&self) -> &dyn Any {
        self
      }

      fn is_side_effectful(&self) -> bool {
        false
      }

      fn is_rng_dependent(&self) -> bool {
        false
      }

      fn invoke(
        &self,
        args: &[Value],
        _kwargs: &FxHashMap<Sym, Value>,
        _ctx: &EvalCtx,
      ) -> Result<Value, ErrorStack> {
        let v = match args.first() {
          Some(Value::Float(v)) => *v,
          other => {
            return Err(ErrorStack::new(format!(
              "Expected Float in test sampler, found: {other:?}"
            )));
          }
        };
        let t = v * std::f32::consts::TAU;
        Ok(Value::Vec2(Vec2::new(t.cos(), t.sin())))
      }

      fn get_return_type_hint(&self) -> Option<crate::ArgType> {
        Some(crate::ArgType::Vec2)
      }
    }

    struct ConstantDynamicProfile {
      sampler: Rc<Callable>,
    }

    impl DynamicCallable for ConstantDynamicProfile {
      fn as_any(&self) -> &dyn Any {
        self
      }

      fn is_side_effectful(&self) -> bool {
        false
      }

      fn is_rng_dependent(&self) -> bool {
        false
      }

      fn invoke(
        &self,
        _args: &[Value],
        _kwargs: &FxHashMap<Sym, Value>,
        _ctx: &EvalCtx,
      ) -> Result<Value, ErrorStack> {
        Ok(Value::Callable(Rc::clone(&self.sampler)))
      }

      fn get_return_type_hint(&self) -> Option<crate::ArgType> {
        None
      }
    }

    let ctx = EvalCtx::default();
    let sampler = Rc::new(Callable::Dynamic {
      name: "test_sampler".to_owned(),
      inner: Box::new(CircleSampler),
    });
    let dynamic_profile_cb = Rc::new(Callable::Dynamic {
      name: "test_dynamic_profile".to_owned(),
      inner: Box::new(ConstantDynamicProfile {
        sampler: Rc::clone(&sampler),
      }),
    });

    let spine = vec![
      Vec3::new(0., 0., 0.),
      Vec3::new(0., 0.2, 1.),
      Vec3::new(0.5, 0.3, 2.),
      Vec3::new(1., 0.4, 3.),
    ];
    let twist = Twist::Const(0.0);
    let mesh = rail_sweep_dynamic(
      &ctx,
      &spine,
      12,
      FrameMode::Rmf,
      false,
      true,
      &twist,
      &dynamic_profile_cb,
      true,
      None,
      false,
    )
    .unwrap();

    mesh
      .check_is_manifold::<true>()
      .expect("Dynamic rail sweep should produce a 2-manifold mesh");
  }

  #[test]
  fn test_rail_sweep_dynamic_adaptive_sampling() {
    // Test that adaptive sampling produces a valid 2-manifold mesh
    // using a superellipse profile (high curvature at corners)
    struct SuperellipseSampler {
      exponent: f32,
    }

    impl DynamicCallable for SuperellipseSampler {
      fn as_any(&self) -> &dyn Any {
        self
      }

      fn is_side_effectful(&self) -> bool {
        false
      }

      fn is_rng_dependent(&self) -> bool {
        false
      }

      fn invoke(
        &self,
        args: &[Value],
        _kwargs: &FxHashMap<Sym, Value>,
        _ctx: &EvalCtx,
      ) -> Result<Value, ErrorStack> {
        let t = match args.first() {
          Some(Value::Float(v)) => *v,
          other => {
            return Err(ErrorStack::new(format!(
              "Expected Float in test sampler, found: {other:?}"
            )));
          }
        };
        let angle = t * std::f32::consts::TAU;
        let cos_a = angle.cos();
        let sin_a = angle.sin();

        // Superellipse parametric formula
        let x = cos_a.abs().powf(2.0 / self.exponent) * cos_a.signum();
        let y = sin_a.abs().powf(2.0 / self.exponent) * sin_a.signum();

        Ok(Value::Vec2(Vec2::new(x, y)))
      }

      fn get_return_type_hint(&self) -> Option<crate::ArgType> {
        Some(crate::ArgType::Vec2)
      }
    }

    // Dynamic profile that returns a map with adaptive: true
    struct AdaptiveDynamicProfile {
      sampler: Rc<Callable>,
    }

    impl DynamicCallable for AdaptiveDynamicProfile {
      fn as_any(&self) -> &dyn Any {
        self
      }

      fn is_side_effectful(&self) -> bool {
        false
      }

      fn is_rng_dependent(&self) -> bool {
        false
      }

      fn invoke(
        &self,
        _args: &[Value],
        _kwargs: &FxHashMap<Sym, Value>,
        _ctx: &EvalCtx,
      ) -> Result<Value, ErrorStack> {
        // Return a map with sampler and adaptive: true
        let mut map = FxHashMap::default();
        map.insert(
          "sampler".to_owned(),
          Value::Callable(Rc::clone(&self.sampler)),
        );
        map.insert("adaptive".to_owned(), Value::Bool(true));
        Ok(Value::Map(Rc::new(map)))
      }

      fn get_return_type_hint(&self) -> Option<crate::ArgType> {
        None
      }
    }

    let ctx = EvalCtx::default();
    let sampler = Rc::new(Callable::Dynamic {
      name: "superellipse_sampler".to_owned(),
      inner: Box::new(SuperellipseSampler { exponent: 6.0 }),
    });
    let dynamic_profile_cb = Rc::new(Callable::Dynamic {
      name: "adaptive_dynamic_profile".to_owned(),
      inner: Box::new(AdaptiveDynamicProfile {
        sampler: Rc::clone(&sampler),
      }),
    });

    let spine = vec![
      Vec3::new(0., 0., 0.),
      Vec3::new(0., 0., 1.),
      Vec3::new(0., 0., 2.),
    ];
    let twist = Twist::Const(0.0);

    // Test with per-ring adaptive setting (via map)
    let mesh = rail_sweep_dynamic(
      &ctx,
      &spine,
      16,
      FrameMode::Rmf,
      false,
      true,
      &twist,
      &dynamic_profile_cb,
      true,
      None,
      false,
    )
    .unwrap();

    mesh
      .check_is_manifold::<true>()
      .expect("Adaptive sampling should produce a 2-manifold mesh");

    // Verify we got a reasonable number of vertices
    // 3 spine points * 16 ring_resolution = 48 expected (approximately)
    assert!(
      mesh.vertices.len() >= 30 && mesh.vertices.len() <= 60,
      "Unexpected vertex count: {}",
      mesh.vertices.len()
    );
  }
}
