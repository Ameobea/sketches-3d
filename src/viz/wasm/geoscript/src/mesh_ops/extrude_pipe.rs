use std::f32::consts::PI;

use mesh::{linked_mesh::Vec3, LinkedMesh};

use crate::ErrorStack;

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

  // Tangents are the direction of the path at each point.  There's some special handling for
  // the first and last points.
  let mut tangents: Vec<Vec3> = Vec::with_capacity(points.len());
  for i in 0..points.len() {
    let dir = if i == points.len() - 1 {
      points[i] - points[i - 1]
    } else {
      points[i + 1] - points[i]
    };
    tangents.push(dir.normalize());
  }

  // Rotation-minimizing frames are used to avoid twists or kinks in the mesh as it's
  // generated along the path.
  //
  // Some more details about that can be found here:
  // https://faculty.engineering.ucdavis.edu/farouki/wp-content/uploads/sites/51/2021/07/Rational-rotation-minimizing-frames.pdf
  // (wayback link: https://web.archive.org/web/20240819234624/https://faculty.engineering.ucdavis.edu/farouki/wp-content/uploads/sites/51/2021/07/Rational-rotation-minimizing-frames.pdf)
  //
  // Rather than using a fixed up vector, the normal is projected forward using the new
  // tangent to minimize its rotation from the previous ring.

  // an initial normal is picked using an arbitrary up vector.
  let t0 = tangents[0];
  let mut up = Vec3::new(0., 1., 0.);
  // if the chosen up vector is nearly parallel to the tangent, a different one is picked to
  // avoid numerical issues
  if t0.dot(&up).abs() > 0.999 {
    up = Vec3::new(1., 0., 0.);
  }
  let mut normal = t0.cross(&up).normalize();
  // the "binormal" is a vector that's perpendicular to the plane defined by the tangent and
  // normal.
  let mut binormal = t0.cross(&normal).normalize();

  let extra_cap_vtx_count = match end_mode {
    EndMode::Close => 0,
    // one vtx as added at the center of each end to serve as the center of the triangle fan
    EndMode::Connect => 2,
    EndMode::Open => 0,
  };
  let mut verts: Vec<Vec3> = Vec::with_capacity(points.len() * resolution + extra_cap_vtx_count);

  let center0 = points[0];
  let radii = get_radius(0, center0)?;
  radii.validate(resolution, 0)?;
  for j in 0..resolution {
    let theta = 2. * PI * (j as f32) / (resolution as f32) + twist(0, center0)?;
    let dir = normal * theta.cos() + binormal * theta.sin();
    verts.push(center0 + dir * radii.get(j));
  }

  for i in 1..points.len() {
    let ti = tangents[i];
    // Project previous normal onto plane ⟂ tangentᵢ
    let dot = ti.dot(&normal);
    let mut proj = normal - ti * dot;
    const EPSILON: f32 = 1e-6;
    if proj.norm_squared() < EPSILON {
      // the same check as before is done to avoid numerical issues if the projected normal is
      // very close to 0
      proj = ti.cross(&binormal);
      if proj.norm_squared() < EPSILON {
        // In the extremely degenerate case, pick any vector ⟂ tangentᵢ
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

    let center = points[i];
    let radii = get_radius(i, center)?;
    radii.validate(resolution, i)?;
    for j in 0..resolution {
      let theta = 2. * PI * (j as f32) / (resolution as f32) + twist(i, center)?;
      let dir = normal * theta.cos() + binormal * theta.sin();
      verts.push(center + dir * radii.get(j));
    }
  }

  assert_eq!(verts.len(), points.len() * resolution);

  // stitch the rings together with quads, two triangles per quad
  let mut index_count = (points.len() - 1) * resolution * 3 * 2;
  match end_mode {
    EndMode::Close => {
      // `n` triangles are needed to tessellate a convex polygon of `n` vertices/edges by filling
      // to a center vertex.
      let cap_triangles = resolution;
      index_count += cap_triangles * 3 * 2;
    }
    EndMode::Connect => {
      // Add one more ring connection (last to first)
      index_count += resolution * 3 * 2;
    }
    EndMode::Open => {}
  }
  let mut indices: Vec<u32> = Vec::with_capacity(index_count);

  for i in 0..(points.len() - 1) {
    for j in 0..resolution {
      let a = (i * resolution + j) as u32;
      let b = (i * resolution + (j + 1) % resolution) as u32;
      let c = ((i + 1) * resolution + j) as u32;
      let d = ((i + 1) * resolution + (j + 1) % resolution) as u32;

      indices.push(a);
      indices.push(b);
      indices.push(c);

      indices.push(b);
      indices.push(d);
      indices.push(c);
    }
  }

  match end_mode {
    EndMode::Close => {
      for (ix_offset, reverse_winding) in [
        (0u32, true),
        ((points.len() - 1) as u32 * resolution as u32, false),
      ] {
        // using a basic triangle fan out from an added center vtx to form the end caps
        let center_vtx_ix = verts.len();
        let center = verts[(ix_offset as usize)..(ix_offset as usize + resolution)]
          .iter()
          .fold(Vec3::new(0., 0., 0.), |acc, v| acc + *v)
          / (resolution as f32);
        verts.push(center);

        for vtx_ix in 0..resolution {
          let a = center_vtx_ix as u32;
          let b = ix_offset + (vtx_ix as u32);
          let c = ix_offset + (((vtx_ix + 1) % resolution) as u32);

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
    EndMode::Connect => {
      // Connect last ring to first ring
      let last = points.len() - 1;
      for j in 0..resolution {
        let a = (last * resolution + j) as u32;
        let b = (last * resolution + (j + 1) % resolution) as u32;
        let c = (0 * resolution + j) as u32;
        let d = (0 * resolution + (j + 1) % resolution) as u32;

        indices.push(a);
        indices.push(b);
        indices.push(c);

        indices.push(b);
        indices.push(d);
        indices.push(c);
      }
    }
    EndMode::Open => {}
  }

  assert_eq!(indices.len(), index_count);

  Ok(LinkedMesh::from_indexed_vertices(
    &verts, &indices, None, None,
  ))
}
