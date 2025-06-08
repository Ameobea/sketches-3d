use std::f32::consts::PI;

use mesh::{linked_mesh::Vec3, LinkedMesh};

use crate::ErrorStack;

pub mod mesh_boolean;
pub mod mesh_ops;

pub fn extrude_pipe(
  get_radius: impl Fn(usize, Vec3) -> Result<f32, ErrorStack>,
  resolution: usize,
  path: impl Iterator<Item = Result<Vec3, ErrorStack>>,
  close_ends: bool,
) -> Result<LinkedMesh<()>, ErrorStack> {
  if resolution < 3 {
    return Err(ErrorStack::new(
      "`extrude_pipe` requires a resolution of at least 3",
    ));
  }

  // TODO: shouldn't need to collect here
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

  let mut verts: Vec<Vec3> = Vec::with_capacity(points.len() * resolution);

  let center0 = points[0];
  for j in 0..resolution {
    let theta = 2. * PI * (j as f32) / (resolution as f32);
    let dir = normal * theta.cos() + binormal * theta.sin();
    verts.push(center0 + dir * get_radius(0, center0)?);
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
    for j in 0..resolution {
      let theta = 2. * PI * (j as f32) / (resolution as f32);
      let dir = normal * theta.cos() + binormal * theta.sin();
      verts.push(center + dir * get_radius(i, center)?);
    }
  }

  assert_eq!(verts.len(), points.len() * resolution);

  // stitch the rings together with quads, two triangles per quad
  let mut index_count = (points.len() - 1) * resolution * 3 * 2;
  if close_ends {
    // `n-2` triangles are needed to tessellate a convex polygon of `n` vertices/edges
    let cap_triangles = resolution - 2;
    index_count += cap_triangles * 3 * 2;
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

  if close_ends {
    for (ix_offset, reverse_winding) in [
      (0u32, true),
      ((points.len() - 1) as u32 * resolution as u32, false),
    ] {
      // using a basic triangle fan to form the end caps
      //
      // 0,1,2
      // 0,2,3
      // ...
      // 0,2n-2,2n-1
      for vtx_ix in 1..(resolution - 1) {
        let a = 0;
        let b = vtx_ix as u32;
        let c = (vtx_ix + 1) as u32;

        if reverse_winding {
          indices.push(ix_offset + c);
          indices.push(ix_offset + b);
          indices.push(ix_offset + a);
        } else {
          indices.push(ix_offset + a);
          indices.push(ix_offset + b);
          indices.push(ix_offset + c);
        }
      }
    }
  }

  // TODO: support mode that creates a closed loop

  assert_eq!(indices.len(), index_count);

  Ok(LinkedMesh::from_indexed_vertices(
    &verts, &indices, None, None,
  ))
}
