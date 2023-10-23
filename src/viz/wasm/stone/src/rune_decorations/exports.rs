use fnv::FnvHashMap;
use lyon::{
  geom::euclid::{Point2D, Point3D, UnknownUnit, Vector3D},
  lyon_tessellation::VertexBuffers,
};
use wasm_bindgen::prelude::*;

use super::{build_and_tessellate_path, extrude_along_normals, RuneGenParams};

pub struct GeneratedRunes2D {
  /// Populated from scratch by tessellating the path.
  pub buffers: VertexBuffers<Point2D<f32, UnknownUnit>, u32>,
}

pub struct GeneratedRunes3D {
  /// After the 2D coords are projected to 3D using the geodesics, this is
  /// populated with the complete 3D mesh created by extruding the 2D mesh.
  pub buffers: VertexBuffers<Point3D<f32, UnknownUnit>, u32>,
  pub vertex_normals: Vec<Vector3D<f32, UnknownUnit>>,
}

fn params() -> RuneGenParams {
  RuneGenParams {
    segment_length: 8.1,
    segment_width: 1.,
    miter_limit: 1.0,
    subpath_count: 2000,
    extrude_height: 0.5,
    scale: 0.1,
    max_path_length: 100,
  }
}

#[wasm_bindgen]
pub fn generate_rune_decoration_mesh_2d() -> *mut GeneratedRunes2D {
  console_error_panic_hook::set_once();

  let buffers = build_and_tessellate_path(&params());
  let generated = Box::new(GeneratedRunes2D { buffers });
  Box::into_raw(generated)
}

#[wasm_bindgen]
pub fn get_generated_indices_3d(generated_ptr: *mut GeneratedRunes3D) -> Vec<u32> {
  let generated = unsafe { &mut *generated_ptr };
  let indices = std::mem::take(&mut generated.buffers.indices);
  indices
}

#[wasm_bindgen]
pub fn get_generated_vertices_3d(generated_ptr: *mut GeneratedRunes3D) -> Vec<f32> {
  let generated = unsafe { &mut *generated_ptr };
  let raw_vertices = std::mem::take(&mut generated.buffers.vertices);

  assert_eq!(
    std::mem::size_of::<Point3D<f32, UnknownUnit>>(),
    std::mem::size_of::<f32>() * 3
  );
  unsafe {
    let (ptr, len, cap) = raw_vertices.into_raw_parts();
    Vec::from_raw_parts(ptr as *mut f32, len * 3, cap * 3)
  }
}

#[wasm_bindgen]
pub fn get_vertex_normals_3d(generated_ptr: *mut GeneratedRunes3D) -> Vec<f32> {
  let generated = unsafe { &mut *generated_ptr };
  let raw_normals = std::mem::take(&mut generated.vertex_normals);

  assert_eq!(
    std::mem::size_of::<Vector3D<f32, UnknownUnit>>(),
    std::mem::size_of::<f32>() * 3
  );
  unsafe {
    let (ptr, len, cap) = raw_normals.into_raw_parts();
    Vec::from_raw_parts(ptr as *mut f32, len * 3, cap * 3)
  }
}

#[wasm_bindgen]
pub fn get_generated_indices_2d(generated_ptr: *mut GeneratedRunes2D) -> Vec<u32> {
  let generated = unsafe { &mut *generated_ptr };
  let indices = std::mem::take(&mut generated.buffers.indices);
  indices
}

#[wasm_bindgen]
pub fn get_generated_vertices_2d(generated_ptr: *mut GeneratedRunes2D) -> Vec<f32> {
  let generated = unsafe { &mut *generated_ptr };
  let raw_vertices = std::mem::take(&mut generated.buffers.vertices);

  assert_eq!(
    std::mem::size_of::<Point2D<f32, UnknownUnit>>(),
    std::mem::size_of::<f32>() * 2
  );
  unsafe {
    let (ptr, len, cap) = raw_vertices.into_raw_parts();
    Vec::from_raw_parts(ptr as *mut f32, len * 2, cap * 2)
  }
}

#[wasm_bindgen]
pub fn extrude_3d_mesh_along_normals(
  indices: Vec<u32>,
  vertices: Vec<f32>,
) -> *mut GeneratedRunes3D {
  let vertices: Vec<Point3D<f32, _>> = unsafe {
    let (ptr, len, cap) = vertices.into_raw_parts();
    Vec::from_raw_parts(ptr as *mut _, len / 3, cap / 3)
  };
  let params = params();
  let (buffers, vertex_normals) = extrude_along_normals(
    VertexBuffers { indices, vertices },
    params.extrude_height * params.scale,
  );
  let generated = Box::new(GeneratedRunes3D {
    buffers,
    vertex_normals,
  });
  Box::into_raw(generated)
}

#[wasm_bindgen]
pub fn free_generated_runes_3d(ptr: *mut GeneratedRunes3D) {
  drop(unsafe { Box::from_raw(ptr) });
}

#[wasm_bindgen]
pub fn free_generated_runes_2d(ptr: *mut GeneratedRunes2D) {
  drop(unsafe { Box::from_raw(ptr) });
}

pub struct DedupVerticesOutput<I> {
  pub indices: Vec<I>,
  pub vertices: Vec<f32>,
}

fn dedup_vertices<I: TryInto<usize> + TryFrom<usize> + Copy>(
  indices: Vec<I>,
  vertices: Vec<f32>,
) -> *mut DedupVerticesOutput<I>
where
  [I; 3]: std::hash::Hash,
  I: std::ops::AddAssign<I>,
{
  let mut vertex_map = FnvHashMap::default();
  let mut deduped_vertices = Vec::with_capacity(vertices.len());
  let mut deduped_indices = Vec::with_capacity(indices.len());

  let mut deduped_vertex_index: I = unsafe { 0.try_into().unwrap_unchecked() };
  for vertex_index in indices {
    let vertex_key = unsafe {
      std::mem::transmute::<_, [u32; 3]>((
        vertices[vertex_index.try_into().unwrap_unchecked() * 3],
        vertices[vertex_index.try_into().unwrap_unchecked() * 3 + 1],
        vertices[vertex_index.try_into().unwrap_unchecked() * 3 + 2],
      ))
    };
    let deduped_vertex_index_for_vertex = vertex_map.entry(vertex_key).or_insert_with(|| {
      deduped_vertices.push(vertices[unsafe { vertex_index.try_into().unwrap_unchecked() } * 3]);
      deduped_vertices
        .push(vertices[unsafe { vertex_index.try_into().unwrap_unchecked() } * 3 + 1]);
      deduped_vertices
        .push(vertices[unsafe { vertex_index.try_into().unwrap_unchecked() } * 3 + 2]);
      let index = deduped_vertex_index;
      deduped_vertex_index += unsafe { I::try_from(1usize).unwrap_unchecked() };
      index
    });

    deduped_indices.push(*deduped_vertex_index_for_vertex);
  }

  let output = DedupVerticesOutput {
    indices: deduped_indices,
    vertices: deduped_vertices,
  };
  Box::into_raw(Box::new(output))
}

#[wasm_bindgen]
pub fn dedup_vertices_u32(indices: Vec<u32>, vertices: Vec<f32>) -> *mut DedupVerticesOutput<u32> {
  dedup_vertices(indices, vertices)
}

#[wasm_bindgen]
pub fn free_dedup_vertices_output_u32(ptr: *mut DedupVerticesOutput<u32>) {
  drop(unsafe { Box::from_raw(ptr) });
}

#[wasm_bindgen]
pub fn get_deduped_indices_u32(ptr: *mut DedupVerticesOutput<u32>) -> Vec<u32> {
  let output = unsafe { &mut *ptr };
  std::mem::take(&mut output.indices)
}

#[wasm_bindgen]
pub fn get_deduped_vertices_u32(ptr: *mut DedupVerticesOutput<u32>) -> Vec<f32> {
  let output = unsafe { &mut *ptr };
  std::mem::take(&mut output.vertices)
}

#[wasm_bindgen]
pub fn dedup_vertices_u16(indices: Vec<u16>, vertices: Vec<f32>) -> *mut DedupVerticesOutput<u16> {
  dedup_vertices(indices, vertices)
}

#[wasm_bindgen]
pub fn free_dedup_vertices_output_u16(ptr: *mut DedupVerticesOutput<u16>) {
  drop(unsafe { Box::from_raw(ptr) });
}

#[wasm_bindgen]
pub fn get_deduped_indices_u16(ptr: *mut DedupVerticesOutput<u16>) -> Vec<u16> {
  let output = unsafe { &mut *ptr };
  std::mem::take(&mut output.indices)
}

#[wasm_bindgen]
pub fn get_deduped_vertices_u16(ptr: *mut DedupVerticesOutput<u16>) -> Vec<f32> {
  let output = unsafe { &mut *ptr };
  std::mem::take(&mut output.vertices)
}

// #[wasm_bindgen]
// pub fn debug_aabb_tree() -> Vec<f32> {
//   let mut ctx = RuneGenCtx {
//     rng: common::build_rng((8195444438u64, 382173857842u64)),
//     segments: Vec::new(),
//     aabb_tree: AABBTree::new(),
//     builder: Path::builder(),
//   };
//   ctx.populate(&params());

//   let debug_output = ctx.aabb_tree.debug();

//   // Buffer format:
//   //
//   // [depth, minx, miny, maxx, maxy]
//   let mut buffer = Vec::new();
//   for (aabb, depth) in debug_output.internal_nodes {
//     buffer.push(depth as f32);
//     buffer.push(aabb.min[0]);
//     buffer.push(aabb.min[1]);
//     buffer.push(aabb.max[0]);
//     buffer.push(aabb.max[1]);
//   }
//   for aabb in debug_output.leaf_nodes {
//     buffer.push(-1.);
//     buffer.push(aabb.min[0]);
//     buffer.push(aabb.min[1]);
//     buffer.push(aabb.max[0]);
//     buffer.push(aabb.max[1]);
//   }

//   buffer
// }
