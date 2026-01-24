use std::f32::consts::PI;

use mesh::{linked_mesh::Vec3, LinkedMesh};

use crate::mesh_ops::adaptive_sampler::adaptive_sample;
use crate::mesh_ops::rail_sweep::{rail_sweep, FrameMode};
use crate::{ErrorStack, Vec2};

pub enum EndMode {
  Open,
  Close,
  Connect,
}

pub enum PipeRadius {
  /// All points in the ring are equidistant from the center, approximating a circle.
  Constant(f32),
  /// The distance of each point in the ring from the center is defined explicitly ahead of time
  Explicit(Vec<f32>),
}

impl PipeRadius {
  pub fn constant(radius: f32) -> Self {
    PipeRadius::Constant(radius)
  }

  fn validate(&self, resolution: usize, ring_ix: usize) -> Result<(), ErrorStack> {
    match self {
      Self::Constant(_) => Ok(()),
      Self::Explicit(radii) => {
        if radii.len() != resolution {
          return Err(ErrorStack::new(format!(
            "Invalid radius count returned from user-provided callback for ring index={ring_ix}; \
             expected {resolution} radii, found {}",
            radii.len()
          )));
        }

        Ok(())
      }
    }
  }

  fn get(&self, index: usize) -> f32 {
    match self {
      Self::Constant(radius) => *radius,
      Self::Explicit(radii) => radii.get(index).copied().unwrap_or_else(|| {
        panic!(
          "We should have already validated user-defined radii seq len.  Tried to get index \
           {index} with len={}",
          radii.len()
        )
      }),
    }
  }
}

pub fn extrude_pipe(
  get_radius: impl Fn(usize, Vec3) -> Result<PipeRadius, ErrorStack>,
  resolution: usize,
  path: impl Iterator<Item = Result<Vec3, ErrorStack>>,
  end_mode: EndMode,
  twist: impl Fn(usize, Vec3) -> Result<f32, ErrorStack>,
  adaptive_path_sampling: bool,
) -> Result<LinkedMesh<()>, ErrorStack> {
  if resolution < 3 {
    return Err(ErrorStack::new(
      "`extrude_pipe` requires a resolution of at least 3",
    ));
  }

  let raw_points = path.collect::<Result<Vec<_>, _>>()?;

  if raw_points.len() < 2 {
    return Err(ErrorStack::new(format!(
      "`extrude_pipe` requires at least two points in the path, found: {}",
      raw_points.len()
    )));
  }

  // Optionally resample the path adaptively based on curvature
  let points = if adaptive_path_sampling && raw_points.len() >= 3 {
    // Use original point t-values as critical points to preserve them
    let original_ts: Vec<f32> = (0..raw_points.len())
      .map(|i| i as f32 / (raw_points.len() - 1) as f32)
      .collect();

    let sample_polyline = |t: f32| -> Vec3 {
      if t <= 0. {
        return raw_points[0];
      }
      if t >= 1. {
        return raw_points[raw_points.len() - 1];
      }
      let scaled = t * (raw_points.len() - 1) as f32;
      let seg_ix = scaled.floor() as usize;
      let local_t = scaled - seg_ix as f32;
      if seg_ix + 1 >= raw_points.len() {
        raw_points[raw_points.len() - 1]
      } else {
        raw_points[seg_ix].lerp(&raw_points[seg_ix + 1], local_t)
      }
    };

    let adaptive_ts =
      adaptive_sample::<Vec3, _>(raw_points.len(), &original_ts, sample_polyline, 1e-5);
    adaptive_ts.iter().map(|&t| sample_polyline(t)).collect()
  } else {
    raw_points
  };

  let mut ring_radii = Vec::with_capacity(points.len());
  for (i, center) in points.iter().enumerate() {
    let radii = get_radius(i, *center)?;
    radii.validate(resolution, i)?;
    ring_radii.push(radii);
  }

  let (closed, capped) = match end_mode {
    EndMode::Open => (false, false),
    EndMode::Close => (false, true),
    EndMode::Connect => (true, false),
  };

  rail_sweep(
    &points,
    resolution,
    FrameMode::Rmf,
    closed,
    capped,
    twist,
    |_, _v_norm, u_ix, v_ix, _| {
      let theta = 2. * PI * (v_ix as f32) / (resolution as f32);
      let radius = ring_radii[u_ix].get(v_ix);
      Ok(Vec2::new(theta.cos() * radius, theta.sin() * radius))
    },
    None,
    None,
    None,
  )
}
