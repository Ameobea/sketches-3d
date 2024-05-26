#![feature(iter_array_chunks)]

use common::mesh::{Mesh, OwnedMesh, Triangle};

mod interface;

fn tessellate_mesh(mesh: Mesh, target_triangle_area: f32) -> OwnedMesh {
  let mut new_vertices = Vec::with_capacity(mesh.vertices.len());
  let mut new_normals = Vec::with_capacity(mesh.normals.as_ref().map(|v| v.len()).unwrap_or(0));

  for (tri_ix, [a, b, c]) in mesh.vertices.iter().array_chunks::<3>().enumerate() {
    let tri = Triangle::new(*a, *b, *c);
    let area = tri.area();
    let subdivided_area = area / 6.;

    // if current area is closer to target area than subdivided area, add the
    // triangle as is
    if (target_triangle_area - area).abs() < (target_triangle_area - subdivided_area).abs() {
      new_vertices.push(*a);
      new_vertices.push(*b);
      new_vertices.push(*c);

      if let Some(normals) = mesh.normals {
        let normals = &normals[tri_ix * 3..tri_ix * 3 + 3];
        new_normals.extend_from_slice(normals);
      }

      continue;
    }

    let center = tri.center();
    // split each edge in half and draw new edges from the center to the midpoints
    //
    // this splits the triangle into 6 smaller triangles

    let ab = (*a + *b) / 2.;
    let bc = (*b + *c) / 2.;
    let ca = (*c + *a) / 2.;

    if let Some(normals) = mesh.normals {
      let normals = &normals[tri_ix * 3..tri_ix * 3 + 3];
      let a_norm = &normals[0];
      let b_norm = &normals[1];
      let c_norm = &normals[2];
      let ab_normal = (a_norm + b_norm) / 2.;
      let bc_normal = (b_norm + c_norm) / 2.;
      let ca_normal = (c_norm + a_norm) / 2.;
      // let center_normal = (a_norm + b_norm + c_norm) / 3.;
      let center_normal = tri.normal();

      // we'll assume that smooth shading is used, so the normals of the new vertices
      // will be interpolated from those of the original vertices
      let new_triangles_and_normals = [
        ([*a, ab, center], [a_norm, &ab_normal, &center_normal]),
        ([ab, *b, center], [&ab_normal, &b_norm, &center_normal]),
        ([*b, bc, center], [b_norm, &bc_normal, &center_normal]),
        ([bc, *c, center], [&bc_normal, &c_norm, &center_normal]),
        ([*c, ca, center], [c_norm, &ca_normal, &center_normal]),
        ([ca, *a, center], [&ca_normal, a_norm, &center_normal]),
      ];

      for ([a, b, c], [a_norm, b_norm, c_norm]) in new_triangles_and_normals {
        new_vertices.push(a);
        new_vertices.push(b);
        new_vertices.push(c);

        new_normals.push(*a_norm);
        new_normals.push(*b_norm);
        new_normals.push(*c_norm);
      }
    } else {
      let new_triangles = [
        [*a, ab, center],
        [ab, *b, center],
        [*b, bc, center],
        [bc, *c, center],
        [*c, ca, center],
        [ca, *a, center],
      ];

      for [a, b, c] in new_triangles {
        new_vertices.push(a);
        new_vertices.push(b);
        new_vertices.push(c);
      }
    }
  }

  OwnedMesh {
    vertices: new_vertices,
    normals: if mesh.normals.is_some() {
      Some(new_normals)
    } else {
      None
    },
    transform: mesh.transform,
  }
}
