use std::rc::Rc;

use fxhash::FxHashMap;
#[cfg(target_arch = "wasm32")]
use mesh::LinkedMesh;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

use crate::ErrorStack;
#[cfg(target_arch = "wasm32")]
use crate::MeshHandle;
use crate::{ArgRef, EvalCtx, Value};

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(module = "src/geoscript/manifold")]
extern "C" {
  pub fn apply_boolean(
    a_handle: usize,
    a_transform: &[f32],
    b_handle: usize,
    b_transform: &[f32],
    op: u8,
    handle_only: bool,
  ) -> Vec<u8>;
  pub fn create_manifold(vertices: &[f32], indices: &[u32]) -> isize;
  fn drop_mesh_handle(handle: usize);
  pub fn drop_all_mesh_handles();
  fn get_last_err() -> String;
}

#[cfg(target_arch = "wasm32")]
pub fn get_last_manifold_err() -> String {
  get_last_err()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn get_last_manifold_err() -> String {
  String::new()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn create_manifold(_vertices: &[f32], _indices: &[u32]) -> usize {
  0
}

#[cfg(target_arch = "wasm32")]
pub fn drop_manifold_mesh_handle(handle: usize) {
  drop_mesh_handle(handle);
}

#[cfg(not(target_arch = "wasm32"))]
pub fn drop_manifold_mesh_handle(_handle: usize) {}

#[repr(u8)]
#[derive(Clone, Copy)]
pub enum MeshBooleanOp {
  Union = 0,
  Intersection = 1,
  Difference = 2,
}

impl MeshBooleanOp {
  pub(crate) fn from_str(name: &str) -> Self {
    match name {
      "union" => MeshBooleanOp::Union,
      "intersect" => MeshBooleanOp::Intersection,
      "difference" => MeshBooleanOp::Difference,
      _ => panic!("Unknown mesh boolean operation: {name}"),
    }
  }
}

#[cfg(target_arch = "wasm32")]
pub fn decode_manifold_output(encoded_output: &[u8]) -> (usize, &[f32], &[u32]) {
  // - 1 u32: manifold handle
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
  let manifold_handle = u32_view[2] as usize;
  let verts = &f32_view[3..(3 + vtx_count * 3)];
  let indices = &u32_view[(3 + vtx_count * 3) as usize..(3 + vtx_count * 3 + tri_count * 3)];

  (manifold_handle, verts, indices)
}

#[cfg(target_arch = "wasm32")]
fn apply_mesh_boolean_op(
  a: &MeshHandle,
  b: &MeshHandle,
  op: MeshBooleanOp,
  handle_only: bool,
) -> Result<MeshHandle, ErrorStack> {
  use std::cell::RefCell;

  use mesh::LinkedMesh;
  use nalgebra::Matrix4;

  use crate::ManifoldHandle;

  let a_handle = a
    .get_or_create_handle()
    .map_err(|err| err.wrap("Error applying mesh boolean op"))?;
  let b_handle = b
    .get_or_create_handle()
    .map_err(|err| err.wrap("Error applying mesh boolean op"))?;

  let encoded_output = apply_boolean(
    a_handle,
    &a.transform.as_slice(),
    b_handle,
    &b.transform.as_slice(),
    op as u8,
    handle_only,
  );

  let (manifold_handle, out_verts, out_indices) = decode_manifold_output(&encoded_output);

  let mesh: LinkedMesh<()> = LinkedMesh::from_raw_indexed(out_verts, out_indices, None, None);
  Ok(MeshHandle {
    mesh: Rc::new(mesh),
    transform: Matrix4::identity(),
    manifold_handle: Rc::new(ManifoldHandle::new(manifold_handle)),
    aabb: RefCell::new(None),
    trimesh: RefCell::new(None),
    material: None,
  })
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn eval_mesh_boolean(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
  ctx: &EvalCtx,
  op: MeshBooleanOp,
) -> Result<Value, ErrorStack> {
  use std::rc::Rc;

  let mut meshes_iter = match def_ix {
    0 => {
      let a = arg_refs[0].resolve(&args, &kwargs).as_mesh().unwrap();
      let b = arg_refs[1].resolve(&args, &kwargs).as_mesh().unwrap();

      let out_mesh = apply_mesh_boolean_op(&*a, &*b, op, false)?;
      return Ok(Value::Mesh(Rc::new(out_mesh)));
    }
    1 => {
      let sequence = arg_refs[0].resolve(&args, &kwargs).as_sequence().unwrap();
      sequence.clone_box().consume(ctx)
    }
    _ => unimplemented!(),
  };

  let Some(acc_res) = meshes_iter.next() else {
    use std::rc::Rc;

    return Ok(Value::Mesh(Rc::new(MeshHandle::new(Rc::new(
      LinkedMesh::new(0, 0, None),
    )))));
  };
  let acc = acc_res.map_err(|err| err.wrap("Error evaluating mesh in boolean op"))?;
  let mut acc = acc
    .as_mesh()
    .ok_or_else(|| {
      ErrorStack::new(format!(
        "Non-mesh value produced in sequence passed to boolean op: {acc:?}"
      ))
    })?
    .clone(true, true, true);

  let mut meshes_iter = meshes_iter.peekable();
  while let Some(res) = meshes_iter.next() {
    let mesh = res
      .map_err(|err| err.wrap("Error produced from iterator passed to mesh boolean function"))?;
    if let Value::Mesh(mesh) = mesh {
      let handle_only = meshes_iter.peek().is_some();
      acc = apply_mesh_boolean_op(&acc, &mesh, op, handle_only)?;
    } else {
      return Err(ErrorStack::new(
        "Non-mesh value produced in sequence passed to boolean op",
      ));
    }
  }

  Ok(Value::Mesh(Rc::new(acc)))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn eval_mesh_boolean(
  _def_ix: usize,
  _arg_refs: &[ArgRef],
  _args: &[Value],
  _kwargs: &FxHashMap<String, Value>,
  _ctx: &EvalCtx,
  _op: MeshBooleanOp,
) -> Result<Value, ErrorStack> {
  // Err("mesh boolean ops are only supported in wasm".to_owned())
  Ok(Value::Mesh(Rc::new(crate::MeshHandle::new(Rc::new(
    mesh::LinkedMesh::new(0, 0, None).into(),
  )))))
}
