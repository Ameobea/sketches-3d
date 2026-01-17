use std::{cell::RefCell, rc::Rc};

use fxhash::FxHashMap;
use mesh::{linked_mesh::Vec3, LinkedMesh};
use nalgebra::Matrix4;

use crate::{ArgRef, ErrorStack, EvalCtx, ManifoldHandle, MeshHandle, Sym, Value, EMPTY_KWARGS};

const COLLAPSE_EPSILON: f32 = 1e-5;

fn row_is_collapsed(row: &[Vec3]) -> bool {
  if row.is_empty() {
    return true;
  }
  let first = row[0];
  let epsilon_sq = COLLAPSE_EPSILON * COLLAPSE_EPSILON;
  row
    .iter()
    .all(|v| (*v - first).norm_squared() <= epsilon_sq)
}

fn compute_row_centroid(row: &[Vec3]) -> Vec3 {
  if row.is_empty() {
    return Vec3::new(0., 0., 0.);
  }
  row.iter().fold(Vec3::new(0., 0., 0.), |acc, v| acc + *v) / (row.len() as f32)
}

struct RowInfo {
  start_ix: usize,
  count: usize,
}

#[inline]
fn push_tri<const FLIP: bool>(indices: &mut Vec<u32>, v0: u32, v1: u32, v2: u32) {
  if FLIP {
    indices.push(v0);
    indices.push(v2);
    indices.push(v1);
  } else {
    indices.push(v0);
    indices.push(v1);
    indices.push(v2);
  }
}

fn stitch_rows<const FLIP: bool>(
  indices: &mut Vec<u32>,
  row_a: &RowInfo,
  row_b: &RowInfo,
  v_points: usize,
  v_closed: bool,
) {
  let wrap_count = if v_closed {
    v_points
  } else {
    v_points.saturating_sub(1)
  };

  match (row_a.count, row_b.count) {
    // both rows collapsed to single points; no faces to create
    (1, 1) => {}
    // row A is collapsed; build triangle fan to all vertices in row B
    (1, _) => {
      let apex = row_a.start_ix as u32;
      for j in 0..wrap_count {
        let b = (row_b.start_ix + j) as u32;
        let c = (row_b.start_ix + (j + 1) % row_b.count) as u32;
        push_tri::<FLIP>(indices, apex, b, c);
      }
    }
    // row B is collapsed; build triangle fan to all vertices in row A
    (_, 1) => {
      let apex = row_b.start_ix as u32;
      for j in 0..wrap_count {
        let a = (row_a.start_ix + j) as u32;
        let b = (row_a.start_ix + (j + 1) % row_a.count) as u32;
        push_tri::<FLIP>(indices, a, apex, b);
      }
    }
    // normal case; stitch together with quads
    _ => {
      for j in 0..wrap_count {
        let j_next = (j + 1) % row_a.count;

        let a = (row_a.start_ix + j) as u32;
        let b = (row_a.start_ix + j_next) as u32;
        let c = (row_b.start_ix + j) as u32;
        let d = (row_b.start_ix + j_next) as u32;

        push_tri::<FLIP>(indices, a, c, b);
        push_tri::<FLIP>(indices, b, c, d);
      }
    }
  }
}

/// Allows creating any genus 0 or 1 surface (generalized spheres or tori) by mapping a 2D plane
/// parameterized by (`u`, `v`) each in [0, 1] to 3D positions.  This is the most fundamental
/// 3D surface generation function Geoscript has; theoretically any other could be implemented
/// in terms of this.
///
/// u_closed=false + v_closed=true -> topological sphere
/// u_closed=true + v_closed=true -> topological torus
/// u_closed=false + v_closed=false -> topological plane
/// u_closd=true + v_closed=false -> topological sphere
pub fn parametric_surface(
  u_res: usize,
  v_res: usize,
  u_closed: bool,
  v_closed: bool,
  flip_normals: bool,
  generator: impl Fn(f32, f32) -> Result<Vec3, ErrorStack>,
) -> Result<LinkedMesh<()>, ErrorStack> {
  if u_res < 1 {
    return Err(ErrorStack::new(format!(
      "`parametric_surface` requires u_res >= 1, found: {u_res}"
    )));
  }
  if v_res < 1 {
    return Err(ErrorStack::new(format!(
      "`parametric_surface` requires v_res >= 1, found: {v_res}"
    )));
  }

  let u_points = if u_closed { u_res } else { u_res + 1 };
  let v_points = if v_closed { v_res } else { v_res + 1 };

  // TODO: would be good to switch this to a flat array of `nalgebra` matrix
  let mut raw_rows: Vec<Vec<Vec3>> = Vec::with_capacity(u_points);

  for i in 0..u_points {
    let u = i as f32 / u_res as f32;
    let mut row = Vec::with_capacity(v_points);
    for j in 0..v_points {
      let v = j as f32 / v_res as f32;
      let pos = generator(u, v)?;
      row.push(pos);
    }
    raw_rows.push(row);
  }

  let mut verts: Vec<Vec3> = Vec::with_capacity(u_points * v_points);
  let mut row_infos: Vec<RowInfo> = Vec::with_capacity(u_points);

  for (u_ix, row) in raw_rows.iter().enumerate() {
    let is_boundary = u_ix == 0 || u_ix == u_points - 1;

    let should_collapse = is_boundary && v_closed && row_is_collapsed(row);
    let start_ix = verts.len();
    if should_collapse {
      verts.push(compute_row_centroid(row));
      row_infos.push(RowInfo { start_ix, count: 1 });
    } else {
      verts.extend(row.iter().copied());
      row_infos.push(RowInfo {
        start_ix,
        count: v_points,
      });
    }
  }

  let mut indices: Vec<u32> = Vec::with_capacity(u_res * v_res * 6);

  for i in 0..u_res {
    let i_next = if u_closed && i == u_res - 1 { 0 } else { i + 1 };
    let stitch_impl = if flip_normals { stitch_rows::<true> } else { stitch_rows::<false> };
    stitch_impl(
      &mut indices,
      &row_infos[i],
      &row_infos[i_next],
      v_points,
      v_closed,
    );
  }

  Ok(LinkedMesh::from_indexed_vertices(
    &verts, &indices, None, None,
  ))
}

pub(crate) fn parametric_surface_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let u_res = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
      let v_res = arg_refs[1].resolve(args, kwargs).as_int().unwrap();
      let u_closed = arg_refs[2].resolve(args, kwargs).as_bool().unwrap();
      let v_closed = arg_refs[3].resolve(args, kwargs).as_bool().unwrap();
      let flip_normals = arg_refs[4].resolve(args, kwargs).as_bool().unwrap();
      let generator = arg_refs[5].resolve(args, kwargs).as_callable().unwrap();

      if u_res < 1 {
        return Err(ErrorStack::new(format!(
          "Invalid u_res for `parametric_surface`; expected >= 1, found: {u_res}"
        )));
      }
      if v_res < 1 {
        return Err(ErrorStack::new(format!(
          "Invalid v_res for `parametric_surface`; expected >= 1, found: {v_res}"
        )));
      }

      let mesh = parametric_surface(u_res as usize, v_res as usize, u_closed, v_closed, flip_normals, |u, v| {
        let out = ctx
          .invoke_callable(generator, &[Value::Float(u), Value::Float(v)], EMPTY_KWARGS)
          .map_err(|err| err.wrap("Error produced by user-supplied `generator` callable in `parametric_surface`"))?;
        out.as_vec3().copied().ok_or_else(|| {
          ErrorStack::new(format!(
            "Expected Vec3 from generator function in `parametric_surface`, found: {out:?}"
          ))
        })
      })?;

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
  use super::*;
  use crate::parse_and_eval_program;
  use std::f32::consts::PI;

  #[test]
  fn test_parametric_surface_simple_plane() {
    // Simple plane: z = 0, x and y from 0 to 1
    let mesh =
      parametric_surface(2, 2, false, false, false, |u, v| Ok(Vec3::new(u, v, 0.0))).unwrap();

    // 3x3 grid of vertices
    assert_eq!(mesh.vertices.len(), 9);
    // 2x2 quads = 4 quads = 8 triangles
    assert_eq!(mesh.faces.len(), 8);
  }

  #[test]
  fn test_parametric_surface_cylinder() {
    // Cylinder: u is height, v is angle (closed)
    // flip_normals=true for outward-facing normals with this parameterization
    let mesh = parametric_surface(2, 8, false, true, true, |u, v| {
      let angle = v * 2.0 * PI;
      Ok(Vec3::new(angle.cos(), u, angle.sin()))
    })
    .unwrap();

    // 3 rows of 8 vertices each
    assert_eq!(mesh.vertices.len(), 24);
    // 2 rings of 8 quads each = 16 quads = 32 triangles
    assert_eq!(mesh.faces.len(), 32);
  }

  #[test]
  fn test_parametric_surface_sphere_with_poles() {
    // Sphere: u is latitude (0=south pole, 1=north pole), v is longitude (closed)
    let mesh = parametric_surface(4, 8, false, true, false, |u, v| {
      let phi = u * PI; // 0 to PI (south to north)
      let theta = v * 2.0 * PI; // 0 to 2PI (around)
      Ok(Vec3::new(
        phi.sin() * theta.cos(),
        phi.cos(),
        phi.sin() * theta.sin(),
      ))
    })
    .unwrap();

    // At u=0 (south pole) and u=1 (north pole), all v vertices collapse
    // So we should have: 1 (south) + 3*8 (middle rows) + 1 (north) = 26 vertices
    assert_eq!(mesh.vertices.len(), 26);

    // Triangles:
    // - South pole to row 1: 8 triangles (fan)
    // - Row 1 to row 2: 8 quads = 16 triangles
    // - Row 2 to row 3: 8 quads = 16 triangles
    // - Row 3 to north pole: 8 triangles (fan)
    // Total: 8 + 16 + 16 + 8 = 48 triangles
    assert_eq!(mesh.faces.len(), 48);
  }

  #[test]
  fn test_parametric_surface_torus() {
    // Torus: both u and v are closed
    // flip_normals=true for outward-facing normals with this parameterization
    let major_r = 2.0;
    let minor_r = 0.5;
    let mesh = parametric_surface(8, 8, true, true, true, |u, v| {
      let theta = u * 2.0 * PI; // around the major circle
      let phi = v * 2.0 * PI; // around the minor circle
      let r = major_r + minor_r * phi.cos();
      Ok(Vec3::new(
        r * theta.cos(),
        minor_r * phi.sin(),
        r * theta.sin(),
      ))
    })
    .unwrap();

    // 8x8 grid, both closed = 8*8 = 64 vertices
    assert_eq!(mesh.vertices.len(), 64);
    // 8x8 quads = 64 quads = 128 triangles
    assert_eq!(mesh.faces.len(), 128);
  }

  #[test]
  fn test_parametric_surface_error_on_zero_resolution() {
    let result = parametric_surface(0, 4, false, false, false, |u, v| Ok(Vec3::new(u, v, 0.0)));
    assert!(result.is_err());

    let result = parametric_surface(4, 0, false, false, false, |u, v| Ok(Vec3::new(u, v, 0.0)));
    assert!(result.is_err());
  }

  #[test]
  fn test_row_is_collapsed() {
    let collapsed = vec![
      Vec3::new(0.0, 0.0, 0.0),
      Vec3::new(1e-6, 0.0, 0.0),
      Vec3::new(0.0, 1e-6, 0.0),
    ];
    assert!(row_is_collapsed(&collapsed));

    let not_collapsed = vec![
      Vec3::new(0.0, 0.0, 0.0),
      Vec3::new(1.0, 0.0, 0.0),
      Vec3::new(0.0, 1.0, 0.0),
    ];
    assert!(!row_is_collapsed(&not_collapsed));
  }

  #[test]
  fn test_parametric_surface_integration_basic() {
    // Test that the builtin is properly wired up through the eval system
    let src = r#"
mesh = parametric_surface(
  u_res=4,
  v_res=4,
  generator=|u, v| v3(u, v, 0)
)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let mesh_val = ctx.get_global("mesh").unwrap();
    assert!(mesh_val.as_mesh().is_some());
  }

  #[test]
  fn test_parametric_surface_integration_sphere() {
    // Test a sphere with pole welding through the eval system
    let src = r#"
mesh = parametric_surface(
  u_res=4,
  v_res=8,
  v_closed=true,
  generator=|u, v| {
    phi = u * pi
    theta = v * 2 * pi
    v3(sin(phi) * cos(theta), cos(phi), sin(phi) * sin(theta))
  }
)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let mesh_val = ctx.get_global("mesh").unwrap();
    let mesh_handle = mesh_val.as_mesh().unwrap();
    // Should have welded poles: 1 + 3*8 + 1 = 26 vertices
    assert_eq!(mesh_handle.mesh.vertices.len(), 26);
  }

  #[test]
  fn test_parametric_surface_integration_torus() {
    // torus
    let src = r#"
major_r = 2
minor_r = 0.5
mesh = parametric_surface(
  u_res=8,
  v_res=8,
  u_closed=true,
  v_closed=true,
  flip_normals=true,
  generator=|u, v| {
    theta = u * 2 * pi
    phi = v * 2 * pi
    r = major_r + minor_r * cos(phi)
    v3(r * cos(theta), minor_r * sin(phi), r * sin(theta))
  }
)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let mesh_val = ctx.get_global("mesh").unwrap();
    let mesh_handle = mesh_val.as_mesh().unwrap();
    // 8x8 = 64 vertices (no collapse for torus)
    assert_eq!(mesh_handle.mesh.vertices.len(), 64);
    // 64 quads = 128 triangles
    assert_eq!(mesh_handle.mesh.faces.len(), 128);
  }
}
