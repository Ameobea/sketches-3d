use common::{random, rng, uninit};
use mesh::{LinkedMesh, Mesh, OwnedIndexedMesh, Triangle};
use nalgebra::{Matrix4, Rotation3, Vector3};
use noise::{Fbm, MultiFractal, NoiseModule, Seedable};
use point_distribute::MeshSurfaceSampler;
use rand::prelude::*;

use crate::gen_hex_triangles;

const UNIQ_CRYSTAL_MESH_COUNT: usize = 8;
const TOTAL_CRYSTALS_TO_GENERATE: usize = 258;

fn generate_crystal_mesh() -> LinkedMesh {
  // hexagonal prism with random height pointed straight up from origin
  let hex_width = rng().gen_range(1.5f32..3.3f32);
  let extrude_height = rng().gen_range(2.5f32..12.5);

  let mut tris = Vec::new();
  let mut base_tris: [Triangle; 6] = uninit();
  let mut top_tris: [Triangle; 6] = uninit();

  let top_scale = rng().gen_range(0.4f32..0.94);
  for (i, tri) in gen_hex_triangles([0., 0.], hex_width, 0.).enumerate() {
    top_tris[i] = Triangle::new(
      tri.a * top_scale + Vector3::new(0., extrude_height, 0.),
      tri.b * top_scale + Vector3::new(0., extrude_height, 0.),
      tri.c * top_scale + Vector3::new(0., extrude_height, 0.),
    );
    base_tris[i] = Triangle::new(tri.c, tri.b, tri.a);
  }
  tris.extend_from_slice(&base_tris);
  tris.extend_from_slice(&top_tris);

  // Add side faces
  for i in 0..6 {
    let t0_v0 = top_tris[i].b;
    let t0_v1 = top_tris[i].a;
    let t1_v0 = base_tris[i].b;
    let t1_v1 = base_tris[i].c;

    tris.push(Triangle::new(t0_v0, t0_v1, t1_v0));
    tris.push(Triangle::new(t0_v1, t1_v1, t1_v0));
  }

  let mut mesh = LinkedMesh::from_triangles(&tris);

  mesh.merge_vertices_by_distance(0.000001);
  let sharp_edge_threshold_rads = 0.8;
  mesh.mark_edge_sharpness(sharp_edge_threshold_rads);
  mesh.compute_vertex_displacement_normals();
  mesh.separate_vertices_and_compute_normals();

  mesh
}

pub(crate) struct BatchMesh {
  pub mesh: OwnedIndexedMesh,
  pub transforms: Vec<Matrix4<f32>>,
}

pub(crate) fn generate_crystals(pillars_mesh: &LinkedMesh) -> Vec<BatchMesh> {
  let mut crystal_meshes = (0..UNIQ_CRYSTAL_MESH_COUNT)
    .map(|_| BatchMesh {
      mesh: generate_crystal_mesh().to_raw_indexed(),
      transforms: Vec::new(),
    })
    .collect::<Vec<_>>();

  let mesh = pillars_mesh.to_owned_mesh(None);
  let mesh: Mesh = (&mesh).into();
  let samp = MeshSurfaceSampler::new(mesh);

  let mut rng = rng();
  let noise: Fbm<f32> = Fbm::new().set_octaves(2).set_seed(3993393993);
  for _ in 0..TOTAL_CRYSTALS_TO_GENERATE {
    let (point, normal) = loop {
      let (point, normal) = samp.sample();

      // we want to keep crystals mostly horizontal or mostly vertical, because
      // diagonal crystals are likely generating on a lip or edge and might clip
      // through geometry
      let normal_y_abs = normal.y.abs();
      if normal_y_abs < 0.84 && normal_y_abs > 0.24 {
        continue;
      }

      if point.y < 0. {
        let roll = random();
        if roll < 0.5 {
          continue;
        }

        if point.y < -30. && roll < 0.8 {
          continue;
        }
      }

      // encourage crystals to generate in clusters rather than evenly distributed
      let coarse_noise_val = noise.get([point.x * 0.01, point.y * 0.01, point.z * 0.01]);
      if coarse_noise_val < 0.2 {
        continue;
      }
      let fine_noise_val = noise.get([point.x * 0.04, point.y * 0.04, point.z * 0.04]);
      if fine_noise_val < 0.54 {
        continue;
      }

      break (point, normal);
    };

    let crystal = crystal_meshes.choose_mut(&mut rng).unwrap();
    let mut transform: Matrix4<f32> = Matrix4::identity();
    let mut pos = point.coords;
    // inset into the mesh slightly, moving backwards along the normal
    pos -= normal * 1.6;
    transform *= Matrix4::new_translation(&pos);
    // crystals are generated pointing up; rotate to align with normal
    let rotation = Rotation3::rotation_between(&Vector3::y_axis(), &normal).unwrap();
    transform *= rotation.to_homogeneous();

    crystal.transforms.push(transform);
  }

  crystal_meshes
}
