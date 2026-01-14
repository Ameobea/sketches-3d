use mesh::{linked_mesh::Vec3, LinkedMesh};

use crate::{ErrorStack, Vec2};

const FRAME_EPSILON: f32 = 1e-6;
const COLLAPSE_EPSILON: f32 = 1e-5;

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
  ring
    .iter()
    .fold(Vec3::new(0., 0., 0.), |acc, v| acc + *v)
    / (ring.len() as f32)
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
  let mut verts: Vec<Vec3> = Vec::with_capacity(spine_points.len() * ring_resolution + 2);
  let mut ring_infos: Vec<RingInfo> = Vec::with_capacity(spine_points.len());

  let u_denom = (frames.len() - 1) as f32;
  let v_denom = ring_resolution as f32;

  for (u_ix, frame) in frames.iter().enumerate() {
    let u_norm = if u_denom > 0.0 {
      u_ix as f32 / u_denom
    } else {
      0.0
    };
    let twist_angle = twist(u_ix, frame.center)?;
    let (normal, binormal) = apply_twist(frame.normal, frame.binormal, twist_angle);

    let mut ring = Vec::with_capacity(ring_resolution);
    for v_ix in 0..ring_resolution {
      let v_norm = v_ix as f32 / v_denom;
      let offset = profile(u_norm, v_norm, u_ix, v_ix, frame.center)?;
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

  let mut indices: Vec<u32> =
    Vec::with_capacity(spine_points.len() * ring_resolution * 6);

  for i in 0..(ring_infos.len() - 1) {
    stitch_rings(&mut indices, &ring_infos[i], &ring_infos[i + 1], ring_resolution);
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

      let center = ring_center(
        &verts[ring_info.start..(ring_info.start + ring_resolution)],
      );
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

#[cfg(test)]
mod tests {
  use super::{rail_sweep, FrameMode};
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
    )
    .unwrap();

    assert_eq!(mesh.vertices.len(), ring_resolution + 2);
    assert_eq!(mesh.faces.len(), ring_resolution * 2);
  }
}
