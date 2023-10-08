use lyon::{lyon_tessellation::VertexBuffers, math::Point};
use wasm_bindgen::prelude::*;

use super::{build_and_tessellate_path, RuneGenParams};

pub struct GeneratedRunes {
  pub buffers: VertexBuffers<Point, u32>,
}

#[wasm_bindgen]
pub fn generate_rune_decoration_mesh() -> *mut GeneratedRunes {
  console_error_panic_hook::set_once();

  let params = RuneGenParams {
    segment_length: 10.1,
  };

  let buffers = build_and_tessellate_path(&params);
  let generated = Box::new(GeneratedRunes { buffers });
  Box::into_raw(generated)
}

#[wasm_bindgen]
pub fn get_generated_indices(generated_ptr: *mut GeneratedRunes) -> Vec<u32> {
  let generated = unsafe { &mut *generated_ptr };
  let indices = std::mem::take(&mut generated.buffers.indices);
  indices
}

#[wasm_bindgen]
pub fn get_generated_vertices(generated_ptr: *mut GeneratedRunes) -> Vec<f32> {
  let generated = unsafe { &mut *generated_ptr };
  let raw_vertices = std::mem::take(&mut generated.buffers.vertices);

  assert_eq!(std::mem::size_of::<Point>(), std::mem::size_of::<f32>() * 2);
  unsafe {
    let (ptr, len, cap) = raw_vertices.into_raw_parts();
    Vec::from_raw_parts(ptr as *mut f32, len * 2, cap * 2)
  }
}

#[wasm_bindgen]
pub fn free_generated_runes(ptr: *mut GeneratedRunes) {
  drop(unsafe { Box::from_raw(ptr) });
}
