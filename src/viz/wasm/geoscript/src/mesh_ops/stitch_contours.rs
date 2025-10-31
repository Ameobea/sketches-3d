use mesh::{
  linked_mesh::{Vec3, Vertex, VertexKey},
  LinkedMesh,
};
use smallvec::SmallVec;

use crate::{ErrorStack, Value};

pub fn stitch_contours<'a>(
  contours: &mut [Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a>],
  flipped: bool,
  closed: bool,
  cap_start: bool,
  cap_end: bool,
) -> Result<LinkedMesh<()>, ErrorStack> {
  if contours.len() < 2 {
    return Err(ErrorStack::new(
      "`stitch_contours` requires at least two contours",
    ));
  }

  let mut mesh = LinkedMesh::new(0, 0, None);

  fn next_vtx(
    mesh: &mut LinkedMesh<()>,
    seq_ix: usize,
    seq: &mut dyn Iterator<Item = Result<Value, ErrorStack>>,
  ) -> Result<Option<VertexKey>, ErrorStack> {
    match seq.next() {
      // TODO: if the position is exactly the same as the previous vertex, re-use prev vtx key
      Some(Ok(Value::Vec3(v3))) => Ok(Some(mesh.vertices.insert(Vertex {
        position: v3,
        shading_normal: None,
        displacement_normal: None,
        edges: SmallVec::new(),
        _padding: Default::default(),
      }))),
      Some(Ok(other)) => Err(ErrorStack::new(format!(
        "Invalid value produced in seq for contour ix={seq_ix}; expected Vec3, found: {other:?}",
      ))),
      Some(Err(e)) => Err(e),
      None => Ok(None),
    }
  }

  fn build_cap(mesh: &mut LinkedMesh<()>, contour_verts: &[VertexKey], flipped: bool) {
    let center = contour_verts
      .iter()
      .map(|v| mesh.vertices[*v].position)
      .sum::<Vec3>()
      / contour_verts.len() as f32;
    let center_vtx = mesh.vertices.insert(Vertex {
      position: center,
      shading_normal: None,
      displacement_normal: None,
      edges: SmallVec::new(),
      _padding: Default::default(),
    });

    for (ix0, ix1) in contour_verts
      .iter()
      .zip(contour_verts.iter().cycle().skip(1))
    {
      let v0 = *ix0;
      let v1 = *ix1;

      let tri = if flipped {
        [v0, center_vtx, v1]
      } else {
        [v1, center_vtx, v0]
      };
      mesh.add_face::<true>(tri, ());
    }
  }

  let start_verts = contours
    .iter_mut()
    .enumerate()
    .map(|(seq_ix, c)| match next_vtx(&mut mesh, seq_ix, c) {
      Ok(Some(vtx)) => Ok(vtx),
      Ok(None) => Err(ErrorStack::new(format!(
        "Contour sequence {seq_ix} is empty; expected at least one vertex"
      ))),
      Err(e) => Err(e),
    })
    .collect::<Result<Vec<_>, _>>()?;
  let mut frontier_verts = start_verts.clone();
  let mut new_frontier_verts = frontier_verts.clone();

  let mut first_contour_verts = Vec::new();
  if cap_start {
    first_contour_verts.push(start_verts[0]);
  }
  let mut last_contour_verts = Vec::new();
  if cap_end {
    last_contour_verts.push(start_verts[start_verts.len() - 1]);
  }

  'outer: loop {
    for ix0 in 0..frontier_verts.len() - 1 {
      let ix1 = ix0 + 1;
      let v0_0 = frontier_verts[ix0];
      let v0_1 = if ix0 == 0 {
        let vtx = match next_vtx(&mut mesh, ix0, &mut contours[ix0])? {
          Some(v) => v,
          None => break 'outer,
        };
        if cap_start {
          first_contour_verts.push(vtx);
        }
        vtx
      } else {
        new_frontier_verts[ix0]
      };
      let v1_0 = frontier_verts[ix1];
      let v1_1 = match next_vtx(&mut mesh, ix1, &mut contours[ix1])? {
        Some(v) => v,
        None => break 'outer,
      };
      if ix1 == frontier_verts.len() - 1 && cap_end {
        last_contour_verts.push(v1_1);
      }

      new_frontier_verts[ix0] = v0_1;
      new_frontier_verts[ix1] = v1_1;

      let (tri0, tri1) = if flipped {
        ([v0_0, v1_1, v1_0], [v0_0, v0_1, v1_1])
      } else {
        ([v1_0, v1_1, v0_0], [v1_1, v0_1, v0_0])
      };

      mesh.add_face::<true>(tri0, ());
      mesh.add_face::<true>(tri1, ());
    }

    std::mem::swap(&mut frontier_verts, &mut new_frontier_verts);
  }

  if cap_start {
    build_cap(&mut mesh, &first_contour_verts, flipped);
  }
  if cap_end {
    build_cap(&mut mesh, &last_contour_verts, !flipped);
  }

  if closed {
    for ix0 in 0..frontier_verts.len() - 1 {
      let ix1 = ix0 + 1;
      let v0_0 = frontier_verts[ix0];
      let v0_1 = start_verts[ix0];
      let v1_0 = frontier_verts[ix1];
      let v1_1 = start_verts[ix1];

      let (tri0, tri1) = if flipped {
        ([v0_0, v1_1, v1_0], [v0_0, v0_1, v1_1])
      } else {
        ([v1_0, v1_1, v0_0], [v1_1, v0_1, v0_0])
      };

      mesh.add_face::<true>(tri0, ());
      mesh.add_face::<true>(tri1, ());
    }
  }

  Ok(mesh)
}
