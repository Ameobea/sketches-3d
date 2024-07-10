use std::f32::consts::PI;

use common::random;
use log::info;
use mesh::{
  linked_mesh::{set_debug_print, DisplacementNormalMethod},
  LinkedMesh, OwnedIndexedMesh, Triangle,
};
use nalgebra::Vector3;
use noise::{self, MultiFractal, NoiseModule, Seedable};
use wasm_bindgen::prelude::*;

/// Connects two hexagons by generating the side faces between them.
fn connect_hexes(
  triangles: &mut Vec<Triangle>,
  hex0: &[Triangle],
  hex1: &[Triangle],
  [hex0_vtx0_ix, hex0_vtx1_ix]: [usize; 2],
  [hex1_vtx0_ix, hex1_vtx1_ix]: [usize; 2],
) {
  let hex0_vtx0 = hex0[hex0_vtx0_ix].b;
  let hex0_vtx1 = hex0[hex0_vtx1_ix].b;
  let hex1_vtx0 = hex1[hex1_vtx0_ix].b;
  let hex1_vtx1 = hex1[hex1_vtx1_ix].b;

  // add two triangles to join the edges
  triangles.push(Triangle {
    a: hex0_vtx0,
    b: hex0_vtx1,
    c: hex1_vtx0,
  });
  triangles.push(Triangle {
    a: hex0_vtx1,
    b: hex1_vtx1,
    c: hex1_vtx0,
  });
}

/// Generates a tessellated hex grid centered at the origin.  Each hex is
/// extruded dynamically based on the height returned by `get_hex_height` at its
/// center.
///
/// `hex_width` corresponds to the distance between the two parallel flat sides.
fn gen_tessellated_hex_grid(
  x_count: usize,
  y_count: usize,
  hex_width: f32,
  get_hex_height: impl Fn(f32, f32) -> f32,
) -> Vec<Triangle> {
  let mut triangles = Vec::new();
  let hex_height = (3.0_f32).sqrt() * hex_width / 2.0;

  for y in 0..y_count {
    for x in 0..x_count {
      let hex_center_x = x as f32 * 1.5 * hex_width;
      let hex_center_z = y as f32 * 2.0 * hex_height + if x % 2 == 0 { 0. } else { hex_height };
      let hex_height = get_hex_height(hex_center_x, hex_center_z);

      let mut vertices = Vec::new();
      // generates verts in the order of right, bottom right, bottom left, left, top
      // left, top right
      for i in 0..6 {
        let angle = PI / 3.0 * i as f32;
        let px = hex_center_x + hex_width * angle.cos();
        let pz = hex_center_z + hex_width * angle.sin();
        vertices.push(Vector3::new(px, hex_height, pz));
      }

      for i in 0..6 {
        let next_i = (i + 1) % 6;
        // CCW winding order as usual
        triangles.push(Triangle {
          a: vertices[next_i],
          b: vertices[i],
          c: Vector3::new(hex_center_x, hex_height, hex_center_z),
        });
      }
    }
  }

  // Generate connecting faces
  let mut new_triangles: Vec<_> = Vec::new();
  for y in 0..y_count {
    for x in 0..x_count {
      let ix = y * x_count + x;
      let base_hex_triangles = &triangles[ix * 6..ix * 6 + 6];

      let bottom_right_ix = if x == x_count - 1 {
        usize::MAX
      } else {
        if x % 2 == 0 {
          ix + 1
        } else {
          ix + x_count + 1
        }
      };
      if bottom_right_ix < x_count * y_count {
        let bottom_right_hex_triangles = &triangles[bottom_right_ix * 6..bottom_right_ix * 6 + 6];
        connect_hexes(
          &mut new_triangles,
          base_hex_triangles,
          bottom_right_hex_triangles,
          [0, 1],
          [4, 3],
        );
      }

      let bottom_ix = ix + x_count;
      if bottom_ix < x_count * y_count {
        let top_right_hex_triangles = &triangles[bottom_ix * 6..bottom_ix * 6 + 6];
        connect_hexes(
          &mut new_triangles,
          base_hex_triangles,
          top_right_hex_triangles,
          [1, 2],
          [5, 4],
        );
      }

      let bottom_left_ix = if x > 0 {
        if x % 2 == 0 {
          ix - 1
        } else {
          ix + x_count - 1
        }
      } else {
        usize::MAX
      };
      if bottom_left_ix < x_count * y_count {
        let bottom_left_hex_triangles = &triangles[bottom_left_ix * 6..bottom_left_ix * 6 + 6];
        connect_hexes(
          &mut new_triangles,
          base_hex_triangles,
          bottom_left_hex_triangles,
          [2, 3],
          [0, 5],
        );
      }

      triangles.extend(new_triangles.drain(..));
    }
  }

  triangles
}

pub struct GenBasaltCtx {
  pub terrrain_mesh: OwnedIndexedMesh,
}

static mut DID_INIT: bool = false;

fn maybe_init() {
  unsafe {
    if DID_INIT {
      return;
    }
    DID_INIT = true;
  }

  common::maybe_init_rng();
  console_error_panic_hook::set_once();
  wasm_logger::init(wasm_logger::Config::new(log::Level::Debug));

  set_debug_print(|s| info!("{}", s));
}

fn round_to_nearest_multiple(value: f32, multiple: f32) -> f32 {
  (value / multiple).round() * multiple
}

fn displace_mesh(mesh: &mut LinkedMesh) {
  let noise = noise::Fbm::new().set_octaves(4);
  for (_vtx_key, vtx) in &mut mesh.vertices {
    let displacement_normal = vtx
      .displacement_normal
      .expect("Expected displacement normal to be set by now");
    if displacement_normal.x.is_nan()
      || displacement_normal.y.is_nan()
      || displacement_normal.z.is_nan()
    {
      // TODO: have to figure out why this happens
      // log::warn!("Displacement normal is NaN; skipping displace");
      continue;
    }

    let pos = vtx.position * 0.2;
    let noise = noise.get([pos.x, pos.y, pos.z]); //.abs();
    vtx.position += displacement_normal * noise * 0.8;
  }
}

#[wasm_bindgen]
pub fn basalt_gen() -> *mut GenBasaltCtx {
  maybe_init();

  let coarse_noise = noise::Fbm::new().set_octaves(1).set_seed(393939939);
  let noise_gen: noise::Fbm<f32> = noise::Fbm::new().set_octaves(2).set_seed(393939939);
  let get_height = |x, z| {
    let void = coarse_noise.get([x * 0.05, z * 0.05]) < 0.2;
    if void {
      return -20.;
    }
    let mut height = (noise_gen.get([x * 0.03, z * 0.03]) + 1.) * 20.;
    if random() < 0.7 {
      height =
        round_to_nearest_multiple(height, 20.) + if random() > 0.6 { random() * 1.2 } else { 0. };
    }
    height
  };
  let terrain_triangles = gen_tessellated_hex_grid(8, 8, 7., get_height);

  let mut mesh = LinkedMesh::from_triangles(&terrain_triangles);
  let merged_count = mesh.merge_vertices_by_distance(0.0001);
  info!(
    "Merged {merged_count} vertices by distance; {} remaining",
    mesh.vertices.len()
  );

  let sharp_edge_threshold_rads = 0.8;
  mesh.mark_edge_sharpness(sharp_edge_threshold_rads);
  mesh.compute_vertex_displacement_normals();
  tessellation::tessellate_mesh(&mut mesh, 1.5, DisplacementNormalMethod::Interpolate);
  info!("Tessellated mesh; new vertex count={}", mesh.vertices.len());

  displace_mesh(&mut mesh);

  mesh.mark_edge_sharpness(sharp_edge_threshold_rads);
  mesh.compute_edge_displacement_normals();
  mesh.separate_vertices_and_compute_normals();
  info!(
    "Separated vertices; new vertex count={}",
    mesh.vertices.len()
  );

  Box::into_raw(Box::new(GenBasaltCtx {
    terrrain_mesh: mesh.to_raw_indexed(),
  }))
}

#[wasm_bindgen]
pub fn basalt_take_vertices(ctx: *mut GenBasaltCtx) -> Vec<f32> {
  let ctx = unsafe { &mut (*ctx) };
  std::mem::take(&mut ctx.terrrain_mesh.vertices)
}

#[wasm_bindgen]
pub fn basalt_take_indices(ctx: *mut GenBasaltCtx) -> Vec<usize> {
  let ctx = unsafe { &mut (*ctx) };
  std::mem::take(&mut ctx.terrrain_mesh.indices)
}

#[wasm_bindgen]
pub fn basalt_take_normals(ctx: *mut GenBasaltCtx) -> Vec<f32> {
  let ctx = unsafe { &mut (*ctx) };
  std::mem::take(
    &mut ctx
      .terrrain_mesh
      .shading_normals
      .as_mut()
      .expect("Shading normals not found"),
  )
}

#[wasm_bindgen]
pub fn basalt_free(ctx: *mut GenBasaltCtx) {
  drop(unsafe { Box::from_raw(ctx) });
}
