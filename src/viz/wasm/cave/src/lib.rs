use common::rng;
use nalgebra::{Matrix4, Point2, Point3, Vector3};
use noise::{NoiseModule, Perlin, Seedable};
use point_distribute::{Mesh, MeshSurfaceSampler};
use rand::Rng;
use wasm_bindgen::prelude::*;

const STALAG_COUNT: usize = 300;

struct Stalags {
  positions: Vec<Point3<f32>>,
  scales: Vec<Point3<f32>>,
  euler_angles: Vec<Vector3<f32>>,
}

static mut STALAGS: *mut Stalags = std::ptr::null_mut();

/// Transform is expected to be provided as a 4x4 matrix in column-major order
/// or empty.
#[wasm_bindgen]
pub fn compute_stalags(cave_mesh_vertices: &[f32], cave_mesh_normals: &[f32], transform: &[f32]) {
  std::panic::set_hook(Box::new(console_error_panic_hook::hook));

  if cave_mesh_vertices.len() % 3 != 0 {
    panic!("cave_mesh_vertices.len() must be a multiple of 3");
  }
  if cave_mesh_normals.len() % 3 != 0 {
    panic!("cave_mesh_normals.len() must be a multiple of 3");
  }
  let cave_mesh_vertices: &[Vector3<f32>] = unsafe {
    std::slice::from_raw_parts(
      cave_mesh_vertices.as_ptr() as *const Vector3<f32>,
      cave_mesh_vertices.len() / 3,
    )
  };
  let cave_mesh_normals: &[Vector3<f32>] = unsafe {
    std::slice::from_raw_parts(
      cave_mesh_normals.as_ptr() as *const Vector3<f32>,
      cave_mesh_normals.len() / 3,
    )
  };
  let mesh = Mesh {
    vertices: cave_mesh_vertices,
    normals: Some(cave_mesh_normals),
    transform: if transform.len() == 0 {
      None
    } else if transform.len() == 16 {
      let mat4 = Matrix4::from_column_slice(transform);
      // panic!(
      //   "translate: {:?}",
      //   mat4.transform_point(&Point3::new(0., 0., 0.))
      // );
      Some(mat4)
    } else {
      panic!(
        "transform must either be empty or a matrix4; got {} elements",
        transform.len()
      );
    },
  };
  let surface_samp = MeshSurfaceSampler::new(mesh);

  // We want the stalags to generate in small pockets, so we filter the generated
  // samples using a noise function.
  let noise_samp = Perlin::new().set_seed(rng().gen::<usize>() + 1);

  let mut iters = 0usize;
  let mut points: Vec<Point3<f32>> = Vec::with_capacity(STALAG_COUNT);
  let mut scales: Vec<Point3<f32>> = Vec::with_capacity(STALAG_COUNT);
  let mut euler_angles: Vec<Vector3<f32>> = Vec::with_capacity(STALAG_COUNT * 3);
  while points.len() < STALAG_COUNT {
    iters += 1;
    if iters > 1_000_000 {
      panic!(
        "too many iterations; found {} valid positions so far",
        points.len()
      );
    }

    let (pos, normal) = surface_samp.sample();
    let noise = noise_samp.get([pos.x * 2., pos.y * 2.]);
    if noise < 0.93 {
      continue;
    }

    // If the surface sampled is too close to being vertical, we don't want to place
    // a stalagmite there.
    if normal.y.abs() < 0.2 {
      continue;
    }
    // We also don't want to generate them if the surface is too flat to try to
    // avoid placing them in the middle of the cave.
    if normal.y.abs() > 0.95 {
      continue;
    }

    // fine-tuning to unblock some passages
    if pos.x < 0.
      || nalgebra::distance(&Point2::new(175., -30.), &Point2::new(pos.x, pos.z)) < 4.
      || nalgebra::distance(&Point2::new(20., 2.), &Point2::new(pos.x, pos.z)) < 2.
    {
      continue;
    }

    points.push(pos);
    // We want to align the make them either mites or tites depending on the
    // normal of the surface they're being placed on.
    //
    // So if the normal is pointing more downwards than upwards, we want to
    // rotate the stalagmite 180 degrees around the x axis.
    let mut euler_angle = Vector3::zeros();
    if normal.y < 0.0 {
      euler_angle.x = std::f32::consts::PI;
    }

    // randomize the rotation about the y axis
    euler_angle.y = rng().gen_range(0.0..std::f32::consts::PI * 2.0);

    scales.push(Point3::new(
      rng().gen_range(1.8..3.0),
      rng().gen_range(0.5..2.0),
      rng().gen_range(1.8..3.0),
    ));

    euler_angles.push(euler_angle);
  }

  let stalags = Stalags {
    positions: points,
    scales,
    euler_angles,
  };

  unsafe {
    if !STALAGS.is_null() {
      let _ = Box::from_raw(STALAGS);
    }
    STALAGS = Box::into_raw(Box::new(stalags));
  }
}

#[wasm_bindgen]
pub fn stalag_count() -> usize {
  unsafe {
    let stalags = &*STALAGS;
    stalags.positions.len()
  }
}

#[wasm_bindgen]
pub fn get_stalag_positions() -> *const f32 {
  unsafe {
    let stalags = &*STALAGS;
    stalags.positions.as_ptr() as *const f32
  }
}

#[wasm_bindgen]
pub fn get_stalag_scales() -> *const f32 {
  unsafe {
    let stalags = &*STALAGS;
    stalags.scales.as_ptr() as *const f32
  }
}

#[wasm_bindgen]
pub fn get_stalag_euler_angles() -> *const f32 {
  unsafe {
    let stalags = &*STALAGS;
    stalags.euler_angles.as_ptr() as *const f32
  }
}

#[wasm_bindgen]
pub fn free_stalags() {
  unsafe {
    if !STALAGS.is_null() {
      let _ = Box::from_raw(STALAGS);
      STALAGS = std::ptr::null_mut();
    }
  }
}
