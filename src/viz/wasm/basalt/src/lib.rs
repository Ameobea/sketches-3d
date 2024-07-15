#![feature(array_windows)]

use std::f32::consts::PI;

use bitvec::bitarr;
use common::{random, smoothstep};
use log::info;
use mesh::{
  linked_mesh::{set_debug_print, DisplacementNormalMethod, Edge},
  LinkedMesh, OwnedIndexedMesh, Triangle,
};
use nalgebra::Vector3;
use noise::{self, Fbm, MultiFractal, NoiseModule, Seedable};
use wasm_bindgen::prelude::*;

/// Connects two hexagons by generating the side faces between them.
///
/// We need to make sure that the mesh remains manifold, so we have to split the
/// side face at levels specified by the `heights` array.
fn connect_hexes(
  triangles: &mut Vec<Triangle>,
  hex0: &[Triangle],
  hex1: &[Triangle],
  [hex0_vtx0_ix, hex0_vtx1_ix]: [usize; 2],
  heights: [f32; 2],
  split_heights: [Option<f32>; 2],
) {
  let hex0_height = hex0[0].b.y;
  let hex1_height = hex1[0].b.y;

  if (hex0_height - hex1_height).abs() < 0.0001 {
    return;
  }

  let left_heights: &mut [_] = if let Some(split_height) = split_heights[0] {
    &mut [heights[0], heights[1], split_height]
  } else {
    &mut [heights[0], heights[1]]
  };
  left_heights.sort_unstable_by(if heights[0] > heights[1] {
    |a: &f32, b: &f32| a.partial_cmp(b).unwrap()
  } else {
    |a: &f32, b: &f32| b.partial_cmp(a).unwrap()
  });
  let right_heights: &mut [_] = if let Some(split_height) = split_heights[1] {
    &mut [heights[0], heights[1], split_height]
  } else {
    &mut [heights[0], heights[1]]
  };
  right_heights.sort_unstable_by(if heights[0] > heights[1] {
    |a: &f32, b: &f32| a.partial_cmp(b).unwrap()
  } else {
    |a: &f32, b: &f32| b.partial_cmp(a).unwrap()
  });

  let hex0_vtx0 = hex0[hex0_vtx0_ix].b;
  let hex0_vtx1 = hex0[hex0_vtx1_ix].b;

  let get_vtx = |is_left: bool, ix: usize| -> Vector3<f32> {
    if is_left {
      Vector3::new(hex0_vtx0.x, left_heights[ix], hex0_vtx0.z)
    } else {
      Vector3::new(hex0_vtx1.x, right_heights[ix], hex0_vtx1.z)
    }
  };

  const LEFT: bool = true;
  const RIGHT: bool = false;
  let tris: &[Triangle] = match (left_heights.len(), right_heights.len()) {
    (2, 2) => &[
      Triangle::new(get_vtx(RIGHT, 0), get_vtx(LEFT, 0), get_vtx(RIGHT, 1)),
      Triangle::new(get_vtx(RIGHT, 1), get_vtx(LEFT, 0), get_vtx(LEFT, 1)),
    ],
    (3, 2) => &[
      Triangle::new(get_vtx(RIGHT, 0), get_vtx(LEFT, 0), get_vtx(LEFT, 1)),
      Triangle::new(get_vtx(RIGHT, 0), get_vtx(LEFT, 1), get_vtx(RIGHT, 1)),
      Triangle::new(get_vtx(RIGHT, 1), get_vtx(LEFT, 1), get_vtx(LEFT, 2)),
    ],
    (2, 3) => &[
      Triangle::new(get_vtx(RIGHT, 0), get_vtx(LEFT, 0), get_vtx(RIGHT, 1)),
      Triangle::new(get_vtx(RIGHT, 1), get_vtx(LEFT, 0), get_vtx(LEFT, 1)),
      Triangle::new(get_vtx(RIGHT, 1), get_vtx(LEFT, 1), get_vtx(RIGHT, 2)),
    ],
    (3, 3) => &[
      Triangle::new(get_vtx(RIGHT, 0), get_vtx(LEFT, 0), get_vtx(RIGHT, 1)),
      Triangle::new(get_vtx(RIGHT, 1), get_vtx(LEFT, 0), get_vtx(LEFT, 1)),
      Triangle::new(get_vtx(RIGHT, 2), get_vtx(RIGHT, 1), get_vtx(LEFT, 1)),
      Triangle::new(get_vtx(RIGHT, 2), get_vtx(LEFT, 1), get_vtx(LEFT, 2)),
    ],
    // _ => unreachable!(),
    _ => return,
  };

  for tri in tris {
    if !tri.is_degenerate() {
      triangles.push(tri.clone());
    }
  }
}

fn is_between(a: f32, b: f32, x: f32) -> bool {
  let (min, max) = if a < b { (a, b) } else { (b, a) };
  x > min && x < max
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

  let mut hex_heights = Vec::with_capacity(x_count * y_count);
  let mut void_flags = vec![bitarr![0; 1024]; y_count];

  let get_hex_center_coords = |y_ix: usize, x_ix: usize| -> (f32, f32) {
    let hex_center_x = x_ix as f32 * 1.5 * hex_width;
    let hex_center_z = y_ix as f32 * 2.0 * hex_height + if x_ix % 2 == 0 { 0. } else { hex_height };
    (hex_center_x, hex_center_z)
  };

  for y in 0..y_count {
    for x in 0..x_count {
      let (hex_center_x, hex_center_z) = get_hex_center_coords(y, x);
      let hex_height = get_hex_height(hex_center_x, hex_center_z);
      hex_heights.push(hex_height);

      if hex_height < -10. {
        void_flags[y].set(x, true);
      }

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

      let bottom_right_ix = if x % 2 == 0 {
        // x+1
        if x < x_count - 1 {
          ix + 1
        } else {
          usize::MAX
        }
      } else {
        // x+1, y+1
        if x < x_count - 1 && y < y_count - 1 {
          ix + x_count + 1
        } else {
          usize::MAX
        }
      };
      let bottom_ix = if y < y_count - 1 {
        // y+1
        ix + x_count
      } else {
        usize::MAX
      };
      let bottom_left_ix = if x % 2 == 0 {
        // x-1
        if x > 0 {
          ix - 1
        } else {
          usize::MAX
        }
      } else {
        // x-1, y+1
        if x > 0 && y < y_count - 1 {
          ix + x_count - 1
        } else {
          usize::MAX
        }
      };
      let top_left_ix = if x % 2 == 0 {
        // x-1, y-1
        if x > 0 && y > 0 {
          ix - x_count - 1
        } else {
          usize::MAX
        }
      } else {
        // x-1
        if x > 0 {
          ix - 1
        } else {
          usize::MAX
        }
      };
      let top_right_ix = if x % 2 == 0 {
        // x+1, y-1
        if x < x_count - 1 && y > 0 {
          ix - x_count + 1
        } else {
          usize::MAX
        }
      } else {
        // x+1
        if x < x_count - 1 {
          ix + 1
        } else {
          usize::MAX
        }
      };

      if let Some(bottom_right_height) = hex_heights.get(bottom_right_ix) {
        let mut split_heights = [None; 2];
        if let Some(top_right_height) = hex_heights.get(top_right_ix) {
          if is_between(hex_heights[ix], *bottom_right_height, *top_right_height) {
            split_heights[0] = Some(*top_right_height);
          }
        }
        if let Some(bottom_height) = hex_heights.get(bottom_ix) {
          if is_between(hex_heights[ix], *bottom_right_height, *bottom_height) {
            split_heights[1] = Some(*bottom_height);
          }
        }

        let bottom_right_hex_triangles = &triangles[bottom_right_ix * 6..bottom_right_ix * 6 + 6];
        connect_hexes(
          &mut new_triangles,
          base_hex_triangles,
          bottom_right_hex_triangles,
          [0, 1],
          [hex_heights[ix], *bottom_right_height],
          split_heights,
        );
      }

      if let Some(bottom_height) = hex_heights.get(bottom_ix) {
        let mut split_heights = [None; 2];
        if let Some(bottom_left_height) = hex_heights.get(bottom_left_ix) {
          if is_between(hex_heights[ix], *bottom_height, *bottom_left_height) {
            split_heights[1] = Some(*bottom_left_height);
          }
        }
        if let Some(bottom_right_height) = hex_heights.get(bottom_right_ix) {
          if is_between(hex_heights[ix], *bottom_height, *bottom_right_height) {
            split_heights[0] = Some(*bottom_right_height);
          }
        }

        let top_right_hex_triangles = &triangles[bottom_ix * 6..bottom_ix * 6 + 6];
        connect_hexes(
          &mut new_triangles,
          base_hex_triangles,
          top_right_hex_triangles,
          [1, 2],
          [hex_heights[ix], *bottom_height],
          split_heights,
        );
      }

      if let Some(bottom_left_height) = hex_heights.get(bottom_left_ix) {
        let mut split_heights = [None; 2];
        if let Some(bottom_height) = hex_heights.get(bottom_ix) {
          if is_between(hex_heights[ix], *bottom_left_height, *bottom_height) {
            split_heights[0] = Some(*bottom_height);
          }
        }
        if let Some(top_left_height) = hex_heights.get(top_left_ix) {
          if is_between(hex_heights[ix], *bottom_left_height, *top_left_height) {
            split_heights[1] = Some(*top_left_height);
          }
        }

        let bottom_left_hex_triangles = &triangles[bottom_left_ix * 6..bottom_left_ix * 6 + 6];
        connect_hexes(
          &mut new_triangles,
          base_hex_triangles,
          bottom_left_hex_triangles,
          [2, 3],
          [hex_heights[ix], *bottom_left_height],
          split_heights,
        );
      }

      triangles.extend(new_triangles.drain(..));
    }
  }

  let triangles_per_hex = 6;
  let mut tri_ix = 0;
  triangles.retain(|_| {
    let hex_ix = tri_ix / triangles_per_hex;
    if hex_ix >= x_count * y_count {
      return true;
    }

    let y_ix = hex_ix / x_count;
    let x_ix = hex_ix % x_count;
    let should_void = void_flags[y_ix].get(x_ix).unwrap();
    let retain = !should_void;

    tri_ix += 1;
    retain
  });

  triangles
}

pub struct GenBasaltCtx {
  pub collission_mesh: OwnedIndexedMesh,
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

fn displace_mesh(noise: &Fbm<f32>, mesh: &mut LinkedMesh) {
  for (_vtx_key, vtx) in &mut mesh.vertices {
    let Some(displacement_normal) = vtx.displacement_normal else {
      continue;
    };
    if displacement_normal.x.is_nan()
      || displacement_normal.y.is_nan()
      || displacement_normal.z.is_nan()
    {
      log::warn!("Displacement normal is NaN; skipping displace");
      continue;
    }

    let pos_scale = 0.027;
    let pos = vtx.position * pos_scale;
    let noise = noise.get([pos.x, pos.y, pos.z]); //.abs();
    let mut noise_scale = 1.36;
    // tone down distortion near the bottoms of the pillars
    noise_scale -= (1. - smoothstep(-20., 20., vtx.position.y)) * 1.;
    vtx.position += Vector3::repeat(noise * noise_scale);
  }
}

#[wasm_bindgen]
pub fn basalt_gen() -> *mut GenBasaltCtx {
  maybe_init();

  let seed = 393939939;
  let coarse_noise: Fbm<f32> = Fbm::new().set_octaves(1).set_seed(seed);
  let noise_gen: Fbm<f32> = Fbm::new().set_octaves(2).set_seed(seed);
  let get_height = |x, z| {
    let void = coarse_noise.get([x * 0.036, z * 0.036]) < 0.2;
    if void {
      return -50.;
    }
    let mut height = (noise_gen.get([x * 0.03, z * 0.03]) + 1.) * 24.;
    if random() < 0.7 {
      height =
        round_to_nearest_multiple(height, 11.) + if random() > 0.6 { random() * 1.2 } else { 0. };
    }
    height
  };
  let terrain_triangles = gen_tessellated_hex_grid(20, 20, 11., get_height);

  let mut mesh = LinkedMesh::from_triangles(&terrain_triangles);
  let merged_count = mesh.merge_vertices_by_distance(0.0001);
  info!(
    "Merged {merged_count} vertices by distance; {} remaining",
    mesh.vertices.len()
  );

  let sharp_edge_threshold_rads = 0.8;
  mesh.mark_edge_sharpness(sharp_edge_threshold_rads);
  mesh.compute_vertex_displacement_normals();

  let mut collission_mesh = mesh.clone();
  let displ_noise = noise::Fbm::new().set_octaves(3);
  displace_mesh(&displ_noise, &mut collission_mesh);
  let collission_mesh = mesh.to_raw_indexed();

  let target_edge_length = 3.16;
  let should_split_edge = |mesh: &LinkedMesh, edge: &Edge| -> bool {
    // avoid splitting edges that are low down to save resources since they will be
    // far from the player and not easily visible
    let [v0_y, v1_y] = [
      mesh.vertices[edge.vertices[0]].position.y,
      mesh.vertices[edge.vertices[1]].position.y,
    ];
    if v0_y < -15. && v1_y < -15. {
      return false;
    } else if v0_y < 0. || v1_y < 0. {
      return edge.length(&mesh.vertices) > 8.0;
    }

    let length = edge.length(&mesh.vertices);
    let split_length = length / 2.;
    // if the post-split length would be closer to the target length than the
    // current length, then we need to split this edge
    (split_length - target_edge_length).abs() < (length - target_edge_length).abs()
  };
  tessellation::tessellate_mesh_cb(
    &mut mesh,
    DisplacementNormalMethod::Interpolate,
    &should_split_edge,
  );
  info!("Tessellated mesh; new vertex count={}", mesh.vertices.len());

  displace_mesh(&displ_noise, &mut mesh);

  mesh.mark_edge_sharpness(sharp_edge_threshold_rads);
  // mesh.compute_edge_displacement_normals();
  mesh.separate_vertices_and_compute_normals();
  info!(
    "Separated vertices; new vertex count={}",
    mesh.vertices.len()
  );

  Box::into_raw(Box::new(GenBasaltCtx {
    collission_mesh,
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
pub fn basalt_take_displacement_normals(ctx: *mut GenBasaltCtx) -> Vec<f32> {
  let ctx = unsafe { &mut (*ctx) };
  std::mem::take(
    &mut ctx
      .terrrain_mesh
      .displacement_normals
      .as_mut()
      .expect("Displacement normals not found"),
  )
}

#[wasm_bindgen]
pub fn basalt_take_collision_vertices(ctx: *mut GenBasaltCtx) -> Vec<f32> {
  let ctx = unsafe { &mut (*ctx) };
  std::mem::take(&mut ctx.collission_mesh.vertices)
}

#[wasm_bindgen]
pub fn basalt_take_collision_indices(ctx: *mut GenBasaltCtx) -> Vec<usize> {
  let ctx = unsafe { &mut (*ctx) };
  std::mem::take(&mut ctx.collission_mesh.indices)
}

#[wasm_bindgen]
pub fn basalt_free(ctx: *mut GenBasaltCtx) {
  drop(unsafe { Box::from_raw(ctx) });
}
