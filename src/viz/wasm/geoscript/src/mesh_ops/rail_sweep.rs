use std::{cell::RefCell, f32::consts::PI, rc::Rc};

use bitvec::prelude::*;

use fxhash::{FxHashMap, FxHashSet};
use mesh::{
  linked_mesh::{
    mesh_flags, Arity, Channel, FaceKey, FlipXform, Interp, SpatialXform, Vec3, VertexKey,
  },
  slotmap_utils::vkey,
  LinkedMesh,
};
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

/// One profile loop (subpath) within a ring's vertex run. Single-subpath profiles have exactly
/// one; disjoint or nested (holed) profiles have several, stored in sampler subpath order.
struct LoopInfo {
  start: usize,
  count: usize,
  /// Whether this loop wraps last→first. Open loops stitch without the wrap edge.
  closed: bool,
  /// Loop-local sample t-values, kept so the FKU stitcher's t-similarity penalty stays meaningful
  /// within each loop.
  t_values: Option<Vec<f32>>,
  /// Bitmask marking which samples are critical points (sharp seam vertices) for the DP stitcher.
  critical_mask: Option<BitVec>,
  /// Sign of the loop's shoelace area in profile space (CCW = true), validated against parity.
  winding_positive: bool,
  /// Nesting depth among this ring's loops (0 = outermost). Even = outer (CCW), odd = hole (CW).
  depth: u8,
}

struct RingInfo {
  /// Profile loops in sampler subpath order; loop `k` of ring A stitches to loop `k` of ring B.
  loops: Vec<LoopInfo>,
  /// Plane frame for capping, only stored for first and last rings when capping is enabled.
  cap_frame: Option<super::tessellate_polygon::PlaneFrame>,
  /// Whether the edges connecting vertices within each loop should be marked sharp.
  sharp: bool,
}

/// Global-t span + closedness of one profile subpath (loop). Single-loop profiles have one entry.
#[derive(Clone, Copy)]
struct LoopSpec {
  span: (f32, f32),
  closed: bool,
}

/// How many samples each profile loop gets. `Uniform` gives every loop the same count; `PerLoop`
/// gives loop `k` its own count (length must match the ring's loop count).
enum RingResolution {
  Uniform(usize),
  PerLoop(Vec<usize>),
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
  /// `(t + rotation_offset).rem_euclid(1.0)` to recover the original t. Only ever non-zero for a
  /// single closed loop.
  rotation_offset: f32,
  /// One entry per profile subpath. A single-loop profile (the common case) has one; multi-subpath
  /// (disjoint or holed) profiles have several. Spans are in global t, closedness per subpath.
  loops: Vec<LoopSpec>,
}

struct RingContext {
  center: Vec3,
  tangent: Vec3,
  normal: Vec3,
  binormal: Vec3,
  u_arclen: f32,
  profile_data: DynamicProfileData,
  cap_frame: Option<super::tessellate_polygon::PlaneFrame>,
  collapsed: bool,
}

fn stitch_rings(
  indices: &mut Vec<u32>,
  ring_a: &LoopInfo,
  ring_b: &LoopInfo,
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

/// Infers whether a profile sampler's ring is closed from its subpath topology. A single
/// subpath contributes its own `closed` flag; `None` when topology is unavailable (black-box
/// callable) or there are multiple subpaths (resolved by the multi-subpath machinery).
fn infer_profile_closed(sampler: &Callable) -> Option<bool> {
  match as_path_sampler(sampler)?.subpath_topology()?.as_slice() {
    [single] => Some(single.closed),
    _ => None,
  }
}

/// Builds the per-subpath loop specs for a profile sampler. Topology-less samplers (black-box
/// callables) and single-subpath paths yield one loop; multi-subpath paths yield one spec per
/// subpath (requiring `subpath_t_spans`, and all subpaths must be closed).
fn build_loop_specs(sampler: &Callable) -> Result<Vec<LoopSpec>, ErrorStack> {
  let single_closed = |closed| {
    vec![LoopSpec {
      span: (0., 1.),
      closed,
    }]
  };
  let Some(topo) = as_path_sampler(sampler).and_then(|s| s.subpath_topology()) else {
    return Ok(single_closed(true));
  };
  match topo.len() {
    0 => Ok(single_closed(true)),
    1 => Ok(single_closed(topo[0].closed)),
    n => {
      let Some(spans) = as_path_sampler(sampler).and_then(|s| s.subpath_t_spans()) else {
        return Err(ErrorStack::new(
          "multi-subpath rail_sweep profile requires a real path (trace_path / offset_path / \
           path_join / text_to_path); this sampler can't expose per-subpath spans",
        ));
      };
      if spans.len() != n {
        return Err(ErrorStack::new(
          "internal error: profile subpath span/topology count mismatch",
        ));
      }
      let open: Vec<usize> = topo
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.closed)
        .map(|(i, _)| i)
        .collect();
      if !open.is_empty() {
        return Err(ErrorStack::new(format!(
          "multi-subpath rail_sweep profiles must be all closed, but subpath(s) {open:?} are open. \
           A single open subpath is a valid open profile; mixing open and closed subpaths, or \
           having several open subpaths, is not supported."
        )));
      }
      Ok(
        spans
          .iter()
          .zip(&topo)
          .map(|(&span, t)| LoopSpec {
            span,
            closed: t.closed,
          })
          .collect(),
      )
    }
  }
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
        loops: build_loop_specs(callable)?,
      });
    }
    return Ok(DynamicProfileData {
      sampler: Rc::clone(callable),
      critical_points: vec![0., 1.],
      sharp: false,
      adaptive: None,
      rotation_offset: 0.0,
      loops: vec![LoopSpec {
        span: (0., 1.),
        closed: true,
      }],
    });
  }

  if let Some(map) = value.as_map() {
    let valid_keys = &["sampler", "path_samplers", "sharp", "adaptive", "closed"];
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

    let mut loops = build_loop_specs(sampler)?;
    if let Some(val) = map.get("closed") {
      let c = val
        .as_bool()
        .ok_or_else(|| ErrorStack::new("dynamic_profile 'closed' key must be a boolean value"))?;
      if loops.len() != 1 {
        return Err(ErrorStack::new(
          "dynamic_profile 'closed' key is not valid for a multi-subpath profile; each subpath's \
           openness comes from its own topology",
        ));
      }
      // A single-subpath path sampler must agree with its topology; a black-box sampler adopts it.
      if let Some(topo_closed) = infer_profile_closed(sampler) {
        if topo_closed != c {
          return Err(ErrorStack::new(format!(
            "dynamic_profile `closed: {c}` contradicts the sampler's own topology (its subpath is \
             {}); omit `closed` to use the path's topology",
            if topo_closed { "closed" } else { "open" }
          )));
        }
      }
      loops[0].closed = c;
    }

    return Ok(DynamicProfileData {
      sampler: Rc::clone(sampler),
      critical_points,
      sharp,
      adaptive,
      rotation_offset,
      loops,
    });
  }

  Err(ErrorStack::new(
    "dynamic_profile must return a callable or a map with 'sampler' key",
  ))
}

/// `v_ix` is the vertex index around the ring, or `-1` for out-of-band probes
/// (adaptive sampler, collapse check).
fn sample_profile_offset(
  ctx: &EvalCtx,
  ring: &RingContext,
  v: f32,
  v_ix: i64,
) -> Result<Vec2, ErrorStack> {
  // If the critical points were rotated to align t=0 with a real feature, undo the rotation
  // before calling the sampler so it receives t-values in its original parameterization.
  let v = if ring.profile_data.rotation_offset != 0.0 {
    (v + ring.profile_data.rotation_offset).rem_euclid(1.0)
  } else {
    v
  };
  let offset_2d = ctx
    .invoke_callable(
      &ring.profile_data.sampler,
      &[Value::Float(v), Value::Int(v_ix)],
      EMPTY_KWARGS,
    )
    .map_err(|err| err.wrap("Error calling user-provided sampler returned by `dynamic_profile`"))?;
  offset_2d.as_vec2().copied().ok_or_else(|| {
    ErrorStack::new(format!(
      "Profile sampler must return Vec2, found: {offset_2d:?}"
    ))
  })
}

/// Samples the 3D position by applying the profile offset to the ring's coordinate frame.
fn sample_profile_at(
  ctx: &EvalCtx,
  ring: &RingContext,
  v: f32,
  v_ix: i64,
) -> Result<Vec3, ErrorStack> {
  let offset = sample_profile_offset(ctx, ring, v, v_ix)?;
  Ok(ring.center + ring.normal * offset.x + ring.binormal * offset.y)
}

fn ring_is_collapsed_dynamic(ctx: &EvalCtx, ring: &RingContext) -> Result<bool, ErrorStack> {
  let samples = [0.0, 0.25, 0.5, 0.75];
  let mut first: Option<Vec3> = None;
  let epsilon_sq = COLLAPSE_EPSILON * COLLAPSE_EPSILON;
  for t in samples {
    let p = sample_profile_at(ctx, ring, t, -1)?;
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

/// Signed area of a 2D polygon (shoelace). Positive = counter-clockwise in x-right/y-up space.
fn shoelace_area(pts: &[Vec2]) -> f32 {
  let n = pts.len();
  if n < 3 {
    return 0.0;
  }
  let mut sum = 0.0f32;
  for i in 0..n {
    let p = pts[i];
    let q = pts[(i + 1) % n];
    sum += p.x * q.y - q.x * p.y;
  }
  sum * 0.5
}

/// Even-odd ray cast: whether `pt` lies inside the polygon `poly`.
fn point_in_polygon2d(pt: Vec2, poly: &[Vec2]) -> bool {
  let n = poly.len();
  if n < 3 {
    return false;
  }
  let mut inside = false;
  let mut j = n - 1;
  for i in 0..n {
    let pi = poly[i];
    let pj = poly[j];
    if (pi.y > pt.y) != (pj.y > pt.y) {
      let x_cross = (pj.x - pi.x) * (pt.y - pi.y) / (pj.y - pi.y) + pi.x;
      if pt.x < x_cross {
        inside = !inside;
      }
    }
    j = i;
  }
  inside
}

fn points2d_collapsed(pts: &[Vec2]) -> bool {
  let mut min = Vec2::new(f32::INFINITY, f32::INFINITY);
  let mut max = Vec2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
  for p in pts {
    min.x = min.x.min(p.x);
    min.y = min.y.min(p.y);
    max.x = max.x.max(p.x);
    max.y = max.y.max(p.y);
  }
  (max - min).norm() < COLLAPSE_EPSILON
}

/// Per-loop sample counts for a ring. `Uniform` gives every loop the full count; `PerLoop` gives
/// loop `k` its own count (each floored at 3 closed / 2 open). Errors if a `PerLoop` sequence's
/// length doesn't match the ring's loop count.
fn distribute_loop_budget(
  ring_resolution: &RingResolution,
  loops: &[LoopSpec],
) -> Result<Vec<usize>, ErrorStack> {
  let floor = |l: &LoopSpec| if l.closed { 3 } else { 2 };
  match ring_resolution {
    RingResolution::Uniform(n) => Ok(loops.iter().map(|l| (*n).max(floor(l))).collect()),
    RingResolution::PerLoop(counts) => {
      if counts.len() != loops.len() {
        return Err(ErrorStack::new(format!(
          "`ring_resolution` sequence has {} entries but this profile has {} subpath(s); provide \
           one count per subpath (in sampler subpath order)",
          counts.len(),
          loops.len()
        )));
      }
      Ok(
        loops
          .iter()
          .zip(counts)
          .map(|(l, &n)| n.max(floor(l)))
          .collect(),
      )
    }
  }
}

/// Samples one profile loop over its global-t span, appending vertices / UVs / tangents. Returns
/// the loop's layout (winding sign filled; nesting `depth` left for the caller) plus its sampled
/// 2D profile-space points for the ring's winding/nesting analysis.
fn sample_one_loop(
  ctx: &EvalCtx,
  ring: &RingContext,
  spec: &LoopSpec,
  global_critical: &[f32],
  budget: usize,
  use_adaptive: bool,
  fku_stitching: bool,
  verts: &mut Vec<Vec3>,
  uvs: &mut Vec<[f32; 2]>,
  tangents: &mut Vec<Vec3>,
) -> Result<(LoopInfo, Vec<Vec2>), ErrorStack> {
  let (lo, hi) = spec.span;
  let width = (hi - lo).max(1e-9);

  // Filter the ring's global critical points to this loop's span and remap to loop-local t.
  let mut local_crit: Vec<f32> = global_critical
    .iter()
    .copied()
    .filter(|&c| c >= lo - 1e-6 && c <= hi + 1e-6)
    .map(|c| ((c - lo) / width).clamp(0.0, 1.0))
    .collect();
  local_crit.push(0.0);
  local_crit.push(1.0);
  local_crit.sort_by(f32::total_cmp);
  local_crit.dedup_by(|a, b| (*a - *b).abs() < 1e-6);

  // Global t exactly on a shared subpath boundary resolves to the *earlier* subpath, so a loop
  // starting at an interior boundary would sample its neighbor. Pad interior span ends inward by
  // a negligible fraction; the ends of the full-perimeter span (0 or 1) need no padding.
  let lo_pad = if lo > 1e-6 { width * 1e-3 } else { 0.0 };
  let hi_pad = if hi < 1.0 - 1e-6 { width * 1e-3 } else { 0.0 };
  let sample_lo = lo + lo_pad;
  let sample_width = (width - lo_pad - hi_pad).max(1e-9);
  let to_global = |t_local: f32| sample_lo + t_local * sample_width;

  let mut samples = if use_adaptive {
    adaptive_sample_fallible(
      budget,
      &local_crit,
      |t| sample_profile_offset(ctx, ring, to_global(t), -1),
      1e-5,
    )?
  } else {
    let use_fku = should_use_fku(fku_stitching, budget, budget);
    let base = build_topology_samples(budget, None, None, false);
    if use_fku {
      snap_critical_points(&base, &local_crit, budget)
    } else {
      base
    }
  };
  if !spec.closed {
    samples.push(1.0);
  }

  let crit_mask = {
    let mut mask = bitvec![0; samples.len()];
    for (i, &t) in samples.iter().enumerate() {
      if local_crit.iter().any(|&c| (t - c).abs() < 1e-6) {
        mask.set(i, true);
      }
    }
    Some(mask)
  };

  let start = verts.len();
  let mut pts2d = Vec::with_capacity(samples.len());
  for (v_ix, &t) in samples.iter().enumerate() {
    let off = sample_profile_offset(ctx, ring, to_global(t), v_ix as i64)?;
    pts2d.push(off);
    verts.push(ring.center + ring.normal * off.x + ring.binormal * off.y);
    uvs.push([ring.u_arclen, t]);
    tangents.push(ring.tangent);
  }

  let count = verts.len() - start;
  Ok((
    LoopInfo {
      start,
      count,
      closed: spec.closed,
      winding_positive: shoelace_area(&pts2d) > 0.0,
      depth: 0,
      t_values: Some(samples),
      critical_mask: crit_mask,
    },
    pts2d,
  ))
}

/// Validates that a stitched ring pair shares the same subpath structure (loop count, per-loop
/// closedness, and — for multi-loop profiles — winding and nesting), which the index-based
/// ring-to-ring correspondence relies on.
fn check_ring_consistency(
  a: &RingInfo,
  b: &RingInfo,
  u_a: usize,
  u_b: usize,
) -> Result<(), ErrorStack> {
  if a.loops.len() != b.loops.len() {
    return Err(ErrorStack::new(format!(
      "dynamic_profile changed topology between u_ix={u_a} ({} subpath(s)) and u_ix={u_b} ({} \
       subpath(s)); rail_sweep requires constant subpath structure along the spine",
      a.loops.len(),
      b.loops.len()
    )));
  }
  let multi = a.loops.len() > 1;
  for (k, (la, lb)) in a.loops.iter().zip(&b.loops).enumerate() {
    if la.closed != lb.closed {
      return Err(ErrorStack::new(format!(
        "dynamic_profile subpath {k} changed open/closed between u_ix={u_a} and u_ix={u_b}; \
         rail_sweep requires constant subpath structure along the spine"
      )));
    }
    if multi && (la.winding_positive != lb.winding_positive || la.depth != lb.depth) {
      return Err(ErrorStack::new(format!(
        "dynamic_profile subpath {k} changed winding/nesting between u_ix={u_a} and u_ix={u_b}; \
         rail_sweep requires constant subpath structure along the spine"
      )));
    }
  }
  Ok(())
}

/// Groups an end ring's loops for capping: each even-depth loop (an outer) roots a group and its
/// directly-nested odd-depth loops (holes) attach to it; deeper even-depth loops (islands) root
/// their own groups. Returns groups of loop indices. Uses the projected 2D geometry to find each
/// hole's immediately-enclosing outer.
fn group_end_ring_loops(
  loops: &[LoopInfo],
  verts: &[Vec3],
  frame: &super::tessellate_polygon::PlaneFrame,
) -> Vec<Vec<usize>> {
  let projected: Vec<Vec<Vec2>> = loops
    .iter()
    .map(|l| {
      (l.start..l.start + l.count)
        .map(|i| {
          let rel = verts[i] - frame.center;
          Vec2::new(rel.dot(&frame.u_axis), rel.dot(&frame.v_axis))
        })
        .collect()
    })
    .collect();
  let n = loops.len();

  let mut groups: Vec<Vec<usize>> = Vec::new();
  let mut root_group: FxHashMap<usize, usize> = FxHashMap::default();
  for k in 0..n {
    if loops[k].depth % 2 == 0 {
      root_group.insert(k, groups.len());
      groups.push(vec![k]);
    }
  }
  for k in 0..n {
    if loops[k].depth % 2 == 1 {
      // Immediately-enclosing loop = the max-depth loop containing this hole's first vertex.
      let probe = projected[k][0];
      let parent = (0..n)
        .filter(|&m| m != k && point_in_polygon2d(probe, &projected[m]))
        .max_by_key(|&m| loops[m].depth);
      if let Some(p) = parent {
        if let Some(&g) = root_group.get(&p) {
          groups[g].push(k);
        }
      }
    }
  }
  groups
}

/// Attaches the analytic `uv` (U = cumulative spine arc length, V = profile parameter ∈ [0,1)) and
/// `tangent` (spine direction) vertex channels to a freshly-built sweep mesh, indexed by the dense
/// `vkey(i + 1, 1)` layout `from_indexed_vertices` produces.
fn attach_sweep_attributes(mesh: &mut LinkedMesh<()>, uvs: &[[f32; 2]], tangents: &[Vec3]) {
  let mut uv_ch = Channel::new(Arity::Vec2, Interp::Lerp, FlipXform::Identity, SpatialXform::Identity);
  let mut tan_ch = Channel::new(
    Arity::Vec3,
    Interp::LerpNormalize,
    FlipXform::Negate,
    SpatialXform::Direction,
  );
  for (i, (uv, tan)) in uvs.iter().zip(tangents).enumerate() {
    let key = vkey(i as u32 + 1, 1);
    uv_ch.set(key, [uv[0], uv[1], 0., 0.]);
    tan_ch.set(key, [tan.x, tan.y, tan.z, 0.]);
  }
  mesh.vertex_channels.insert("uv".to_owned(), uv_ch);
  mesh.vertex_channels.insert("tangent".to_owned(), tan_ch);
}

/// Duplicate a boundary ring `[ring_start, ring_start+count)` into cap-owned verts carrying
/// planar profile-space UVs (`offset` projected onto the cap `PlaneFrame`) + an in-plane tangent
/// (`u_axis`). Keeps the tube body's arc-length UV / spine tangent intact on the shared ring and
/// gives the cap edge a sharp normal (cap verts touch only cap faces). Returns the first dup index.
fn dup_cap_ring(
  verts: &mut Vec<Vec3>,
  uvs: &mut Vec<[f32; 2]>,
  tangents: &mut Vec<Vec3>,
  ring_start: usize,
  count: usize,
  frame: &super::tessellate_polygon::PlaneFrame,
  cap_uv_scale: Vec2,
) -> usize {
  let cap_start = verts.len();
  for k in 0..count {
    let p = verts[ring_start + k];
    let d = p - frame.center;
    verts.push(p);
    uvs.push([
      d.dot(&frame.u_axis) * cap_uv_scale.x,
      d.dot(&frame.v_axis) * cap_uv_scale.y,
    ]);
    tangents.push(frame.u_axis);
  }
  cap_start
}

/// Post-normal closed-profile seam split: every ring wraps `last → first` (`v` runs `[0,1)`), so the
/// wrap quad bridges `V≈(n-1)/n` back to the `V=0` seam vertex and crushes the texture across it. For
/// each full ring we clone the seam vertex (inheriting the already-computed smooth normal, so no
/// crease) onto a `V=1` copy and repoint just the wrap faces — those incident to the seam vertex that
/// also touch a last-column vertex. Runs after `attach_sweep_attributes` so the `uv` channel exists.
fn split_profile_seams(mesh: &mut LinkedMesh<()>, rings: impl Iterator<Item = (usize, usize)>) {
  let rings: Vec<(usize, usize)> = rings.filter(|&(_, count)| count >= 3).collect();
  let last_col: FxHashSet<VertexKey> = rings
    .iter()
    .map(|&(start, count)| vkey((start + count - 1) as u32 + 1, 1))
    .collect();
  for &(start, _) in &rings {
    let seam_key = vkey(start as u32 + 1, 1);
    let mut wrap_faces: Vec<FaceKey> = Vec::new();
    for &edge_key in &mesh.vertices[seam_key].edges {
      for &face_key in &mesh.edges[edge_key].faces {
        if mesh.faces[face_key].vertices.iter().any(|v| last_col.contains(v))
          && !wrap_faces.contains(&face_key)
        {
          wrap_faces.push(face_key);
        }
      }
    }
    if wrap_faces.is_empty() {
      continue;
    }
    let clone = mesh.split_off_faces(seam_key, &wrap_faces);
    if let Some(uv_ch) = mesh.vertex_channels.get_mut("uv") {
      if let Some(mut uv) = uv_ch.get(seam_key) {
        uv[1] = 1.;
        uv_ch.set(clone, uv);
      }
    }
  }
}

/// Finalize a `split_seams` mesh: compute shading normals with the seam still SHARED (so it stays
/// smooth), then split the profile seam — each clone inherits its source's smooth normal, so the
/// seam reads seamless. The complete shading normals make the render pipeline skip its auto-smooth
/// recompute; `NO_WELD` additionally stops its merge-by-distance from welding the coincident seam/cap
/// duplicates back together.
fn finalize_split_seams(mesh: &mut LinkedMesh<()>, rings: impl Iterator<Item = (usize, usize)>) {
  mesh.separate_vertices_and_compute_normals();
  split_profile_seams(mesh, rings);
  mesh.flags |= mesh_flags::NO_WELD;
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
  split_seams: bool,
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
  let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(spine_points.len() * ring_resolution + 2);
  let mut tangents: Vec<Vec3> = Vec::with_capacity(spine_points.len() * ring_resolution + 2);
  let mut ring_infos: Vec<RingInfo> = Vec::with_capacity(spine_points.len());

  let u_denom = (frames.len() - 1) as f32;
  let mut u_arclen = 0f32;
  let mut prev_center: Option<Vec3> = None;

  for (u_ix, frame) in frames.iter().enumerate() {
    if let Some(pc) = prev_center {
      u_arclen += (frame.center - pc).norm();
    }
    prev_center = Some(frame.center);

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
      uvs.push([u_arclen, 0.]);
      tangents.push(frame.tangent);
      ring_infos.push(RingInfo {
        loops: vec![LoopInfo {
          start,
          count: 1,
          closed: true,
          t_values: None,
          critical_mask: None,
          winding_positive: true,
          depth: 0,
        }],
        cap_frame: None,
        sharp: false,
      });
    } else {
      let start = verts.len();
      verts.extend(ring);
      for &v_norm in &v_samples {
        uvs.push([u_arclen, v_norm]);
        tangents.push(frame.tangent);
      }
      ring_infos.push(RingInfo {
        loops: vec![LoopInfo {
          start,
          count: ring_resolution,
          closed: true,
          t_values: None,
          critical_mask: None,
          winding_positive: true,
          depth: 0,
        }],
        cap_frame,
        sharp: false,
      });
    }
  }

  let mut indices: Vec<u32> = Vec::with_capacity(spine_points.len() * ring_resolution * 6);

  for i in 0..(ring_infos.len() - 1) {
    stitch_rings(
      &mut indices,
      &ring_infos[i].loops[0],
      &ring_infos[i + 1].loops[0],
      ring_resolution,
    );
  }

  if closed {
    let r_last = &ring_infos[ring_infos.len() - 1].loops[0];
    let r_first = &ring_infos[0].loops[0];
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
      let (ring_start, count, cap_frame) = {
        let ri = &ring_infos[ring_ix];
        (ri.loops[0].start, ri.loops[0].count, ri.cap_frame.clone())
      };
      if count != ring_resolution {
        continue;
      }
      let frame =
        cap_frame.expect("cap_frame should always be set for end rings when capping is enabled");

      // `split_seams` duplicates the boundary ring so the cap carries planar UVs/tangents (at the
      // cost of watertight topology); otherwise the cap reuses the shared ring verts (2-manifold).
      let cap_base = if split_seams {
        dup_cap_ring(
          &mut verts,
          &mut uvs,
          &mut tangents,
          ring_start,
          count,
          &frame,
          Vec2::new(1., 1.),
        )
      } else {
        ring_start
      };
      let ring_slice = &verts[cap_base..(cap_base + ring_resolution)];
      let cap_indices = super::tessellate_polygon::tessellate_ring_cap_with_frame(
        ring_slice,
        cap_base,
        reverse_winding,
        &frame,
      )?;
      indices.extend(cap_indices);
    }
  }

  let mut mesh = LinkedMesh::from_indexed_vertices(&verts, &indices, None, None);
  attach_sweep_attributes(&mut mesh, &uvs, &tangents);
  if split_seams {
    finalize_split_seams(
      &mut mesh,
      ring_infos.iter().map(|r| (r.loops[0].start, r.loops[0].count)),
    );
  }
  Ok(mesh)
}

fn rail_sweep_dynamic(
  ctx: &EvalCtx,
  spine_points: &[Vec3],
  ring_resolution: &RingResolution,
  frame_mode: FrameMode,
  closed: bool,
  capped: bool,
  capped_explicit: bool,
  twist: &Twist,
  dynamic_profile_cb: &Rc<Callable>,
  fku_stitching: bool,
  spine_u_values: Option<&[f32]>,
  adaptive_profile_sampling: bool,
  split_seams: bool,
  cap_uv_scale: Vec2,
) -> Result<LinkedMesh<()>, ErrorStack> {
  let frames = calculate_spine_frames(spine_points, frame_mode)?;
  let u_denom = (frames.len() - 1) as f32;

  let mut ring_contexts: Vec<RingContext> = Vec::with_capacity(frames.len());
  let mut u_arclen = 0f32;
  let mut prev_center: Option<Vec3> = None;
  for (u_ix, frame) in frames.iter().enumerate() {
    if let Some(pc) = prev_center {
      u_arclen += (frame.center - pc).norm();
    }
    prev_center = Some(frame.center);

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
      &[
        Value::Float(u),
        Value::Int(u_ix as i64),
        Value::Vec3(frame.center),
      ],
      EMPTY_KWARGS,
    )?;
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
      tangent: frame.tangent,
      normal,
      binormal,
      u_arclen,
      profile_data,
      cap_frame,
      collapsed: false,
    };
    // Collapse-to-apex only applies to single-loop profiles; a vanishing loop in a multi-loop
    // ring is a genus change and is rejected during sampling instead.
    ring.collapsed =
      ring.profile_data.loops.len() == 1 && ring_is_collapsed_dynamic(ctx, &ring)?;
    ring_contexts.push(ring);
  }

  // Open profiles sweep into a sheet with no closed boundary ring, so they can't be capped. An
  // explicit `capped=true` is an error; the default-true just falls back to uncapped.
  let any_profile_open = ring_contexts
    .iter()
    .any(|r| r.profile_data.loops.iter().any(|l| !l.closed));
  let effective_capped = if capped && !closed && any_profile_open {
    if capped_explicit {
      return Err(ErrorStack::new(
        "`capped=true` is invalid for an open profile: an open profile sweeps into a sheet with \
         no closed boundary ring to triangulate. Remove `capped` or set it to false.",
      ));
    }
    false
  } else {
    capped
  };

  let mut verts: Vec<Vec3> = Vec::new();
  let mut uvs: Vec<[f32; 2]> = Vec::new();
  let mut tangents: Vec<Vec3> = Vec::new();
  let mut indices: Vec<u32> = Vec::new();

  let mut sampled_rings: Vec<RingInfo> = Vec::with_capacity(ring_contexts.len());
  for (u_ix, ring) in ring_contexts.into_iter().enumerate() {
    // Single-loop collapse to a shared apex vertex (cones/tips).
    if ring.collapsed {
      let start = verts.len();
      let apex = sample_profile_at(ctx, &ring, 0.0, 0)?;
      verts.push(apex);
      uvs.push([ring.u_arclen, 0.]);
      tangents.push(ring.tangent);
      sampled_rings.push(RingInfo {
        loops: vec![LoopInfo {
          start,
          count: 1,
          closed: ring.profile_data.loops[0].closed,
          t_values: None,
          critical_mask: None,
          winding_positive: true,
          depth: 0,
        }],
        cap_frame: ring.cap_frame.clone(),
        sharp: ring.profile_data.sharp,
      });
      continue;
    }

    let use_adaptive = ring
      .profile_data
      .adaptive
      .unwrap_or(adaptive_profile_sampling);
    let budgets = distribute_loop_budget(ring_resolution, &ring.profile_data.loops)?;
    let n_loops = ring.profile_data.loops.len();

    let mut loops: Vec<LoopInfo> = Vec::with_capacity(n_loops);
    let mut loop_pts: Vec<Vec<Vec2>> = Vec::with_capacity(n_loops);
    for (spec, &budget) in ring.profile_data.loops.iter().zip(&budgets) {
      let (info, pts2d) = sample_one_loop(
        ctx,
        &ring,
        spec,
        &ring.profile_data.critical_points,
        budget,
        use_adaptive,
        fku_stitching,
        &mut verts,
        &mut uvs,
        &mut tangents,
      )?;
      if n_loops > 1 && (info.count < 3 || points2d_collapsed(&pts2d)) {
        return Err(ErrorStack::new(format!(
          "dynamic_profile subpath {} collapsed at u_ix={u_ix}; a vanishing loop in a \
           multi-subpath profile changes the genus and is not supported",
          loops.len(),
        )));
      }
      loops.push(info);
      loop_pts.push(pts2d);
    }

    // Multi-loop: derive nesting depth (even-odd ray cast against the other loops) and enforce
    // the winding convention — outers (even depth) CCW, holes (odd depth) CW.
    if n_loops > 1 {
      for k in 0..n_loops {
        let probe = loop_pts[k][0];
        let depth = (0..n_loops)
          .filter(|&m| m != k && point_in_polygon2d(probe, &loop_pts[m]))
          .count();
        loops[k].depth = depth as u8;
        let want_positive = depth % 2 == 0;
        if loops[k].winding_positive != want_positive {
          return Err(ErrorStack::new(format!(
            "dynamic_profile subpath {k} at u_ix={u_ix} has {} winding but nesting depth {depth}; \
             outers (even depth) must be CCW and holes (odd depth) CW (see notes/paths.md)",
            if loops[k].winding_positive { "CCW" } else { "CW" },
          )));
        }
      }
    }

    sampled_rings.push(RingInfo {
      loops,
      cap_frame: ring.cap_frame.clone(),
      sharp: ring.profile_data.sharp,
    });
  }

  // Index-based ring-to-ring correspondence requires constant subpath structure along the spine.
  for i in 1..sampled_rings.len() {
    check_ring_consistency(&sampled_rings[i - 1], &sampled_rings[i], i - 1, i)?;
  }
  if closed && sampled_rings.len() >= 2 {
    let last = sampled_rings.len() - 1;
    check_ring_consistency(&sampled_rings[last], &sampled_rings[0], last, 0)?;
  }

  // Stitch loop k of one ring to loop k of the next (correspondence is by index). Each loop pair
  // is exactly the single-ring stitch: apex fans, FKU DP, or uniform quads.
  let stitch_pair = |idx_a: usize, idx_b: usize, indices: &mut Vec<u32>| {
    let r_a = &sampled_rings[idx_a];
    let r_b = &sampled_rings[idx_b];
    for (la, lb) in r_a.loops.iter().zip(&r_b.loops) {
      if la.count == 1 && lb.count == 1 {
        continue;
      }
      if la.count == 1 {
        stitch_apex_to_row(la.start, lb.start, lb.count, lb.closed, true, true, indices);
        continue;
      }
      if lb.count == 1 {
        stitch_apex_to_row(lb.start, la.start, la.count, la.closed, false, false, indices);
        continue;
      }
      let v_closed = la.closed;
      if should_use_fku(fku_stitching, la.count, lb.count) {
        let pts_a = &verts[la.start..la.start + la.count];
        let pts_b = &verts[lb.start..lb.start + lb.count];
        dp_stitch_presampled(
          pts_a,
          pts_b,
          la.t_values.as_deref(),
          lb.t_values.as_deref(),
          la.critical_mask.as_deref(),
          lb.critical_mask.as_deref(),
          la.start,
          lb.start,
          v_closed,
          indices,
        );
      } else {
        let count = la.count.min(lb.count);
        uniform_stitch_rows(la.start, lb.start, count, v_closed, true, indices);
      }
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

  if effective_capped && !closed && sampled_rings.len() >= 2 {
    for (ring_ix, reverse_winding) in [(0usize, false), (sampled_rings.len() - 1, true)] {
      let frame = sampled_rings[ring_ix]
        .cap_frame
        .clone()
        .expect("cap_frame should always be set for end rings when capping is enabled");

      // Group the ring's loops by nesting (outer + its holes); disjoint outers get their own
      // groups. Grouping borrows `verts` immutably, so gather layouts before any dup mutation.
      let groups = group_end_ring_loops(&sampled_rings[ring_ix].loops, &verts, &frame);
      for group in &groups {
        let loop_meta: Vec<(usize, usize)> = group
          .iter()
          .map(|&li| {
            let l = &sampled_rings[ring_ix].loops[li];
            (l.start, l.count)
          })
          .collect();
        // A collapsed apex (count 1) has no boundary ring to cap.
        if loop_meta.iter().any(|&(_, count)| count < 3) {
          continue;
        }

        // `split_seams` gives caps their own planar-UV verts (non-watertight); otherwise caps
        // reuse the shared ring verts (2-manifold).
        let cap_bases: Vec<usize> = if split_seams {
          loop_meta
            .iter()
            .map(|&(start, count)| {
              dup_cap_ring(
                &mut verts,
                &mut uvs,
                &mut tangents,
                start,
                count,
                &frame,
                cap_uv_scale,
              )
            })
            .collect()
        } else {
          loop_meta.iter().map(|&(start, _)| start).collect()
        };

        let cap_indices = if loop_meta.len() == 1 {
          let (_, count) = loop_meta[0];
          let base = cap_bases[0];
          super::tessellate_polygon::tessellate_ring_cap_with_frame(
            &verts[base..base + count],
            base,
            reverse_winding,
            &frame,
          )?
        } else {
          let loop_slices: Vec<&[Vec3]> = loop_meta
            .iter()
            .zip(&cap_bases)
            .map(|(&(_, count), &base)| &verts[base..base + count])
            .collect();
          let base_indices: Vec<u32> = cap_bases.iter().map(|&b| b as u32).collect();
          super::tessellate_polygon::tessellate_ring_cap_with_holes(
            &loop_slices,
            &base_indices,
            reverse_winding,
            &frame,
          )?
        };
        indices.extend(cap_indices);
      }
    }
  }

  let mut mesh = LinkedMesh::from_indexed_vertices(&verts, &indices, None, None);

  // For any rings marked sharp, annotate each loop's within-ring edges (skipping the wrap edge on
  // open loops) so the crease survives shading-normal computation.
  //
  // `from_indexed_vertices` guarantees that `VertexKey`s map back to original vtx indices as:
  // `{ ix: vtx_ix + 1, version: 1 }`
  for ring in sampled_rings.iter() {
    if !ring.sharp {
      continue;
    }
    for loop_info in &ring.loops {
      let edge_count = if loop_info.closed {
        loop_info.count
      } else {
        loop_info.count.saturating_sub(1)
      };
      for j in 0..edge_count {
        let v0_ix = loop_info.start + j;
        let v1_ix = loop_info.start + (j + 1) % loop_info.count;
        let vkeys = [vkey(v0_ix as u32 + 1, 1), vkey(v1_ix as u32 + 1, 1)];
        if let Some(edge_key) = mesh.get_edge_key(vkeys) {
          if let Some(edge) = mesh.edges.get_mut(edge_key) {
            edge.sharp = true;
          }
        }
      }
    }
  }

  attach_sweep_attributes(&mut mesh, &uvs, &tangents);
  if split_seams {
    // Only closed loops have a wrap seam to split; open loops already span V∈[0,1].
    finalize_split_seams(
      &mut mesh,
      sampled_rings
        .iter()
        .flat_map(|r| r.loops.iter())
        .filter(|l| l.closed)
        .map(|l| (l.start, l.count)),
    );
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

fn is_passthrough_str(s: &str) -> bool {
  s.eq_ignore_ascii_case("passthrough")
    || s.eq_ignore_ascii_case("pass_through")
    || s.eq_ignore_ascii_case("as_is")
    || s.eq_ignore_ascii_case("as-is")
    || s.eq_ignore_ascii_case("raw")
}

fn is_passthrough_spine_scheme(scheme: &Value) -> bool {
  match scheme {
    Value::String(s) => is_passthrough_str(s),
    Value::Map(map) => map
      .get("type")
      .and_then(|v| v.as_str())
      .map(is_passthrough_str)
      .unwrap_or(false),
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
/// - `bevel_fraction`: fraction of domain at each end that is the bevel zone (0..0.5). Default when
///   called with just an exponent: `(0.5 / exponent).clamp(0.05, 0.25)`.
/// - `density`: how many times denser the bevel zones are vs the middle (>= 1.0). Controls both
///   sample allocation and the power-curve concentration within bevel zones.
///
/// Returns `None` if parameters are invalid or produce degenerate results.
fn superellipse_nodes_with_params(n: usize, bevel_fraction: f32, density: f32) -> Option<Vec<f32>> {
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
    let u_ix = match args.get(1) {
      Some(Value::Int(i)) => *i,
      Some(Value::Float(f)) => *f as i64,
      _ => 0,
    };
    let spine_center = match args.get(2) {
      Some(Value::Vec3(v)) => *v,
      _ => Vec3::new(0., 0., 0.),
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
    // Wrap a |u, v, u_ix, v_ix, spine_center| callable: captures the spine-level args and
    // forwards `v`/`v_ix` from each per-vertex call.
    let inner = Rc::new(Callable::Dynamic {
      name: "static_profile_inner".to_owned(),
      inner: Box::new(StaticProfileInner {
        profile: Rc::clone(&self.profile),
        u,
        u_ix,
        spine_center,
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
  u_ix: i64,
  spine_center: Vec3,
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
    // -1 signals out-of-band probes (adaptive sampler, collapse check).
    let v_ix = match args.get(1) {
      Some(Value::Int(i)) => *i,
      Some(Value::Float(f)) => *f as i64,
      _ => -1,
    };
    ctx
      .invoke_callable(
        &self.profile,
        &[
          Value::Float(self.u),
          Value::Float(v),
          Value::Int(self.u_ix),
          Value::Int(v_ix),
          Value::Vec3(self.spine_center),
        ],
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
      if spine_resolution < 2 {
        return Err(ErrorStack::new(format!(
          "Invalid spine_resolution for `rail_sweep`; expected >= 2, found: {spine_resolution}"
        )));
      }
      let spine_resolution = spine_resolution as usize;

      let check_ring_res = |n: i64, at: &str| -> Result<usize, ErrorStack> {
        if n < 3 {
          return Err(ErrorStack::new(format!(
            "Invalid ring_resolution{at} for `rail_sweep`; expected >= 3, found: {n}"
          )));
        }
        Ok(n as usize)
      };
      let ring_resolution_val = arg_refs[1].resolve(args, kwargs);
      let ring_resolution = match ring_resolution_val {
        Value::Sequence(seq) => {
          let mut counts = Vec::new();
          for (ix, res) in seq.consume(ctx).enumerate() {
            let val = res?;
            let n = val.as_int().ok_or_else(|| {
              ErrorStack::new(format!(
                "Invalid `ring_resolution` sequence entry at index {ix} for `rail_sweep`; \
                 expected int, found: {val:?}"
              ))
            })?;
            counts.push(check_ring_res(n, &format!(" at index {ix}"))?);
          }
          if counts.is_empty() {
            return Err(ErrorStack::new(
              "`ring_resolution` sequence for `rail_sweep` must have at least one entry",
            ));
          }
          RingResolution::PerLoop(counts)
        }
        other => {
          let n = other.as_int().ok_or_else(|| {
            ErrorStack::new(format!(
              "Invalid ring_resolution for `rail_sweep`; expected int or sequence of ints, found: \
               {other:?}"
            ))
          })?;
          RingResolution::Uniform(check_ring_res(n, "")?)
        }
      };

      let spine = arg_refs[2].resolve(args, kwargs);
      let profile_val = arg_refs[3].resolve(args, kwargs);
      let frame_mode_val = arg_refs[4].resolve(args, kwargs);
      let twist_val = arg_refs[5].resolve(args, kwargs);
      let closed = arg_refs[6].resolve(args, kwargs).as_bool().unwrap();
      let capped = arg_refs[7].resolve(args, kwargs).as_bool().unwrap();
      // Distinguishes an explicit `capped=true` (an error for open profiles) from the default.
      let capped_explicit = !matches!(&arg_refs[7], ArgRef::Default(_));
      let profile_samplers_val = arg_refs[8].resolve(args, kwargs);
      let dynamic_profile_val = arg_refs[9].resolve(args, kwargs);
      let fku_stitching = arg_refs[10].resolve(args, kwargs).as_bool().unwrap();
      let spine_sampling_scheme_val = arg_refs[11].resolve(args, kwargs);
      let adaptive_profile_sampling = arg_refs[12].resolve(args, kwargs).as_bool().unwrap();
      let split_seams = arg_refs[13].resolve(args, kwargs).as_bool().unwrap();
      let cap_uv_scale = *arg_refs[14].resolve(args, kwargs).as_vec2().unwrap();

      let use_adaptive_spine = is_adaptive_spine_scheme(&spine_sampling_scheme_val);
      let explicit_passthrough = is_passthrough_spine_scheme(&spine_sampling_scheme_val);
      let scheme_is_default = matches!(spine_sampling_scheme_val, Value::Nil);
      // Sequence spines default to using points as-is; explicit passthrough is the
      // same but with a strict length check.
      let use_passthrough = explicit_passthrough || scheme_is_default;

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
        } else if use_passthrough {
          if raw_points.len() != spine_resolution {
            let hint = if explicit_passthrough {
              "spine_sampling_scheme \"passthrough\" requires the sequence length to match \
               spine_resolution"
            } else {
              "by default, `rail_sweep` uses the provided spine points as-is when spine is a \
               sequence. Either set spine_resolution to match the sequence length, or pass \
               spine_sampling_scheme=\"uniform\" (or another scheme) to resample"
            };
            return Err(ErrorStack::new(format!(
              "`rail_sweep` spine sequence has {} points, but \
               spine_resolution={spine_resolution}. {hint}",
              raw_points.len(),
            )));
          }

          // Use points directly; u is normalized cumulative arc length so profile
          // callbacks still receive a fractional arc-length position.
          let mut cumulative = Vec::with_capacity(raw_points.len());
          cumulative.push(0.0_f32);
          for i in 1..raw_points.len() {
            let seg_len = (raw_points[i] - raw_points[i - 1]).norm();
            cumulative.push(cumulative[i - 1] + seg_len);
          }
          let total = *cumulative.last().unwrap_or(&0.0);
          let t_values: Vec<f32> = if total > 0. {
            cumulative.iter().map(|c| c / total).collect()
          } else {
            uniform_nodes(raw_points.len())
          };
          (t_values, raw_points)
        } else {
          // Explicit non-passthrough scheme: resample the polyline at the scheme's t-values.
          let spine_t_values =
            compute_spine_t_values(ctx, &spine_sampling_scheme_val, spine_resolution)?;
          let points = resample_spine_points_at_t(&raw_points, &spine_t_values)?;
          (spine_t_values, points)
        }
      } else if let Some(cb) = spine.as_callable() {
        if explicit_passthrough {
          return Err(ErrorStack::new(
            "`rail_sweep` spine_sampling_scheme \"passthrough\" requires a sequence spine; \
             callable spines have no inherent point set to preserve",
          ));
        }
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
        &ring_resolution,
        frame_mode,
        closed,
        capped,
        capped_explicit,
        &twist,
        &dynamic_profile_cb,
        fku_stitching,
        Some(&spine_t_values),
        adaptive_profile_sampling,
        split_seams,
        cap_uv_scale,
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
    chebyshev_nodes, mesh_flags, rail_sweep, rail_sweep_dynamic, superellipse_nodes,
    superellipse_nodes_with_params, FrameMode, RingResolution, Twist,
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
      false,
    )
    .unwrap();

    assert_eq!(mesh.vertices.len(), 8);
    assert_eq!(mesh.faces.len(), 8);
  }

  #[test]
  fn rail_sweep_emits_uv_and_tangent_channels() {
    use mesh::linked_mesh::ChannelStore;
    use mesh::slotmap_utils::vkey;

    // Straight spine along +Z with unit segments → ring U = cumulative arc length 0, 1, 2.
    let spine = vec![
      Vec3::new(0., 0., 0.),
      Vec3::new(0., 0., 1.),
      Vec3::new(0., 0., 2.),
    ];
    let res = 4;
    let mesh = rail_sweep(
      &spine,
      res,
      FrameMode::Rmf,
      false,
      false,
      |_, _| Ok(0.),
      |_, v_norm, _, _, _| {
        let angle = v_norm * std::f32::consts::TAU;
        Ok(Vec2::new(angle.cos(), angle.sin()))
      },
      None,
      None,
      None,
      true,
    )
    .unwrap();

    let ChannelStore::Vec2(uv) = &mesh.vertex_channels["uv"].store else {
      panic!("uv channel missing or wrong arity");
    };
    let ChannelStore::Vec3(tan) = &mesh.vertex_channels["tangent"].store else {
      panic!("tangent channel missing or wrong arity");
    };
    // 3 rings x 4 verts, plus one V=1 seam clone per ring from the closed-profile seam split.
    assert_eq!(uv.len(), 15, "3 rings x 4 verts + 3 seam clones");
    assert_eq!(tan.len(), 15);
    let seam_clones = uv.values().filter(|uv| (uv[1] - 1.).abs() < 1e-5).count();
    assert_eq!(seam_clones, 3, "one V=1 seam clone per ring");

    // V = profile parameter; ring vertex 0 (the seam vertex) keeps V=0 (its clone carries V=1).
    let samples = build_topology_samples(res, None, None, false);
    for (ring_ix, expected_u) in [0f32, 1., 2.].iter().enumerate() {
      let v0 = vkey((ring_ix * res) as u32 + 1, 1);
      let [u, v] = uv[v0];
      assert!((u - expected_u).abs() < 1e-5, "ring {ring_ix} U: {u} vs {expected_u}");
      assert!((v - samples[0]).abs() < 1e-5, "ring {ring_ix} V: {v} vs {}", samples[0]);
    }

    // Tangent tracks the spine direction (+Z) for every vertex.
    for t in tan.values() {
      let dir = Vec3::new(t[0], t[1], t[2]);
      assert!((dir - Vec3::new(0., 0., 1.)).norm() < 1e-5, "tangent not +Z: {dir:?}");
    }
  }

  #[test]
  fn split_seams_gives_caps_planar_uvs() {
    use mesh::linked_mesh::ChannelStore;

    // Capped circular tube (radius 1.5). Cap planar UVs = profile coords, so they reach negative
    // U; the swept tube body's U = arc length is always >= 0. The presence of negative-U verts is
    // therefore a direct signal that the cap duplication ran.
    let build = |split_seams: bool| {
      let spine = vec![Vec3::new(0., 0., 0.), Vec3::new(0., 0., 1.)];
      let mesh = rail_sweep(
        &spine,
        24,
        FrameMode::Rmf,
        false,
        true,
        |_, _| Ok(0.),
        |_, v_norm, _, _, _| {
          let a = v_norm * std::f32::consts::TAU;
          Ok(Vec2::new(a.cos() * 1.5, a.sin() * 1.5))
        },
        None,
        None,
        None,
        split_seams,
      )
      .unwrap();
      let ChannelStore::Vec2(uv) = &mesh.vertex_channels["uv"].store else {
        panic!("uv channel missing");
      };
      let min_u = uv.values().map(|uv| uv[0]).fold(f32::INFINITY, f32::min);
      // split_seams finalizes its own normals so the render pipeline leaves the mesh alone.
      let normals_complete = mesh.shading_normals.len() == mesh.vertices.len();
      (min_u, normals_complete, mesh.flags)
    };

    let (min_u_plain, normals_plain, flags_plain) = build(false);
    let (min_u_split, normals_split, flags_split) = build(true);
    assert!(min_u_plain >= -1e-6, "without split_seams caps reuse tube UV (U = arc length >= 0)");
    assert!(!normals_plain, "without split_seams normals are left to the render pipeline");
    assert_eq!(flags_plain, 0, "without split_seams the mesh gets default render finalize");
    assert!(min_u_split < -1.0, "with split_seams caps carry planar profile-coord UVs (reach -1.5)");
    assert!(normals_split, "with split_seams every vertex carries a finalized shading normal");
    assert!(
      flags_split & mesh_flags::NO_WELD != 0,
      "with split_seams the mesh opts out of the distance-weld to preserve its seam duplicates"
    );
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
      false,
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
    assert!(
      *nodes.last().unwrap() > 0.95,
      "Last node should be close to 1"
    );

    // All nodes in range and strictly increasing
    for (i, &node) in nodes.iter().enumerate() {
      assert!(
        node >= 0.0 && node <= 1.0,
        "Node {} out of range: {}",
        i,
        node
      );
      if i > 0 {
        assert!(
          node > nodes[i - 1],
          "Nodes should be strictly increasing at {i}"
        );
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

    assert!(
      in_start_bevel >= 2,
      "Should have multiple samples in start bevel, got {in_start_bevel}"
    );
    assert!(
      in_end_bevel >= 2,
      "Should have multiple samples in end bevel, got {in_end_bevel}"
    );
    assert!(
      in_middle >= in_start_bevel,
      "Middle should have at least as many samples as a bevel zone"
    );
    assert_eq!(in_start_bevel + in_end_bevel + in_middle, 20);
  }

  #[test]
  fn test_superellipse_nodes_bevel_concentration() {
    // Within the start bevel zone, samples should be denser near 0 (the corner)
    let nodes = superellipse_nodes_with_params(20, 0.15, 4.0).unwrap();

    // Collect start bevel samples
    let bevel_samples: Vec<f32> = nodes.iter().copied().filter(|&t| t < 0.15).collect();
    assert!(
      bevel_samples.len() >= 3,
      "Need enough bevel samples to test concentration"
    );

    // Gaps should increase as we move away from 0
    let gaps: Vec<f32> = bevel_samples.windows(2).map(|w| w[1] - w[0]).collect();
    for i in 1..gaps.len() {
      assert!(
        gaps[i] >= gaps[i - 1] * 0.9, // allow small tolerance
        "Bevel gaps should generally increase away from corner: gap[{}]={} vs gap[{}]={}",
        i - 1,
        gaps[i - 1],
        i,
        gaps[i]
      );
    }
  }

  #[test]
  fn test_superellipse_nodes_middle_is_uniform() {
    let nodes = superellipse_nodes_with_params(30, 0.1, 3.0).unwrap();

    // Collect middle samples
    let middle: Vec<f32> = nodes
      .iter()
      .copied()
      .filter(|&t| t >= 0.1 && t <= 0.9)
      .collect();
    assert!(middle.len() >= 5);

    // Middle gaps should be approximately equal
    let gaps: Vec<f32> = middle.windows(2).map(|w| w[1] - w[0]).collect();
    let avg_gap = gaps.iter().sum::<f32>() / gaps.len() as f32;
    for (i, &gap) in gaps.iter().enumerate() {
      assert!(
        (gap - avg_gap).abs() < avg_gap * 0.15,
        "Middle gap {} = {} deviates too much from avg {}",
        i,
        gap,
        avg_gap
      );
    }
  }

  #[test]
  fn test_superellipse_nodes_exponent_affects_distribution() {
    // Higher exponent -> smaller bevel_fraction -> fewer samples in bevel, more in middle
    let nodes_exp2 = superellipse_nodes(20, 2.0).unwrap(); // bevel_fraction=0.25
    let nodes_exp10 = superellipse_nodes(20, 10.0).unwrap(); // bevel_fraction=0.05

    // With exp=10, the bevel zone is [0, 0.05] ∪ [0.95, 1], much smaller than exp=2's [0, 0.25] ∪
    // [0.75, 1] So exp=10 should have more samples concentrated in the middle region [0.25,
    // 0.75]
    let mid_count_exp2 = nodes_exp2
      .iter()
      .filter(|&&t| t >= 0.25 && t <= 0.75)
      .count();
    let mid_count_exp10 = nodes_exp10
      .iter()
      .filter(|&&t| t >= 0.25 && t <= 0.75)
      .count();
    assert!(
      mid_count_exp10 > mid_count_exp2,
      "Higher exponent should have more samples in center: exp10={} vs exp2={}",
      mid_count_exp10,
      mid_count_exp2
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
        i,
        nodes[i],
        mirror,
        nodes[mirror],
        sum
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
      false,
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
      false,
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
      &RingResolution::Uniform(12),
      FrameMode::Rmf,
      false,
      true,
      true,
      &twist,
      &dynamic_profile_cb,
      true,
      None,
      false,
      false,
      Vec2::new(1., 1.),
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
      &RingResolution::Uniform(16),
      FrameMode::Rmf,
      false,
      true,
      true,
      &twist,
      &dynamic_profile_cb,
      true,
      None,
      false,
      false,
      Vec2::new(1., 1.),
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

  /// Open profile declared via `closed: false` on the dynamic_profile map: a black-box
  /// semicircle swept along a straight spine produces an open sheet — a 2-manifold *with
  /// boundary* — and the profile V coordinate reaches 1.0 (the appended endpoint sample).
  #[test]
  fn rail_sweep_open_profile_is_manifold_with_boundary() {
    use mesh::linked_mesh::ChannelStore;

    struct Semicircle;
    impl DynamicCallable for Semicircle {
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
        let Some(Value::Float(v)) = args.first() else {
          return Err(ErrorStack::new("expected float"));
        };
        let a = v * PI;
        Ok(Value::Vec2(Vec2::new(a.cos(), a.sin())))
      }
      fn get_return_type_hint(&self) -> Option<crate::ArgType> {
        Some(crate::ArgType::Vec2)
      }
    }

    struct OpenProfile {
      sampler: Rc<Callable>,
    }
    impl DynamicCallable for OpenProfile {
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
        let mut map = FxHashMap::default();
        map.insert("sampler".to_owned(), Value::Callable(Rc::clone(&self.sampler)));
        map.insert("closed".to_owned(), Value::Bool(false));
        Ok(Value::Map(Rc::new(map)))
      }
      fn get_return_type_hint(&self) -> Option<crate::ArgType> {
        None
      }
    }

    let ctx = EvalCtx::default();
    let sampler = Rc::new(Callable::Dynamic {
      name: "semicircle".to_owned(),
      inner: Box::new(Semicircle),
    });
    let dynamic_profile_cb = Rc::new(Callable::Dynamic {
      name: "open_profile".to_owned(),
      inner: Box::new(OpenProfile {
        sampler: Rc::clone(&sampler),
      }),
    });

    let spine = vec![
      Vec3::new(0., 0., 0.),
      Vec3::new(0., 0., 1.),
      Vec3::new(0., 0., 2.),
    ];
    let twist = Twist::Const(0.0);
    let mesh = rail_sweep_dynamic(
      &ctx,
      &spine,
      &RingResolution::Uniform(8),
      FrameMode::Rmf,
      false,
      false, // capped
      false, // capped_explicit
      &twist,
      &dynamic_profile_cb,
      false, // fku_stitching off -> deterministic uniform stitch
      None,
      false, // adaptive off -> uniform sampling
      false,
      Vec2::new(1., 1.),
    )
    .unwrap();

    // 3 rings x (8 samples in [0,1) + appended t=1 endpoint) = 27 verts; 2 gaps x 8 quads.
    assert_eq!(mesh.vertices.len(), 27);
    assert_eq!(mesh.faces.len(), 32);
    mesh
      .check_is_manifold::<false>()
      .expect("open sheet is a 2-manifold with boundary");
    assert!(
      mesh.check_is_manifold::<true>().is_err(),
      "open sheet must report a boundary (not a closed 2-manifold)"
    );

    let ChannelStore::Vec2(uv) = &mesh.vertex_channels["uv"].store else {
      panic!("uv channel missing");
    };
    let max_v = uv.values().map(|uv| uv[1]).fold(f32::NEG_INFINITY, f32::max);
    assert!(
      (max_v - 1.0).abs() < 1e-5,
      "open profile V should reach 1.0, got {max_v}"
    );
  }

  /// An open `trace_path` profile (openness inferred from its topology, no `closed` key) swept
  /// along a *closed* spine yields an annular band: a 2-manifold with boundary.
  #[test]
  fn rail_sweep_open_profile_closed_spine_is_annulus() {
    // Distinct closed-loop spine points (no repeated first/last vertex, which would collapse the
    // last→first ring under closed=true). A fixed-up frame keeps the profile plane from twisting
    // around the loop; unlike a closed ring, an open profile can't rotate to realign at the seam.
    let src = r#"
mesh = rail_sweep(
  spine_resolution=6,
  ring_resolution=6,
  spine=[v3(3,0,0), v3(1.5,2.6,0), v3(-1.5,2.6,0), v3(-3,0,0), v3(-1.5,-2.6,0), v3(1.5,-2.6,0)],
  frame_mode=v3(0,0,1),
  closed=true,
  profile=build_path(path {
    move(0, 0)
    line(0.2, 0.3)
    line(0, 0.6)
  }),
)
"#;
    let ctx = crate::parse_and_eval_program(src).unwrap();
    let mesh_val = ctx.get_global("mesh").unwrap();
    let mesh = &mesh_val.as_mesh().unwrap().mesh;
    mesh
      .check_is_manifold::<false>()
      .expect("open profile + closed spine should be a 2-manifold with boundary");
    assert!(
      mesh.check_is_manifold::<true>().is_err(),
      "annular band must report a boundary (not a closed 2-manifold)"
    );
  }

  fn eval_rail_sweep_mesh(src: &str) -> Result<Rc<mesh::LinkedMesh<()>>, ErrorStack> {
    let ctx = crate::parse_and_eval_program(src)?;
    let mesh_val = ctx.get_global("mesh").expect("program must assign `mesh`");
    Ok(Rc::clone(&mesh_val.as_mesh().expect("`mesh` must be a mesh").mesh))
  }

  /// Two disjoint CCW squares swept along a straight spine (uncapped) build two independent open
  /// tubes in one call: one mesh, two connected components. Each subpath gets the *full*
  /// `ring_resolution` (not a split budget), so the two-square sweep has twice the faces of a
  /// single square at the same `ring_resolution`. A sequence `ring_resolution` sets per-subpath
  /// counts.
  #[test]
  fn rail_sweep_two_disjoint_squares() {
    const TWO_SQUARES: &str = r#"build_path(path {
        move(-2,-1) line(-1,-1) line(-1,1) line(-2,1) close()
        move(1,-1) line(2,-1) line(2,1) line(1,1) close()
      })"#;
    let build = |profile: &str, ring_resolution: &str| {
      let src = format!(
        r#"
mesh = rail_sweep(
  spine_resolution=3,
  ring_resolution={ring_resolution},
  spine=[v3(0,0,0), v3(0,0,1), v3(0,0,2)],
  capped=false,
  fku_stitching=false,
  adaptive_profile_sampling=false,
  profile={profile},
)
"#
      );
      eval_rail_sweep_mesh(&src).unwrap()
    };

    let one_12 =
      build(r#"build_path(path { move(1,-1) line(2,-1) line(2,1) line(1,1) close() })"#, "12");
    let one_6 =
      build(r#"build_path(path { move(1,-1) line(2,-1) line(2,1) line(1,1) close() })"#, "6");
    let two = build(TWO_SQUARES, "12");

    two
      .check_is_manifold::<false>()
      .expect("disjoint square tubes are 2-manifold with boundary");
    assert_eq!(
      two.connected_components().len(),
      2,
      "two disjoint squares form two connected components"
    );
    assert_eq!(
      two.faces.len(),
      2 * one_12.faces.len(),
      "each subpath gets the full ring_resolution, so two squares = 2x a single square at 12"
    );

    // Sequence ring_resolution: [12, 6] gives loop 0 twelve samples and loop 1 six.
    let two_seq = build(TWO_SQUARES, "[12, 6]");
    assert_eq!(two_seq.connected_components().len(), 2);
    assert_eq!(
      two_seq.faces.len(),
      one_12.faces.len() + one_6.faces.len(),
      "per-subpath counts [12, 6] should equal a 12-square plus a 6-square"
    );
  }

  /// Two disjoint squares swept CAPPED build two independent closed boxes. Each single-loop group
  /// caps through the native single-polygon path, so this runs on native builds.
  #[test]
  fn rail_sweep_capped_disjoint_squares() {
    let mesh = eval_rail_sweep_mesh(
      r#"
mesh = rail_sweep(
  spine_resolution=2,
  ring_resolution=16,
  spine=[v3(0,0,0), v3(0,0,2)],
  capped=true,
  profile=build_path(path {
    move(-2,-1) line(-1,-1) line(-1,1) line(-2,1) close()
    move(1,-1) line(2,-1) line(2,1) line(1,1) close()
  }),
)
"#,
    )
    .unwrap();
    mesh
      .check_is_manifold::<true>()
      .expect("capped disjoint squares are two closed 2-manifolds");
    assert_eq!(
      mesh.connected_components().len(),
      2,
      "two capped boxes are two closed components"
    );
  }

  /// An O-profile (outer CCW rect + inner CW rect) swept uncapped along a straight spine builds a
  /// hollow tube wall: a single connected 2-manifold with boundary (four boundary loops).
  #[test]
  fn rail_sweep_o_profile_walls() {
    let mesh = eval_rail_sweep_mesh(
      r#"
mesh = rail_sweep(
  spine_resolution=3,
  ring_resolution=32,
  spine=[v3(0,0,0), v3(0,0,1), v3(0,0,2)],
  capped=false,
  profile=build_path(path {
    move(-1,-1) line(1,-1) line(1,1) line(-1,1) close()
    move(-0.5,-0.5) line(-0.5,0.5) line(0.5,0.5) line(0.5,-0.5) close()
  }),
)
"#,
    )
    .unwrap();
    mesh
      .check_is_manifold::<false>()
      .expect("O-profile walls are 2-manifold with boundary");
    assert!(
      mesh.check_is_manifold::<true>().is_err(),
      "uncapped O-profile walls must report a boundary"
    );
    // Uncapped, the outer and inner walls are two disjoint tubes; only the (Phase 3) caps join
    // them into one genus-1 surface.
    assert_eq!(
      mesh.connected_components().len(),
      2,
      "uncapped O-profile is an outer tube + an inner tube"
    );
  }

  /// `split_seams` on a multi-loop profile splits each closed loop's seam independently: the mesh
  /// finalizes its own shading normals and opts out of the distance weld.
  #[test]
  fn rail_sweep_multi_loop_split_seams() {
    let mesh = eval_rail_sweep_mesh(
      r#"
mesh = rail_sweep(
  spine_resolution=3,
  ring_resolution=32,
  spine=[v3(0,0,0), v3(0,0,1), v3(0,0,2)],
  capped=false,
  split_seams=true,
  profile=build_path(path {
    move(-1,-1) line(1,-1) line(1,1) line(-1,1) close()
    move(-0.5,-0.5) line(-0.5,0.5) line(0.5,0.5) line(0.5,-0.5) close()
  }),
)
"#,
    )
    .unwrap();
    assert_eq!(
      mesh.shading_normals.len(),
      mesh.vertices.len(),
      "split_seams finalizes shading normals for every vertex"
    );
    assert!(
      mesh.flags & mesh_flags::NO_WELD != 0,
      "split_seams opts out of the distance weld"
    );
  }

  /// A dynamic_profile that changes subpath count partway along the spine is rejected, and the
  /// error names both offending spine indices.
  #[test]
  fn rail_sweep_topology_change_errors() {
    let err = crate::parse_and_eval_program(
      r#"
rail_sweep(
  spine_resolution=4,
  ring_resolution=16,
  spine=[v3(0,0,0), v3(0,0,1), v3(0,0,2), v3(0,0,3)],
  capped=false,
  dynamic_profile=|u: float, u_ix: int| {
    if u_ix < 2 {
      build_path(path { move(-1,-1) line(1,-1) line(1,1) line(-1,1) close() })
    } else {
      build_path(path {
        move(-1,-1) line(1,-1) line(1,1) line(-1,1) close()
        move(-0.5,-0.5) line(-0.5,0.5) line(0.5,0.5) line(0.5,-0.5) close()
      })
    }
  },
) | render
"#,
    )
    .unwrap_err()
    .to_string();
    assert!(
      err.contains("u_ix=1") && err.contains("u_ix=2"),
      "topology-change error should name both spine indices, got: {err}"
    );
  }

  /// A hole with the same winding as its outer (both CCW) violates the nesting-parity convention.
  #[test]
  fn rail_sweep_winding_parity_errors() {
    let err = crate::parse_and_eval_program(
      r#"
rail_sweep(
  spine_resolution=2,
  ring_resolution=24,
  spine=[v3(0,0,0), v3(0,0,1)],
  capped=false,
  profile=build_path(path {
    move(-1,-1) line(1,-1) line(1,1) line(-1,1) close()
    move(-0.5,-0.5) line(0.5,-0.5) line(0.5,0.5) line(-0.5,0.5) close()
  }),
) | render
"#,
    )
    .unwrap_err()
    .to_string();
    assert!(
      err.contains("winding") && err.contains("nesting depth"),
      "same-winding nested loops should raise a winding/parity error, got: {err}"
    );
  }

  /// A non-uniform sequence spine: by default the points are used as-is, but explicit
  /// "uniform" still resamples by arc length (legacy escape hatch).
  #[test]
  fn integration_rail_sweep_sequence_spine_distribution() {
    let render = |scheme: &str| -> Vec<f32> {
      let src = format!(
        r#"
rail_sweep(
  spine_resolution=3,
  ring_resolution=4,
  spine=[v3(0,0,0), v3(0,0,0.1), v3(0,0,1)],
  {scheme}
  profile=|u: float, v: float| v2(cos(v * tau) * 0.1, sin(v * tau) * 0.1),
  capped=false,
)
  | render
"#
      );
      let rendered = crate::parse_and_eval_program(&src)
        .unwrap()
        .rendered_meshes
        .into_inner();
      let mesh = &rendered[0].mesh;
      let mut zs: Vec<f32> = mesh
        .mesh
        .vertices
        .values()
        .map(|v| (mesh.transform * v.position.push(1.)).z)
        .collect();
      zs.sort_by(|a, b| a.partial_cmp(b).unwrap());
      zs
    };

    // Default: middle ring sits at z=0.1 (the input point).
    let zs = render("");
    assert!(
      (zs[5] - 0.1).abs() < 1e-4,
      "default should preserve points: {zs:?}"
    );

    // Explicit "uniform": middle ring is at ~z=0.5 (arc-length midpoint).
    let zs = render(r#"spine_sampling_scheme="uniform","#);
    assert!(
      (zs[5] - 0.5).abs() < 0.05,
      "uniform should resample: {zs:?}"
    );
  }
}

/// Watertight holed caps depend on CGAL's constrained Delaunay (wasm-only), so these run under
/// `wasm-bindgen-test` / Geotoy rather than native `cargo test`.
#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
  /// A capped O-profile (outer + hole) is a closed genus-1 2-manifold: watertight, single
  /// connected component, and Euler characteristic V − E + F = 0.
  #[test]
  fn capped_o_profile_is_genus_1() {
    let ctx = crate::parse_and_eval_program(
      r#"
mesh = rail_sweep(
  spine_resolution=3,
  ring_resolution=32,
  spine=[v3(0,0,0), v3(0,0,1), v3(0,0,2)],
  capped=true,
  profile=build_path(path {
    move(-1,-1) line(1,-1) line(1,1) line(-1,1) close()
    move(-0.5,-0.5) line(-0.5,0.5) line(0.5,0.5) line(0.5,-0.5) close()
  }),
)
"#,
    )
    .unwrap();
    let mesh_val = ctx.get_global("mesh").unwrap();
    let mesh = &mesh_val.as_mesh().unwrap().mesh;
    mesh
      .check_is_manifold::<true>()
      .expect("capped O-profile is a closed 2-manifold");
    assert_eq!(
      mesh.connected_components().len(),
      1,
      "capped hollow tube is one connected component"
    );
    let v = mesh.vertices.len() as i64;
    let e = mesh.edges.len() as i64;
    let f = mesh.faces.len() as i64;
    assert_eq!(v - e + f, 0, "capped hollow tube has Euler characteristic 0 (genus 1)");
  }
}
