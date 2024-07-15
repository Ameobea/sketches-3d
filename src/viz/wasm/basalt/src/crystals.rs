use common::{random, rng};
use mesh::{LinkedMesh, Mesh, OwnedIndexedMesh, Triangle};
use nalgebra::{Matrix4, Rotation3, Vector3};
use point_distribute::MeshSurfaceSampler;
use rand::prelude::*;

const UNIQ_CRYSTAL_MESH_COUNT: usize = 8;
const TOTAL_CRYSTALS_TO_GENERATE: usize = 258;

fn generate_crystal_mesh() -> LinkedMesh {
  // placeholder for now - rectangular prism with random height pointed straight
  // up from origin
  let width = 0.2f32;
  let height = rng().gen_range(2.5f32..10.5);

  let tris = &[
    Triangle::new(
      Vector3::new(-width, 0.0, -width),
      Vector3::new(width, 0.0, -width),
      Vector3::new(width, 0.0, width),
    ),
    Triangle::new(
      Vector3::new(-width, 0.0, -width),
      Vector3::new(width, 0.0, width),
      Vector3::new(-width, 0.0, width),
    ),
    Triangle::new(
      Vector3::new(-width, 0.0, -width),
      Vector3::new(-width, height, -width),
      Vector3::new(width, 0.0, -width),
    ),
    Triangle::new(
      Vector3::new(-width, height, -width),
      Vector3::new(width, height, -width),
      Vector3::new(width, 0.0, -width),
    ),
    Triangle::new(
      Vector3::new(width, 0.0, -width),
      Vector3::new(width, height, -width),
      Vector3::new(width, 0.0, width),
    ),
    Triangle::new(
      Vector3::new(width, height, -width),
      Vector3::new(width, height, width),
      Vector3::new(width, 0.0, width),
    ),
    Triangle::new(
      Vector3::new(width, 0.0, width),
      Vector3::new(width, height, width),
      Vector3::new(-width, 0.0, width),
    ),
    Triangle::new(
      Vector3::new(width, height, width),
      Vector3::new(-width, height, width),
      Vector3::new(-width, 0.0, width),
    ),
    Triangle::new(
      Vector3::new(-width, 0.0, width),
      Vector3::new(-width, height, width),
      Vector3::new(-width, 0.0, -width),
    ),
    Triangle::new(
      Vector3::new(-width, height, width),
      Vector3::new(-width, height, -width),
      Vector3::new(-width, 0.0, -width),
    ),
    Triangle::new(
      Vector3::new(-width, height, -width),
      Vector3::new(-width, height, width),
      Vector3::new(width, height, -width),
    ),
    Triangle::new(
      Vector3::new(-width, height, width),
      Vector3::new(width, height, width),
      Vector3::new(width, height, -width),
    ),
  ];
  let mut mesh = LinkedMesh::from_triangles(tris);

  mesh.merge_vertices_by_distance(0.000001);
  let sharp_edge_threshold_rads = 0.8;
  mesh.mark_edge_sharpness(sharp_edge_threshold_rads);
  mesh.compute_vertex_displacement_normals();

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
  for _ in 0..TOTAL_CRYSTALS_TO_GENERATE {
    let (point, normal) = loop {
      let (point, normal) = samp.sample();

      // TODO: will have to be more sophisticated here.  will probably end up using
      // noise to make crystals more and less likely to spawn in certain areas
      if point.y < 0. {
        let roll = random();
        if roll < 0.5 {
          continue;
        }

        if point.y < -30. && roll < 0.8 {
          continue;
        }
      }

      break (point, normal);
    };

    let crystal = crystal_meshes.choose_mut(&mut rng).unwrap();
    let mut transform: Matrix4<f32> = Matrix4::identity();
    let mut pos = point.coords;
    // inset into the mesh slightly, moving backwards along the normal
    pos -= normal * 0.1;
    transform *= Matrix4::new_translation(&pos);
    // crystals are generated pointing up; rotate to align with normal
    let rotation = Rotation3::rotation_between(&Vector3::y_axis(), &normal).unwrap();
    transform *= rotation.to_homogeneous();

    crystal.transforms.push(transform);
  }

  crystal_meshes
}
