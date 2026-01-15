use std::f32::consts::PI;

use mesh::{linked_mesh::Vec3, LinkedMesh};

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
) -> Result<LinkedMesh<()>, ErrorStack> {
  if resolution < 3 {
    return Err(ErrorStack::new(
      "`extrude_pipe` requires a resolution of at least 3",
    ));
  }

  let points = path.collect::<Result<Vec<_>, _>>()?;

  if points.len() < 2 {
    return Err(ErrorStack::new(format!(
      "`extrude_pipe` requires at least two points in the path, found: {}",
      points.len()
    )));
  }

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
  )
}
