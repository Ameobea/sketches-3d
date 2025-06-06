use std::sync::Arc;

use fxhash::FxHashMap;
#[cfg(target_arch = "wasm32")]
use mesh::LinkedMesh;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

use crate::{ArgRef, EvalCtx, Value};

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(module = "src/viz/wasmComp/manifold")]
extern "C" {
  pub fn apply_boolean(
    a_verts: &[f32],
    a_indices: &[u32],
    b_verts: &[f32],
    b_indices: &[u32],
    op: u8,
  ) -> Vec<u8>;
}

#[repr(u8)]
#[derive(Clone, Copy)]
pub enum MeshBooleanOp {
  Union = 0,
  Intersection = 1,
  Difference = 2,
}

#[cfg(target_arch = "wasm32")]
pub fn decode_manifold_output(encoded_output: &[u8]) -> (&[f32], &[u32]) {
  // - 1 u32: vtxCount
  // - 1 u32: triCount
  // - (vtxCount * 3 * f32): vertex positions (x, y, z)
  // - (triCount * 3 * u32): triangle indices (v0, v1, v2)

  assert!(
    encoded_output.len() % 4 == 0,
    "Every element in the output should be 32 bits"
  );

  let u32_view = unsafe {
    std::slice::from_raw_parts(
      encoded_output.as_ptr() as *const u32,
      encoded_output.len() / 4,
    )
  };
  let f32_view = unsafe {
    std::slice::from_raw_parts(
      encoded_output.as_ptr() as *const f32,
      encoded_output.len() / 4,
    )
  };

  let vtx_count = u32_view[0] as usize;
  let tri_count = u32_view[1] as usize;
  let verts = &f32_view[2..(2 + vtx_count * 3)];
  let indices = &u32_view[(2 + vtx_count * 3) as usize..(2 + vtx_count * 3 + tri_count * 3)];

  (verts, indices)
}

#[cfg(target_arch = "wasm32")]
fn apply_boolean_op(
  a: &LinkedMesh<()>,
  b: &LinkedMesh<()>,
  op: MeshBooleanOp,
) -> Result<LinkedMesh<()>, String> {
  use mesh::LinkedMesh;

  let a_exported = a.to_raw_indexed(false, false, true);
  let b_exported = b.to_raw_indexed(false, false, true);

  assert!(std::mem::size_of::<u32>() == std::mem::size_of::<usize>());
  let mesh0_exported_indices = unsafe {
    std::slice::from_raw_parts(
      a_exported.indices.as_ptr() as *const u32,
      a_exported.indices.len(),
    )
  };
  let mesh1_exported_indices = unsafe {
    std::slice::from_raw_parts(
      b_exported.indices.as_ptr() as *const u32,
      b_exported.indices.len(),
    )
  };

  let encoded_output = apply_boolean(
    &a_exported.vertices,
    mesh0_exported_indices,
    &b_exported.vertices,
    mesh1_exported_indices,
    op as u8,
  );

  let (out_verts, out_indices) = decode_manifold_output(&encoded_output);
  Ok(LinkedMesh::from_raw_indexed(
    out_verts,
    out_indices,
    None,
    None,
  ))
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn eval_mesh_boolean(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
  ctx: &EvalCtx,
  op: MeshBooleanOp,
) -> Result<Value, String> {
  let mut meshes_iter = match def_ix {
    0 => {
      let a = arg_refs[0].resolve(&args, &kwargs).as_mesh().unwrap();
      let b = arg_refs[1].resolve(&args, &kwargs).as_mesh().unwrap();

      let out_mesh = apply_boolean_op(&*a, &*b, op)?;
      return Ok(Value::Mesh(Arc::new(out_mesh)));
    }
    1 => {
      let sequence = arg_refs[0].resolve(&args, &kwargs).as_sequence().unwrap();
      sequence.clone_box().consume(ctx)
    }
    _ => unimplemented!(),
  };

  let Some(acc_res) = meshes_iter.next() else {
    return Ok(Value::Mesh(Arc::new(LinkedMesh::new(0, 0, None))));
  };
  let acc = acc_res.map_err(|err| format!("Error evaluating mesh in boolean op: {err}"))?;
  let acc = acc
    .as_mesh()
    .ok_or_else(|| format!("Non-mesh value produced in sequence passed to boolean op: {acc:?}"))?;
  let mut acc = (*acc).clone();

  for res in meshes_iter {
    let mesh = res.map_err(|err| format!("Error evaluating mesh in boolean op: {err}"))?;
    if let Value::Mesh(mesh) = mesh {
      acc = apply_boolean_op(&acc, &mesh, op)?;
    } else {
      return Err("Mesh boolean operations require a sequence of meshes".to_owned());
    }
  }

  Ok(Value::Mesh(Arc::new(acc)))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn eval_mesh_boolean(
  _def_ix: usize,
  _arg_refs: &[ArgRef],
  _args: &[Value],
  _kwargs: &FxHashMap<String, Value>,
  _ctx: &EvalCtx,
  _op: MeshBooleanOp,
) -> Result<Value, String> {
  // Err("mesh boolean ops are only supported in wasm".to_owned())
  Ok(Value::Mesh(Arc::new(mesh::LinkedMesh::new(0, 0, None))))
}
