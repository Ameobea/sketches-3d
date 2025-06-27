use crate::algorithm::NoiseParams;
use common::maybe_init_rng;
use nanoserde::DeJson;

mod imports {
  extern "C" {
    #[allow(dead_code)]
    pub fn log_msg(msg: *const u8, len: usize);
    pub fn log_error(msg: *const u8, len: usize);
  }
}

#[allow(dead_code)]
pub(crate) fn log(s: &str) {
  unsafe {
    imports::log_msg(s.as_ptr(), s.len());
  }
}

pub(crate) fn log_error(s: &str) {
  unsafe {
    imports::log_error(s.as_ptr(), s.len());
  }
}

static mut PANIC_HOOK_SET: bool = false;

fn maybe_init_panic_hook() {
  if unsafe { PANIC_HOOK_SET } {
    return;
  }

  std::panic::set_hook(Box::new(|info| log_error(&format!("{}", info))));
}

pub struct TerrainGenCtx {
  pub noise_params: Option<NoiseParams>,
}

#[no_mangle]
pub extern "C" fn create_terrain_gen_ctx() -> *mut TerrainGenCtx {
  maybe_init_rng();
  maybe_init_panic_hook();

  Box::into_raw(Box::new(TerrainGenCtx { noise_params: None }))
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn malloc(size: usize) -> *mut u8 {
  let mut v: Vec<u8> = Vec::with_capacity(size);
  unsafe {
    v.set_len(size);
  }
  let ptr = v.as_mut_ptr();
  std::mem::forget(v);
  ptr
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn free(ptr: *mut u8, size: usize) {
  drop(unsafe { Vec::from_raw_parts(ptr, size, size) });
}

#[no_mangle]
pub extern "C" fn set_params(ctx: *mut TerrainGenCtx, params: *const u8, size: usize) {
  let serialized_params = unsafe { std::slice::from_raw_parts(params, size) };
  let serialized_params = unsafe { std::str::from_utf8_unchecked(serialized_params) };
  let params =
    NoiseParams::deserialize_json(&serialized_params).expect("Failed to deserialize params");
  unsafe {
    (*ctx).noise_params = Some(params);
  }
}

#[no_mangle]
pub extern "C" fn gen_heightmap(
  ctx: *mut TerrainGenCtx,
  resolution_x: usize,
  resolution_y: usize,
  world_space_min_x: f32,
  world_space_min_y: f32,
  world_space_max_x: f32,
  world_space_max_y: f32,
) -> *const f32 {
  let ctx = unsafe { &mut *ctx };
  let resolution = (resolution_x, resolution_y);
  let world_space_mins = (world_space_min_x, world_space_min_y);
  let world_space_maxs = (world_space_max_x, world_space_max_y);
  let noise_source = ctx
    .noise_params
    .as_mut()
    .expect("Noise params not set")
    .build();
  let heightmap = crate::gen_heightmap(
    noise_source,
    resolution,
    (world_space_mins, world_space_maxs),
  );

  let heightmap = heightmap.into_boxed_slice();
  let ptr = Box::into_raw(heightmap) as *const f32;
  ptr
}
