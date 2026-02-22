use std::{cell::RefCell, rc::Rc};

use fxhash::FxHashMap;
use mesh::{linked_mesh::Vec3, LinkedMesh};
use nalgebra::Matrix4;

use crate::{ArgRef, ErrorStack, EvalCtx, ManifoldHandle, MeshHandle, Sym, Value, EMPTY_KWARGS};

use super::adaptive_sampler::adaptive_sample_fallible;
use super::fku_stitch::{
  dp_stitch_presampled, should_use_fku, stitch_apex_to_row, uniform_stitch_rows,
};
use super::helpers::{compute_centroid, vertices_are_collapsed};

struct RowInfo {
  start_ix: usize,
  count: usize,
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
///
/// TODO: should probably collapse these arguments down to a params struct
pub fn parametric_surface(
  u_res: usize,
  v_res: usize,
  u_closed: bool,
  v_closed: bool,
  flip_normals: bool,
  fku_stitching: bool,
  adaptive_u_sampling: bool,
  adaptive_v_sampling: bool,
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

  // Compute u sample values (either uniform or adaptive)
  let u_samples: Vec<f32> = if adaptive_u_sampling && u_points >= 3 {
    // For adaptive u sampling, we sample the generator along a representative v curve
    // (using v=0.5 as a middle sample) to detect curvature in the u direction
    let v_sample = 0.5;
    let initial_ts = &[0.0, 1.0];

    adaptive_sample_fallible::<Vec3, ErrorStack>(
      u_points,
      initial_ts,
      |u| generator(u, v_sample),
      1e-5,
    )?
  } else {
    // Uniform u sampling
    (0..u_points).map(|i| i as f32 / u_res as f32).collect()
  };

  // Generate rows, potentially with adaptive v sampling per row
  let mut raw_rows: Vec<Vec<Vec3>> = Vec::with_capacity(u_points);
  let mut row_v_counts: Vec<usize> = Vec::with_capacity(u_points);

  for &u in &u_samples {
    if adaptive_v_sampling && v_points >= 3 {
      // Adaptive v sampling for this row
      let initial_ts: Vec<f32> = vec![0.0, 1.0];

      let v_samples = adaptive_sample_fallible::<Vec3, ErrorStack>(
        v_points,
        &initial_ts,
        |v| generator(u, v),
        1e-5,
      )?;

      let mut row = Vec::with_capacity(v_samples.len());
      for &v in &v_samples {
        row.push(generator(u, v)?);
      }
      row_v_counts.push(row.len());
      raw_rows.push(row);
    } else {
      // Uniform v sampling
      let mut row = Vec::with_capacity(v_points);
      for j in 0..v_points {
        let v = j as f32 / v_res as f32;
        row.push(generator(u, v)?);
      }
      row_v_counts.push(row.len());
      raw_rows.push(row);
    }
  }

  let mut verts: Vec<Vec3> = Vec::with_capacity(u_points * v_points);
  let mut row_infos: Vec<RowInfo> = Vec::with_capacity(u_points);

  for (u_ix, row) in raw_rows.iter().enumerate() {
    let is_boundary = u_ix == 0 || u_ix == u_points - 1;

    let should_collapse = is_boundary && v_closed && vertices_are_collapsed(row);
    let start_ix = verts.len();
    if should_collapse {
      verts.push(compute_centroid(row));
      row_infos.push(RowInfo { start_ix, count: 1 });
    } else {
      verts.extend(row.iter().copied());
      row_infos.push(RowInfo {
        start_ix,
        count: row.len(),
      });
    }
  }

  let mut indices: Vec<u32> = Vec::with_capacity(u_res * v_res * 6);

  // When using adaptive v sampling, rows may have different vertex counts,
  // so we need to check FKU compatibility per row pair
  let base_use_fku = should_use_fku(fku_stitching, v_points, v_points);

  for i in 0..u_res {
    let i_next = if u_closed && i == u_res - 1 { 0 } else { i + 1 };
    let row_a = &row_infos[i];
    let row_b = &row_infos[i_next];

    match (row_a.count, row_b.count) {
      // Both rows collapsed to single points; no faces to create
      (1, 1) => {}
      // Row A is collapsed (apex); build triangle fan
      (1, _) => {
        stitch_apex_to_row(
          row_a.start_ix,
          row_b.start_ix,
          row_b.count,
          v_closed,
          true,
          flip_normals,
          &mut indices,
        );
      }
      // Row B is collapsed (apex); build triangle fan
      (_, 1) => {
        stitch_apex_to_row(
          row_b.start_ix,
          row_a.start_ix,
          row_a.count,
          v_closed,
          false,
          flip_normals,
          &mut indices,
        );
      }
      // Normal case: both rows have full vertex count
      _ => {
        // When rows have different vertex counts (adaptive v sampling), always use FKU
        let use_fku = base_use_fku || row_a.count != row_b.count;
        if use_fku {
          // Use FKU DP-based stitching for optimal triangulation
          // This minimizes edge lengths, avoiding sharp angles when vertices drift between rows
          let pts_a: Vec<Vec3> = (row_a.start_ix..row_a.start_ix + row_a.count)
            .map(|idx| verts[idx])
            .collect();
          let pts_b: Vec<Vec3> = (row_b.start_ix..row_b.start_ix + row_b.count)
            .map(|idx| verts[idx])
            .collect();

          // Handle flip_normals by swapping row order
          if flip_normals {
            dp_stitch_presampled(
              &pts_b,
              &pts_a,
              None,
              None,
              None,
              None,
              row_b.start_ix,
              row_a.start_ix,
              v_closed,
              &mut indices,
            );
          } else {
            dp_stitch_presampled(
              &pts_a,
              &pts_b,
              None,
              None,
              None,
              None,
              row_a.start_ix,
              row_b.start_ix,
              v_closed,
              &mut indices,
            );
          }
        } else {
          // Use simple uniform quad-based stitching for predictable topology
          uniform_stitch_rows(
            row_a.start_ix,
            row_b.start_ix,
            row_a.count,
            v_closed,
            flip_normals,
            &mut indices,
          );
        }
      }
    }
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
      let fku_stitching = arg_refs[6].resolve(args, kwargs).as_bool().unwrap();
      let adaptive_u_sampling = arg_refs[7].resolve(args, kwargs).as_bool().unwrap();
      let adaptive_v_sampling = arg_refs[8].resolve(args, kwargs).as_bool().unwrap();

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

      let mesh = parametric_surface(
        u_res as usize,
        v_res as usize,
        u_closed,
        v_closed,
        flip_normals,
        fku_stitching,
        adaptive_u_sampling,
        adaptive_v_sampling,
        |u, v| {
          let out = ctx
            .invoke_callable(generator, &[Value::Float(u), Value::Float(v)], EMPTY_KWARGS)
            .map_err(|err| {
              err.wrap(
                "Error produced by user-supplied `generator` callable in `parametric_surface`",
              )
            })?;
          out.as_vec3().copied().ok_or_else(|| {
            ErrorStack::new(format!(
              "Expected Vec3 from generator function in `parametric_surface`, found: {out:?}"
            ))
          })
        },
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
  use super::*;
  use crate::parse_and_eval_program;
  use std::f32::consts::PI;

  #[test]
  fn test_parametric_surface_simple_plane() {
    // Simple plane: z = 0, x and y from 0 to 1
    let mesh = parametric_surface(2, 2, false, false, false, false, false, false, |u, v| {
      Ok(Vec3::new(u, v, 0.0))
    })
    .unwrap();

    // 3x3 grid of vertices
    assert_eq!(mesh.vertices.len(), 9);
    // 2x2 quads = 4 quads = 8 triangles
    assert_eq!(mesh.faces.len(), 8);
  }

  #[test]
  fn test_parametric_surface_cylinder() {
    let mesh = parametric_surface(2, 8, false, true, true, false, false, false, |u, v| {
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
    let mesh = parametric_surface(4, 8, false, true, false, false, false, false, |u, v| {
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
    let mesh = parametric_surface(8, 8, true, true, true, false, false, false, |u, v| {
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
    let result = parametric_surface(0, 4, false, false, false, false, false, false, |u, v| {
      Ok(Vec3::new(u, v, 0.0))
    });
    assert!(result.is_err());

    let result = parametric_surface(4, 0, false, false, false, false, false, false, |u, v| {
      Ok(Vec3::new(u, v, 0.0))
    });
    assert!(result.is_err());
  }

  #[test]
  fn test_vertices_are_collapsed() {
    let collapsed = vec![
      Vec3::new(0.0, 0.0, 0.0),
      Vec3::new(1e-6, 0.0, 0.0),
      Vec3::new(0.0, 1e-6, 0.0),
    ];
    assert!(vertices_are_collapsed(&collapsed));

    let not_collapsed = vec![
      Vec3::new(0.0, 0.0, 0.0),
      Vec3::new(1.0, 0.0, 0.0),
      Vec3::new(0.0, 1.0, 0.0),
    ];
    assert!(!vertices_are_collapsed(&not_collapsed));
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
    // torus with uniform stitching (fku_stitching=false for predictable topology)
    let src = r#"
major_r = 2
minor_r = 0.5
mesh = parametric_surface(
  u_res=8,
  v_res=8,
  u_closed=true,
  v_closed=true,
  flip_normals=true,
  fku_stitching=false,
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
    // 64 quads = 128 triangles (uniform stitching)
    assert_eq!(mesh_handle.mesh.faces.len(), 128);
  }
}
