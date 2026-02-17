use std::{cell::RefCell, f32::consts::PI, rc::Rc};

use fxhash::FxHashMap;
use mesh::{linked_mesh::Vec3, slotmap_utils::vkey, LinkedMesh};
use nalgebra::Matrix4;

use crate::{
  builtins::trace_path::{
    as_path_sampler, build_interval_weights, build_topology_samples, normalize_guides,
    SegmentInterval,
  },
  ArgRef, Callable, ErrorStack, EvalCtx, ManifoldHandle, MeshHandle, Sym, Value, Vec2,
  EMPTY_KWARGS,
};

use super::adaptive_sampler::adaptive_sample_fallible;
use super::fku_stitch::{
  dp_stitch_presampled, should_use_fku, snap_critical_points, stitch_apex_to_row,
  uniform_stitch_rows,
};
use super::helpers::{compute_centroid, vertices_are_collapsed};

const FRAME_EPSILON: f32 = 1e-6;
const COLLAPSE_EPSILON: f32 = 1e-5;
/// Maximum allowed deviation from a straight line (as a fraction of total spine length) before
/// disabling the topology-aware sampling optimization.
const SPINE_STRAIGHTNESS_THRESHOLD: f32 = 0.035;
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

/// Checks whether the spine points lie approximately on a straight line.
///
/// Returns `true` if the maximum perpendicular distance of any point from the line connecting the
/// first and last points is within `SPINE_STRAIGHTNESS_THRESHOLD` times the total spine length.
fn spine_is_approximately_straight(points: &[Vec3]) -> bool {
  if points.len() < 3 {
    return true;
  }

  let start = points[0];
  let end = points[points.len() - 1];
  let line_dir = end - start;
  let line_len_sq = line_dir.norm_squared();

  // degenerate case
  if line_len_sq < FRAME_EPSILON {
    let threshold_sq = SPINE_STRAIGHTNESS_THRESHOLD * SPINE_STRAIGHTNESS_THRESHOLD;
    return points
      .iter()
      .all(|p| (*p - start).norm_squared() < threshold_sq);
  }

  let line_len = line_len_sq.sqrt();
  let line_dir_normalized = line_dir / line_len;

  let mut max_deviation = 0.0f32;
  for point in &points[1..points.len() - 1] {
    let to_point = *point - start;
    // Project onto line direction
    let proj_len = to_point.dot(&line_dir_normalized);
    let proj = line_dir_normalized * proj_len;
    // Perpendicular distance
    let perp = to_point - proj;
    let deviation = perp.norm();
    max_deviation = max_deviation.max(deviation);
  }

  max_deviation <= line_len * SPINE_STRAIGHTNESS_THRESHOLD
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
}

struct DynamicProfileData {
  sampler: Rc<Callable>,
  critical_points: Vec<f32>,
  sharp: bool,
  /// When Some(true), forces adaptive sampling for this ring.
  /// When Some(false), disables adaptive sampling even if global is enabled.
  /// When None, uses the global `adaptive_profile_sampling` setting.
  adaptive: Option<bool>,
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
      let critical_points = normalize_guides(&sampler.critical_t_values());
      return Ok(DynamicProfileData {
        sampler: Rc::clone(callable),
        critical_points,
        sharp: false,
        adaptive: None,
      });
    }
    return Ok(DynamicProfileData {
      sampler: Rc::clone(callable),
      critical_points: vec![0., 1.],
      sharp: false,
      adaptive: None,
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

    let critical_points = match map.get("path_samplers") {
      Some(val) => collect_path_sampler_guides(ctx, val, "dynamic_profile path_samplers")?
        .unwrap_or_else(|| vec![0., 1.]),
      None => vec![0., 1.],
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
    });
  }

  Err(ErrorStack::new(
    "dynamic_profile must return a callable or a map with 'sampler' key",
  ))
}

/// Samples the 2D profile offset at the given t value.
fn sample_profile_offset(ctx: &EvalCtx, ring: &RingContext, v: f32) -> Result<Vec2, ErrorStack> {
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

    if is_end && vertices_are_collapsed(&ring) {
      let start = verts.len();
      verts.push(compute_centroid(&ring));
      ring_infos.push(RingInfo {
        start,
        count: 1,
        cap_frame: None,
        sharp: false,
        t_values: None,
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

    let result = ctx.invoke_callable(dynamic_profile_cb, &[Value::Float(u)], EMPTY_KWARGS)?;
    let mut profile_data = extract_dynamic_profile_data(ctx, result)?;
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
      let (count, t_values) = if ring.collapsed {
        let apex = sample_profile_at(ctx, &ring, 0.0)?;
        verts.push(apex);
        (1, None)
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

        let t_vals = samples.clone();
        for v in samples {
          verts.push(sample_profile_at(ctx, &ring, v)?);
        }
        (verts.len() - start, Some(t_vals))
      };

      Ok(RingInfo {
        start,
        count,
        cap_frame: ring.cap_frame.clone(),
        sharp: ring.profile_data.sharp,
        t_values,
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

/// Computes superellipse-adapted sampling nodes for `n` points, mapped to [0, 1].
///
/// Uses a power-transformed cosine distribution that concentrates samples near endpoints.
/// The concentration increases with the exponent:
/// - exponent=2: equivalent to Chebyshev-like (cosine) spacing
/// - exponent>2: progressively more concentration at endpoints
/// - exponent<2: samples spread more toward the center
/// - exponent=1: degenerates toward uniform (with some numerical edge cases)
///
/// Like Chebyshev nodes, the first and last samples are near but not exactly 0 and 1.
/// This is important for superellipse profiles where t=0 or t=1 would give zero radius.
///
/// Returns `None` if the computation produces invalid results (NaN/Inf), in which case
/// the caller should fall back to uniform sampling.
fn superellipse_nodes(n: usize, exponent: f32) -> Option<Vec<f32>> {
  if exponent <= 0. || !exponent.is_finite() {
    return None;
  }

  // Hard-coded mixing factor to ensure that we don't undersample the middle of the shape too
  // severely, even for high exponents.
  const UNIFORM_MIX: f32 = 0.2;

  let n_f = n as f32;
  let power = 2. / exponent;

  let mut nodes = Vec::with_capacity(n);
  for k in 0..n {
    // chebyshev-style angular sampling which ensures first/last samples are near but not exactly
    // 0/1.  This is critical for superellipse profiles where t=0 or t=1 gives zero radius.
    let theta = PI * (2. * k as f32 + 1.) / (2. * n_f);
    let cos_val = theta.cos();

    let transformed = if cos_val.abs() < 1e-10 {
      0.
    } else {
      let sign = cos_val.signum();
      let magnitude = cos_val.abs().powf(power);
      sign * magnitude.clamp(-1., 1.)
    };

    let t_super = 0.5 * (1. - transformed);
    let t_uniform = (2. * k as f32 + 1.) / (2. * n_f);

    let t = t_super * (1. - UNIFORM_MIX) + t_uniform * UNIFORM_MIX;
    if !t.is_finite() {
      return None;
    }

    nodes.push(t.clamp(0., 1.));
  }

  Some(nodes)
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
            if key != "type" && key != "exponent" {
              return Err(ErrorStack::new(format!(
                "Unknown key '{key}' in spine_sampling_scheme map for type \"{type_str}\"; \
                 allowed keys are 'type' and 'exponent'"
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

          match superellipse_nodes(spine_resolution, exponent) {
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
          as_path_sampler(callable).map(|s| SamplerData {
            guides: s.critical_t_values(),
            intervals: s.segment_intervals(),
          })
        }

        let err_expected = || {
          ErrorStack::new(format!(
            "Invalid profile_samplers argument for `rail_sweep`; expected a path sampler, a \
             sequence of samplers, or a function of `|u: float|: Seq<PathSampler> | PathSampler`",
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

      let profile_guide_data = if has_profile {
        collect_profile_guides(ctx, &profile_samplers_val)?
      } else {
        None
      };

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

      let profile = if has_profile {
        Some(profile_val.as_callable().ok_or_else(|| {
          ErrorStack::new(format!(
            "Invalid profile argument for `rail_sweep`; expected Callable, found: {profile_val:?}"
          ))
        })?)
      } else {
        None
      };

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

      // Disable topology-aware sampling optimization when conditions might cause artifacts:
      // 1. Non-constant twist can distort "straight" segments in ways that require more detail
      // 2. Non-straight spines can bend segments, causing shading artifacts with reduced detail
      //
      // TODO: With the new DP-based stitching algorithm, we may be able to re-enable these
      // optimizations even in curved/twisted cases, since the DP algorithm can absorb the
      // resulting vertex drift. This should be investigated after the DP stitching is proven
      // to work well in practice.
      let has_varying_twist = matches!(twist, Twist::Presampled(_));
      let spine_is_straight = spine_is_approximately_straight(&spine_points);
      let should_disable_optimization = has_varying_twist || !spine_is_straight;

      let effective_profile_weights = if should_disable_optimization {
        None
      } else {
        profile_weights
      };

      let mesh = if has_dynamic_profile {
        let dynamic_profile_cb = dynamic_profile_val.as_callable().ok_or_else(|| {
          ErrorStack::new(format!(
            "Invalid dynamic_profile argument for `rail_sweep`; expected Callable, found: \
             {dynamic_profile_val:?}"
          ))
        })?;
        rail_sweep_dynamic(
          ctx,
          &spine_points,
          ring_resolution,
          frame_mode,
          closed,
          capped,
          &twist,
          dynamic_profile_cb,
          fku_stitching,
          Some(&spine_t_values),
          adaptive_profile_sampling,
        )?
      } else {
        let profile =
          profile.expect("profile should be available when dynamic_profile is not used");
        match twist {
          Twist::Const(twist_val) => rail_sweep(
            &spine_points,
            ring_resolution,
            frame_mode,
            closed,
            capped,
            |_, _| Ok(twist_val),
            build_profile_callable(ctx, profile),
            profile_guides,
            effective_profile_weights,
            Some(&spine_t_values),
          )?,
          Twist::Presampled(twist_values) => rail_sweep(
            &spine_points,
            ring_resolution,
            frame_mode,
            closed,
            capped,
            |i, _| Ok(twist_values[i]),
            build_profile_callable(ctx, profile),
            profile_guides,
            effective_profile_weights,
            Some(&spine_t_values),
          )?,
        }
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
  use super::{
    chebyshev_nodes, rail_sweep, rail_sweep_dynamic, superellipse_nodes, FrameMode, Twist,
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
  fn test_superellipse_nodes_properties() {
    // Test superellipse nodes with exponent=2 should be similar to Chebyshev
    let nodes_exp2 = superellipse_nodes(10, 2.0).expect("exponent=2 should work");
    assert_eq!(nodes_exp2.len(), 10);

    // Like Chebyshev, first/last nodes should be near but NOT exactly 0/1
    // This is critical for superellipse profiles where t=0 or t=1 gives zero radius
    assert!(nodes_exp2[0] > 0.0, "First node should be > 0");
    assert!(nodes_exp2[0] < 0.1, "First node should be close to 0");
    assert!(nodes_exp2[9] < 1.0, "Last node should be < 1");
    assert!(nodes_exp2[9] > 0.9, "Last node should be close to 1");

    // Verify nodes are in valid range and strictly increasing
    for (i, &node) in nodes_exp2.iter().enumerate() {
      assert!(
        node >= 0.0 && node <= 1.0,
        "Node {} out of range: {}",
        i,
        node
      );
      if i > 0 {
        assert!(
          node > nodes_exp2[i - 1],
          "Nodes should be strictly increasing"
        );
      }
    }

    // Test that higher exponent concentrates samples more at endpoints
    let nodes_exp2 = superellipse_nodes(10, 2.0).unwrap();
    let nodes_exp5 = superellipse_nodes(10, 5.0).unwrap();
    let nodes_exp10 = superellipse_nodes(10, 10.0).unwrap();

    // Second node (index 1) should be closer to 0 with higher exponent
    // (First node is always exactly 0)
    assert!(
      nodes_exp5[1] < nodes_exp2[1],
      "Higher exponent should give second node closer to 0: exp5={} vs exp2={}",
      nodes_exp5[1],
      nodes_exp2[1]
    );
    assert!(
      nodes_exp10[1] < nodes_exp5[1],
      "Even higher exponent should give second node even closer to 0: exp10={} vs exp5={}",
      nodes_exp10[1],
      nodes_exp5[1]
    );

    // Middle node (index 4 or 5 for n=10) should stay at or near 0.5 for all exponents
    // Note: With even n, the middle is between indices, so we check index 4 and 5
    let mid_exp2 = (nodes_exp2[4] + nodes_exp2[5]) / 2.0;
    let mid_exp5 = (nodes_exp5[4] + nodes_exp5[5]) / 2.0;
    let mid_exp10 = (nodes_exp10[4] + nodes_exp10[5]) / 2.0;
    assert!(
      (mid_exp2 - 0.5).abs() < 0.05,
      "Middle region should be ~0.5 for exp=2, got {}",
      mid_exp2
    );
    assert!(
      (mid_exp5 - 0.5).abs() < 0.05,
      "Middle region should be ~0.5 for exp=5, got {}",
      mid_exp5
    );
    assert!(
      (mid_exp10 - 0.5).abs() < 0.05,
      "Middle region should be ~0.5 for exp=10, got {}",
      mid_exp10
    );

    // Test edge cases
    // Exponent < 2 should still work (samples spread more toward center)
    let nodes_exp1_5 = superellipse_nodes(10, 1.5).expect("exponent=1.5 should work");
    assert_eq!(nodes_exp1_5.len(), 10);
    assert!(
      nodes_exp1_5[1] > nodes_exp2[1],
      "Lower exponent should give second node further from 0: exp1.5={} vs exp2={}",
      nodes_exp1_5[1],
      nodes_exp2[1]
    );

    // Very high exponent should still produce valid nodes
    let nodes_exp50 = superellipse_nodes(10, 50.0).expect("exponent=50 should work");
    assert_eq!(nodes_exp50.len(), 10);
    for &node in &nodes_exp50 {
      assert!(
        node >= 0.0 && node <= 1.0,
        "Node out of range with high exponent"
      );
    }

    // Invalid exponent should return None
    assert!(
      superellipse_nodes(10, 0.0).is_none(),
      "exponent=0 should return None"
    );
    assert!(
      superellipse_nodes(10, -1.0).is_none(),
      "negative exponent should return None"
    );
    assert!(
      superellipse_nodes(10, f32::INFINITY).is_none(),
      "infinite exponent should return None"
    );
    assert!(
      superellipse_nodes(10, f32::NAN).is_none(),
      "NaN exponent should return None"
    );
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

  // Note: DP stitching tests have been moved to fku_stitch.rs module

  #[test]
  fn test_spine_is_approximately_straight_with_straight_spine() {
    use super::spine_is_approximately_straight;

    // Perfectly straight spine
    let straight = vec![
      Vec3::new(0., 0., 0.),
      Vec3::new(0., 0., 1.),
      Vec3::new(0., 0., 2.),
    ];
    assert!(spine_is_approximately_straight(&straight));

    // Straight spine with more points
    let longer_straight = vec![
      Vec3::new(0., 0., 0.),
      Vec3::new(1., 1., 1.),
      Vec3::new(2., 2., 2.),
      Vec3::new(3., 3., 3.),
      Vec3::new(4., 4., 4.),
    ];
    assert!(spine_is_approximately_straight(&longer_straight));

    // Two-point spine is always straight
    let two_points = vec![Vec3::new(0., 0., 0.), Vec3::new(5., 5., 5.)];
    assert!(spine_is_approximately_straight(&two_points));
  }

  #[test]
  fn test_spine_is_approximately_straight_with_curved_spine() {
    use super::spine_is_approximately_straight;

    // Significantly curved spine (should fail the straightness check)
    let curved = vec![
      Vec3::new(0., 0., 0.),
      Vec3::new(5., 0., 5.), // Large deviation from straight line
      Vec3::new(0., 0., 10.),
    ];
    assert!(!spine_is_approximately_straight(&curved));

    // Arc-like spine
    let arc = vec![
      Vec3::new(0., 0., 0.),
      Vec3::new(0.5, 1., 0.),
      Vec3::new(1., 1.5, 0.),
      Vec3::new(1.5, 1., 0.),
      Vec3::new(2., 0., 0.),
    ];
    assert!(!spine_is_approximately_straight(&arc));
  }

  #[test]
  fn test_spine_is_approximately_straight_within_threshold() {
    use super::{spine_is_approximately_straight, SPINE_STRAIGHTNESS_THRESHOLD};

    // Spine with small deviation just within threshold
    // For a line of length 10, 1% threshold means 0.1 max deviation
    let spine_len = 10.0;
    let small_deviation = spine_len * SPINE_STRAIGHTNESS_THRESHOLD * 0.5; // Half the threshold
    let within_threshold = vec![
      Vec3::new(0., 0., 0.),
      Vec3::new(0., small_deviation, 5.),
      Vec3::new(0., 0., 10.),
    ];
    assert!(spine_is_approximately_straight(&within_threshold));

    // Spine with deviation just beyond threshold
    let large_deviation = spine_len * SPINE_STRAIGHTNESS_THRESHOLD * 2.0; // Double the threshold
    let beyond_threshold = vec![
      Vec3::new(0., 0., 0.),
      Vec3::new(0., large_deviation, 5.),
      Vec3::new(0., 0., 10.),
    ];
    assert!(!spine_is_approximately_straight(&beyond_threshold));
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
