//! Tube + strip UV solvers, split out of the core geoscript crate so the main wasm blob doesn't
//! carry them (or faer/gemm) — loaded lazily as a separate module via the async-dep mechanism.

mod strip;
mod tube;

pub use strip::{strip_uvs, Layout, StripOptions, UMode};
pub use tube::{tube_uvs, TubeOptions};

use mesh::{csg::Plane, linked_mesh::Vec3};

pub struct FlatUvMesh {
  pub verts: Vec<f32>,
  pub indices: Vec<u32>,
  /// Interleaved per-vertex [u, v], already scaled.
  pub uvs: Vec<f32>,
  /// Interleaved per-vertex [x, y, z, w].
  pub tangents: Vec<f32>,
}

pub(crate) fn orthonormal_basis(normal: Vec3) -> (Vec3, Vec3) {
  Plane { normal, w: 0. }.compute_basis()
}

#[cfg(target_arch = "wasm32")]
mod wasm {
  use std::cell::RefCell;

  use mesh::LinkedMesh;
  use wasm_bindgen::prelude::wasm_bindgen;

  use crate::{FlatUvMesh, Layout, StripOptions, TubeOptions, UMode};

  thread_local! {
    static OUTPUT: RefCell<Option<FlatUvMesh>> = const { RefCell::new(None) };
  }

  fn store(res: Result<FlatUvMesh, String>) -> String {
    match res {
      Ok(out) => {
        OUTPUT.with(|o| *o.borrow_mut() = Some(out));
        String::new()
      }
      Err(err) => err,
    }
  }

  fn take_out<T>(f: impl FnOnce(&mut FlatUvMesh) -> Vec<T>) -> Vec<T> {
    OUTPUT.with(|o| f(o.borrow_mut().as_mut().unwrap()))
  }

  #[wasm_bindgen]
  pub fn uv_solvers_tube(
    verts: &[f32],
    indices: &[u32],
    scale: f32,
    sharp_threshold_rad: f32,
    caps: bool,
    cap_angle_rad: f32,
    cap_max_span: f64,
    cap_alignment: f64,
    normalize_v: bool,
    seam_straightness: f64,
    detwist: bool,
  ) -> String {
    let m = LinkedMesh::from_raw_indexed(verts, indices, None, None);
    store(crate::tube_uvs(m, scale, sharp_threshold_rad, &TubeOptions {
      caps,
      cap_angle_rad: if cap_angle_rad.is_nan() {
        None
      } else {
        Some(cap_angle_rad)
      },
      cap_max_span,
      cap_alignment,
      normalize_v,
      seam_straightness,
      detwist,
    }))
  }

  #[wasm_bindgen]
  pub fn uv_solvers_strip(
    verts: &[f32],
    indices: &[u32],
    scale: f32,
    sharp_threshold_rad: f32,
    strip_angle_rad: f32,
    layout: u8,
    u_mode: u8,
    planar_fallback: bool,
  ) -> String {
    let m = LinkedMesh::from_raw_indexed(verts, indices, None, None);
    store(crate::strip_uvs(m, scale, sharp_threshold_rad, &StripOptions {
      strip_angle_rad: if strip_angle_rad.is_nan() {
        None
      } else {
        Some(strip_angle_rad)
      },
      layout: Layout::from_u8(layout),
      u_mode: UMode::from_u8(u_mode),
      planar_fallback,
    }))
  }

  #[wasm_bindgen]
  pub fn uv_solvers_get_verts() -> Vec<f32> {
    take_out(|o| std::mem::take(&mut o.verts))
  }

  #[wasm_bindgen]
  pub fn uv_solvers_get_indices() -> Vec<u32> {
    take_out(|o| std::mem::take(&mut o.indices))
  }

  #[wasm_bindgen]
  pub fn uv_solvers_get_uvs() -> Vec<f32> {
    take_out(|o| std::mem::take(&mut o.uvs))
  }

  #[wasm_bindgen]
  pub fn uv_solvers_get_tangents() -> Vec<f32> {
    take_out(|o| std::mem::take(&mut o.tangents))
  }
}
