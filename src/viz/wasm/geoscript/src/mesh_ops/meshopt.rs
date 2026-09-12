//! Fixed-position simplification; retain the source vertex channels and smoothing fans.
#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
#[cfg(target_arch = "wasm32")]
use std::rc::Rc;

#[cfg(test)]
use mesh::linked_mesh::mesh_flags;
#[cfg(any(target_arch = "wasm32", test))]
use mesh::linked_mesh::VertexKey;
#[cfg(any(target_arch = "wasm32", test))]
use mesh::slotmap_utils::vkey;
#[cfg(any(target_arch = "wasm32", test))]
use mesh::LinkedMesh;

#[cfg(target_arch = "wasm32")]
use crate::ManifoldHandle;
use crate::{ErrorStack, MeshHandle};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(module = "src/geoscript/meshopt")]
extern "C" {
  fn meshopt_is_loaded() -> bool;
  #[wasm_bindgen(catch)]
  fn meshopt_simplify(
    indices: &[u32],
    positions: &[f32],
    attributes: &[f32],
    weights: &[f32],
    vertex_locks: &[u8],
    tolerance: f32,
  ) -> Result<Vec<u32>, JsValue>;
}

/// A failed collapse can create a zero-cost non-manifold edge even at tiny error budgets.
/// Protect the affected original one-ring before retrying. If the failure is not an edge
/// incidence problem (for example a pinched vertex), keeping every vertex is the safe last step.
#[cfg(any(target_arch = "wasm32", test))]
fn protect_problem_regions(
  input: &[u32],
  output: &[u32],
  geometric_vertex: &[u32],
  locks: &mut [u8],
) {
  use fxhash::FxHashMap;
  let mut edges = FxHashMap::<(u32, u32), (u32, i32)>::default();
  let mut bad = vec![false; locks.len()];
  for tri in output.chunks_exact(3) {
    let [a, b, c] = [0, 1, 2].map(|i| geometric_vertex[tri[i] as usize]);
    if a == b || b == c || c == a {
      for v in [a, b, c] {
        bad[v as usize] = true;
      }
    }
    for (a, b) in [(a, b), (b, c), (c, a)] {
      let entry = edges.entry((a.min(b), a.max(b))).or_default();
      entry.0 += 1;
      entry.1 += if a < b { 1 } else { -1 };
    }
  }
  for ((a, b), (count, direction)) in edges {
    if count != 2 || direction != 0 {
      bad[a as usize] = true;
      bad[b as usize] = true;
    }
  }
  let mut changed = false;
  for tri in input.chunks_exact(3) {
    if tri
      .iter()
      .any(|&v| bad[geometric_vertex[v as usize] as usize])
    {
      for &v in tri {
        changed |= locks[v as usize] == 0;
        locks[v as usize] = 1;
      }
    }
  }
  if !changed {
    locks.fill(1);
  }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(module = "src/geoscript/manifold")]
extern "C" {
  fn read_manifold_mesh(handle: usize) -> Vec<u8>;
}

/// Remap directly by vertex identity. This preserves missing values and channel policies too.
#[cfg(any(target_arch = "wasm32", test))]
fn rebuild(
  source: &LinkedMesh<()>,
  keys: &[VertexKey],
  indices: &[u32],
  preserve_normals: bool,
) -> LinkedMesh<()> {
  let mut remap = vec![u32::MAX; keys.len()];
  let mut kept = Vec::new();
  let mut vertices = Vec::new();
  let mut normals = Vec::new();
  let indices: Vec<u32> = indices
    .iter()
    .map(|&old| {
      let slot = &mut remap[old as usize];
      if *slot == u32::MAX {
        *slot = vertices.len() as u32;
        let key = keys[old as usize];
        kept.push(key);
        vertices.push(source.vertices[key].position);
        normals.push(source.shading_normal(key).unwrap_or_default());
      }
      *slot
    })
    .collect();
  let mut out = LinkedMesh::from_indexed_vertices(
    &vertices,
    &indices,
    preserve_normals.then_some(normals.as_slice()),
    source.transform,
  );
  out.flags = source.flags;
  for (name, channel) in &source.vertex_channels {
    let mut dst = channel.empty_like();
    for (i, &key) in kept.iter().enumerate() {
      if let Some(value) = channel.get(key) {
        dst.set(vkey(i as u32 + 1, 1), value);
      }
    }
    out.vertex_channels.insert(name.clone(), dst);
  }
  out
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn simplify(
  mesh: &MeshHandle,
  tolerance: f32,
  sharp_angle_degrees: f32,
) -> Result<MeshHandle, ErrorStack> {
  simplify_impl(mesh, tolerance, sharp_angle_degrees, true)
}

#[cfg(any(target_arch = "wasm32", test))]
fn prepare_normals(mut mesh: LinkedMesh<()>, sharp_angle_degrees: f32) -> (LinkedMesh<()>, bool) {
  mesh.mark_edge_sharpness(sharp_angle_degrees.to_radians());
  let preserve = !mesh.edges.values().any(|edge| edge.sharp);
  if !preserve {
    return (mesh, false);
  }
  mesh.compute_vertex_displacement_normals();
  let keys: Vec<_> = mesh.vertices.keys().collect();
  for key in keys {
    mesh.set_shading_normal(key, mesh.displacement_normal(key));
  }
  // Never create normal-fan seams in an otherwise closed geometric mesh. Consumers such as
  // geodesics need its actual topology, not just merge vectors understood by Manifold. For sharp
  // meshes without authored normals, the renderer will split/recompute after simplification.
  (mesh, preserve)
}

#[cfg(target_arch = "wasm32")]
fn has_sharp_edges(mesh: &LinkedMesh<()>, degrees: f32) -> bool {
  let threshold = degrees.to_radians().cos();
  mesh.edges.values().any(|edge| {
    if edge.faces.len() != 2 {
      return true;
    }
    if degrees >= 180. {
      return false;
    }
    let dot = mesh.faces[edge.faces[0]]
      .normal(&mesh.vertices)
      .dot(&mesh.faces[edge.faces[1]].normal(&mesh.vertices));
    !dot.is_finite() || dot < threshold
  })
}

#[cfg(target_arch = "wasm32")]
fn simplify_impl(
  mesh: &MeshHandle,
  tolerance: f32,
  sharp_angle_degrees: f32,
  allow_import_cleanup: bool,
) -> Result<MeshHandle, ErrorStack> {
  use mesh::slotmap_utils::vkey_ix;

  crate::or_async_dep_bit(crate::DEP_BIT_MESHOPT);
  if !meshopt_is_loaded() {
    return Err(ErrorStack::new_uninitialized_module("meshopt"));
  }

  let prepared;
  let (source, preserve_normals) = if mesh.mesh.shading_normals.len() == mesh.mesh.vertices.len() {
    (&*mesh.mesh, true)
  } else if has_sharp_edges(&mesh.mesh, sharp_angle_degrees) {
    (&*mesh.mesh, false)
  } else {
    let (copy, preserve) = prepare_normals((*mesh.mesh).clone(), sharp_angle_degrees);
    prepared = copy;
    (&prepared, preserve)
  };
  let keys: Vec<_> = source.vertices.keys().collect();
  let mut index_of = vec![
    0u32;
    keys
      .iter()
      .map(|key| vkey_ix(key) as usize + 1)
      .max()
      .unwrap_or(0)
  ];
  for (i, key) in keys.iter().enumerate() {
    index_of[vkey_ix(key) as usize] = i as u32;
  }
  let positions: Vec<f32> = keys
    .iter()
    .flat_map(|&key| source.vertices[key].position.iter().copied())
    .collect();
  let indices: Vec<u32> = source
    .faces
    .values()
    .flat_map(|face| face.vertices.map(|key| index_of[vkey_ix(&key) as usize]))
    .collect();

  // Keep the distance budget geometric instead of mixing in an angular normal penalty. Normals
  // are retained by vertex identity independently. Passive channels guide the cost where space
  // allows, and every channel is retained even beyond meshoptimizer's 32 weighted lanes.
  let mut channels: Vec<_> = source.vertex_channels.iter().collect();
  channels.sort_unstable_by_key(|(name, _)| *name);
  let mut weights = Vec::new();
  let mut weighted = Vec::new();
  for (name, channel) in channels {
    let lanes = channel.store.arity();
    if weights.len() + lanes <= 32 {
      weights.extend(std::iter::repeat_n(1., lanes));
      weighted.push((name, channel, lanes));
    }
  }
  let mut attributes = Vec::with_capacity(keys.len() * weights.len());
  for &key in &keys {
    for (_, channel, lanes) in &weighted {
      attributes.extend_from_slice(&channel.get(key).unwrap_or_default()[..*lanes]);
    }
  }

  let mut last_error = None;
  let mut vertex_locks = vec![0u8; keys.len()];
  let mut geometric_vertex = None;
  for attempt in 0..4 {
    let budget = tolerance * 0.5f32.powi(attempt);
    let result = meshopt_simplify(
      &indices,
      &positions,
      &attributes,
      &weights,
      &vertex_locks,
      budget,
    )
    .map_err(|err| {
      ErrorStack::new(
        err
          .as_string()
          .unwrap_or_else(|| format!("meshopt: {err:?}")),
      )
    })?;
    if result.len() % 3 != 0 || result.iter().any(|&i| i as usize >= keys.len()) {
      return Err(ErrorStack::new("meshopt returned invalid triangle indices"));
    }
    if result.is_empty() {
      last_error = Some(ErrorStack::new("meshopt would remove the whole mesh"));
      vertex_locks.fill(1);
      continue;
    }
    let out = MeshHandle {
      mesh: Rc::new(rebuild(source, &keys, &result, preserve_normals)),
      transform: mesh.transform,
      manifold_handle: Rc::new(ManifoldHandle::new(0)),
      aabb: RefCell::new(None),
      bvh: RefCell::new(None),
      material: mesh.material.clone(),
    };
    // Validate the much smaller output through the existing solid importer. This catches the
    // non-manifold results observed in meshoptimizer trials without importing the dense input.
    // Normal/attribute seams use the same explicit merge vectors as other authored meshes.
    match out.get_or_create_handle() {
      Ok(_) => return Ok(out),
      Err(err) => {
        last_error = Some(err);
        let geometric = geometric_vertex.get_or_insert_with(|| {
          if source.has_flag(mesh::linked_mesh::mesh_flags::NO_WELD) {
            let mut seen = fxhash::FxHashMap::default();
            positions
              .chunks_exact(3)
              .enumerate()
              .map(|(i, p)| {
                let key = [0, 1, 2].map(|j| if p[j] == 0. { 0 } else { p[j].to_bits() });
                *seen.entry(key).or_insert(i as u32)
              })
              .collect::<Vec<_>>()
          } else {
            (0..keys.len() as u32).collect()
          }
        });
        protect_problem_regions(&indices, &result, geometric, &mut vertex_locks);
        if attempt == 2 {
          vertex_locks.fill(1);
        }
      }
    }
  }
  if allow_import_cleanup {
    // Some older compositions contain degenerate poles/caps that the original importer cleaned
    // before Manifold simplification. Normalize only on the failed path, then still decimate
    // with meshoptimizer; the common well-formed input avoids importing the dense source.
    let handle = mesh.get_or_create_handle()?;
    let encoded = read_manifold_mesh(handle);
    let decoded = super::mesh_boolean::decode_manifold_output(&encoded);
    let cleaned = MeshHandle {
      mesh: Rc::new(super::mesh_boolean::manifold_output_to_mesh(
        &decoded,
        &mesh.manifold_handle.layout(),
        false,
      )),
      transform: mesh.transform,
      manifold_handle: Rc::clone(&mesh.manifold_handle),
      aabb: RefCell::new(None),
      bvh: RefCell::new(None),
      material: mesh.material.clone(),
    };
    if cleaned.mesh.faces.is_empty() {
      return Ok(cleaned);
    }
    return simplify_impl(&cleaned, tolerance, sharp_angle_degrees, false);
  }
  Err(last_error.unwrap().wrap(
    "meshopt could not retain manifold topology; try a smaller tolerance or engine=\"manifold\"",
  ))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn simplify(
  mesh: &MeshHandle,
  _tolerance: f32,
  _sharp_angle_degrees: f32,
) -> Result<MeshHandle, ErrorStack> {
  // Like the pre-existing native Manifold path, native evaluation is a geometry no-op.
  Ok(mesh.clone(false, false, false))
}

#[cfg(test)]
mod tests {
  use super::*;
  use mesh::linked_mesh::{Arity, Channel, Vec3};

  #[test]
  fn retained_vertices_keep_normals_channel_values_and_missingness() {
    let mut source = LinkedMesh::from_indexed_vertices(
      &[
        Vec3::new(0., 0., 0.),
        Vec3::new(1., 0., 0.),
        Vec3::new(0., 1., 0.),
        Vec3::new(2., 2., 0.),
      ],
      &[0, 1, 2, 1, 3, 2],
      Some(&[Vec3::z(); 4]),
      None,
    );
    source.flags |= mesh_flags::NO_WELD;
    let keys: Vec<_> = source.vertices.keys().collect();
    let mut uv = Channel::for_attr("uv", Arity::Vec2);
    uv.set(keys[0], [0., 0., 0., 0.]);
    uv.set(keys[2], [0., 1., 0., 0.]);
    source.vertex_channels.insert("uv".to_owned(), uv);
    let out = rebuild(&source, &keys, &[2, 0, 3], true);
    let out_keys: Vec<_> = out.vertices.keys().collect();
    assert_eq!(
      out.vertices[out_keys[0]].position,
      source.vertices[keys[2]].position
    );
    assert_eq!(
      out.vertex_channels["uv"].get(out_keys[0]),
      Some([0., 1., 0., 0.])
    );
    assert_eq!(
      out.vertex_channels["uv"].get(out_keys[1]),
      Some([0., 0., 0., 0.])
    );
    assert_eq!(out.vertex_channels["uv"].get(out_keys[2]), None);
    assert!(out_keys
      .iter()
      .all(|&k| out.shading_normal(k) == Some(Vec3::z())));
    assert!(out.has_flag(mesh_flags::NO_WELD));
  }

  #[test]
  fn computing_source_normals_keeps_closed_mesh_topology() {
    let mesh = LinkedMesh::new_box(2., 2., 2.);
    let nv = mesh.vertices.len();
    let nf = mesh.faces.len();
    let (prepared, preserve) = prepare_normals(mesh, 30.);
    assert!(!preserve);
    assert_eq!(prepared.vertices.len(), nv);
    assert_eq!(prepared.faces.len(), nf);
    assert!(!prepared.has_flag(mesh_flags::NO_WELD));
    prepared.check_is_manifold::<true>().unwrap();
  }

  #[test]
  fn invalid_edge_protects_its_input_neighborhood() {
    let input = [0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3];
    let mut output = input.to_vec();
    output.extend_from_slice(&[0, 2, 1]);
    let mut locks = [0; 4];
    protect_problem_regions(&input, &output, &[0, 1, 2, 3], &mut locks);
    assert_eq!(locks, [1; 4]);
  }

  #[test]
  fn engine_and_tolerance_errors_are_reported_by_geoscript() {
    for source in [
      r#"box(2) | simplify(engine="unknown")"#,
      r#"box(2) | simplify(tolerance=-0.1, engine="meshopt")"#,
      r#"box(2) | simplify(tolerance=0, engine="manifold")"#,
    ] {
      assert!(crate::parse_and_eval_program(source).is_err(), "{source}");
    }
    for source in [
      r#"box(2) | simplify"#,
      r#"box(2) | simplify(engine="meshopt")"#,
      r#"simplify(mesh=box(2))"#,
      r#"box(2) | simplify(0.1, engine="meshopt")"#,
      r#"box(2) | simplify(tolerance=0.1, engine="manifold")"#,
    ] {
      let result = crate::parse_and_eval_program(source);
      assert!(result.is_ok(), "{source}: {:?}", result.err());
    }
  }
}
