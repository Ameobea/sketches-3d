use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use bitvec::prelude::*;
use fxhash::FxHashMap;
use mesh::{
  linked_mesh::{Mat4, Vec3},
  LinkedMesh,
};

use crate::{materials::Material, ErrorStack, ManifoldHandle, MeshHandle};

/// Returns a buffer that can be indexed by [z * dims[0] * dims[1] + y * dims[0] + x].  Each
/// voxel is represented by a byte where 0 is empty and numbers > 1 mean it is filled with material
/// index `n - 1`.
///
/// TODO: would be ideal to detect if the function is pure and parallelize if so.
fn sample_voxel_matrix(
  dims: [usize; 3],
  cb: impl Fn(usize, usize, usize) -> Result<u8, ErrorStack>,
) -> Result<(u8, Vec<u8>), ErrorStack> {
  let mut buf = Vec::with_capacity(dims[0] * dims[1] * dims[2]);
  unsafe {
    buf.set_len(dims[0] * dims[1] * dims[2]);
  }
  let mut max_material_ix = 0u8;

  for z in 0..dims[2] {
    for y in 0..dims[1] {
      for x in 0..dims[0] {
        let value = cb(x, y, z)?;
        if value > max_material_ix {
          max_material_ix = value;
        }
        buf[z * dims[0] * dims[1] + y * dims[0] + x] = value;
      }
    }
  }

  Ok((max_material_ix, buf))
}

pub fn sample_voxels(
  dims: [usize; 3],
  cb: impl Fn(usize, usize, usize) -> Result<u8, ErrorStack>,
  fill_internal_voids: bool,
  materials: Vec<Rc<Material>>,
  use_cgal_remeshing: Option<bool>,
) -> Result<Vec<MeshHandle>, ErrorStack> {
  #[cfg(target_arch = "wasm32")]
  let mut did_verify_cgal_loaded = false;
  #[cfg(target_arch = "wasm32")]
  if use_cgal_remeshing == Some(true) {
    did_verify_cgal_loaded = true;
    super::mesh_ops::verify_cgal_loaded()?;
  }

  let (max_material_ix, mut voxel_buf) = sample_voxel_matrix(dims, cb)?;

  if max_material_ix == 0 {
    return Ok(Vec::new());
  }

  if fill_internal_voids {
    fill_voids(dims, &mut voxel_buf);
  }

  // each unique material produces its own mesh, and value `n` indicates material index `n-1`
  let mut mesh_builders: Vec<VoxelMeshBuilder> = (0..max_material_ix)
    .map(|i| VoxelMeshBuilder::new(dims, &voxel_buf, i + 1))
    .collect();

  let get_voxel = |x: isize, y: isize, z: isize| -> u8 {
    if x < 0 || y < 0 || z < 0 {
      return 0;
    }
    let (ux, uy, uz) = (x as usize, y as usize, z as usize);
    if ux >= dims[0] || uy >= dims[1] || uz >= dims[2] {
      return 0;
    }
    voxel_buf[uz * dims[0] * dims[1] + uy * dims[0] + ux]
  };

  for z in 0..dims[2] {
    for y in 0..dims[1] {
      for x in 0..dims[0] {
        let current_mat = voxel_buf[z * dims[0] * dims[1] + y * dims[0] + x];
        if current_mat == 0 {
          continue;
        }

        let neighbors = [
          [-1, 0, 0],
          [1, 0, 0],
          [0, -1, 0],
          [0, 1, 0],
          [0, 0, -1],
          [0, 0, 1],
        ];

        for [nx, ny, nz] in neighbors {
          let neighbor_mat = get_voxel(x as isize + nx, y as isize + ny, z as isize + nz);

          // if the neighbor voxel is different material, this is a border and we emit a face
          if neighbor_mat != current_mat {
            let builder = &mut mesh_builders[(current_mat - 1) as usize];
            emit_face(builder, x as u32, y as u32, z as u32, [nx, ny, nz]);
          }
        }
      }
    }
  }

  let mut out_meshes: Vec<MeshHandle> = Vec::with_capacity(max_material_ix as usize);

  for (i, builder) in mesh_builders.into_iter().enumerate() {
    let (indices, verts) = builder.finish();

    if indices.is_empty() {
      continue;
    }

    assert_eq!(verts.len() % 3, 0);
    let verts_slice: &[Vec3] =
      unsafe { std::slice::from_raw_parts(verts.as_ptr() as *const Vec3, verts.len() / 3) };

    let use_cgal_remeshing = use_cgal_remeshing.unwrap_or_else(|| verts.len() < 100_000);

    if use_cgal_remeshing {
      #[cfg(target_arch = "wasm32")]
      {
        if !did_verify_cgal_loaded {
          super::mesh_ops::verify_cgal_loaded()?;
          did_verify_cgal_loaded = true;
        }

        super::mesh_ops::cgal_remesh_planar_patches(&verts, &indices, 1., 0.1);

        let mat = materials.get(i).cloned();
        let out_mesh = super::mesh_ops::read_cgal_output_mesh(Mat4::identity(), mat);
        out_meshes.push(out_mesh);
      }

      #[cfg(not(target_arch = "wasm32"))]
      return Err(ErrorStack::new_uninitialized_module(
        "cgal remeshing not supported outside of wasm",
      ));
    } else {
      // TODO: it would be good to do some light-weight optimization here of our own for cases where
      // the mesh is too big for CGAL to handle effectively.
      //
      // There should be some ways to handle obvious cases and reduce vtx/face counts - especially
      // for simple cases.

      let mesh = LinkedMesh::from_indexed_vertices(verts_slice, &indices, None, None);
      let mesh = MeshHandle {
        mesh: Rc::new(mesh),
        transform: Mat4::identity(),
        manifold_handle: Rc::new(ManifoldHandle::new_empty()),
        aabb: RefCell::new(None),
        trimesh: RefCell::new(None),
        material: materials.get(i).cloned(),
      };
      out_meshes.push(mesh);
    }
  }

  Ok(out_meshes)
}

struct VoxelMeshBuilder<'a> {
  verts: Vec<f32>,
  indices: Vec<u32>,
  // `(grid_x, grid_y, grid_z, component_id) -> vtx_ix``
  vert_lookup: FxHashMap<(u32, u32, u32, usize), u32>,
  dims: [usize; 3],
  voxel_buf: &'a [u8],
  target_material: u8,
}

impl<'a> VoxelMeshBuilder<'a> {
  fn new(dims: [usize; 3], voxel_buf: &'a [u8], target_material: u8) -> Self {
    Self {
      verts: Vec::new(),
      indices: Vec::new(),
      vert_lookup: FxHashMap::default(),
      dims,
      voxel_buf,
      target_material,
    }
  }

  /// Determines which "manifold component" the source voxel belongs to at this corner.
  /// Returns a canonical ID (the smallest linear index of a voxel in the component).
  fn get_component_id(
    &self,
    gx: u32,
    gy: u32,
    gz: u32,
    src_vx: u32,
    src_vy: u32,
    src_vz: u32,
  ) -> usize {
    let [w, h, d] = self.dims;

    let mut active_voxels = smallvec::SmallVec::<[(u32, u32, u32); 8]>::new();

    for z in gz.saturating_sub(1)..=gz {
      for y in gy.saturating_sub(1)..=gy {
        for x in gx.saturating_sub(1)..=gx {
          if x >= w as u32 || y >= h as u32 || z >= d as u32 {
            continue;
          }

          let idx = (z as usize * w * h) + (y as usize * w) + (x as usize);
          if self.voxel_buf[idx] == self.target_material {
            active_voxels.push((x, y, z));
          }
        }
      }
    }

    if active_voxels.len() <= 1 {
      return (src_vz as usize * w * h) + (src_vy as usize * w) + (src_vx as usize);
    }

    let src_tuple = (src_vx, src_vy, src_vz);
    let mut stack = smallvec::SmallVec::<[(u32, u32, u32); 8]>::new();
    stack.push(src_tuple);

    let mut component_members = smallvec::SmallVec::<[(u32, u32, u32); 8]>::new();

    while let Some(current) = stack.pop() {
      if component_members.contains(&current) {
        continue;
      }
      component_members.push(current);

      for &candidate in &active_voxels {
        if component_members.contains(&candidate) {
          continue;
        }

        let dist = (candidate.0 as i32 - current.0 as i32).abs()
          + (candidate.1 as i32 - current.1 as i32).abs()
          + (candidate.2 as i32 - current.2 as i32).abs();

        if dist == 1 {
          stack.push(candidate);
        }
      }
    }

    let mut min_idx = usize::MAX;
    for (vx, vy, vz) in component_members {
      let idx = (vz as usize * w * h) + (vy as usize * w) + (vx as usize);
      if idx < min_idx {
        min_idx = idx;
      }
    }
    min_idx
  }

  fn get_or_insert_vert(
    &mut self,
    gx: u32,
    gy: u32,
    gz: u32,
    src_x: u32,
    src_y: u32,
    src_z: u32,
  ) -> u32 {
    // this is used to distinguish vertices at the same position but belonging to different
    // manifold components.  This prevents bowties which break manifoldness.
    let comp_id = self.get_component_id(gx, gy, gz, src_x, src_y, src_z);

    let key = (gx, gy, gz, comp_id);

    if let Some(&idx) = self.vert_lookup.get(&key) {
      return idx;
    }

    let idx = (self.verts.len() / 3) as u32;
    self.verts.push(gx as f32);
    self.verts.push(gy as f32);
    self.verts.push(gz as f32);
    self.vert_lookup.insert(key, idx);
    idx
  }

  fn add_quad(
    &mut self,
    v1: (u32, u32, u32),
    v2: (u32, u32, u32),
    v3: (u32, u32, u32),
    v4: (u32, u32, u32),
    src_x: u32,
    src_y: u32,
    src_z: u32,
  ) {
    let i1 = self.get_or_insert_vert(v1.0, v1.1, v1.2, src_x, src_y, src_z);
    let i2 = self.get_or_insert_vert(v2.0, v2.1, v2.2, src_x, src_y, src_z);
    let i3 = self.get_or_insert_vert(v3.0, v3.1, v3.2, src_x, src_y, src_z);
    let i4 = self.get_or_insert_vert(v4.0, v4.1, v4.2, src_x, src_y, src_z);

    self.indices.push(i1);
    self.indices.push(i2);
    self.indices.push(i3);
    self.indices.push(i1);
    self.indices.push(i3);
    self.indices.push(i4);
  }

  fn finish(self) -> (Vec<u32>, Vec<f32>) {
    (self.indices, self.verts)
  }
}

/// Emits a quad for the face of voxel (x, y, z) facing direction (nx, ny, nz).
fn emit_face(builder: &mut VoxelMeshBuilder, x: u32, y: u32, z: u32, [nx, ny, nz]: [isize; 3]) {
  let [v1, v2, v3, v4] = match (nx, ny, nz) {
    // left
    (-1, 0, 0) => [(x, y, z), (x, y, z + 1), (x, y + 1, z + 1), (x, y + 1, z)],
    // right
    (1, 0, 0) => [
      (x + 1, y, z),
      (x + 1, y + 1, z),
      (x + 1, y + 1, z + 1),
      (x + 1, y, z + 1),
    ],
    // bottom
    (0, -1, 0) => [(x, y, z), (x + 1, y, z), (x + 1, y, z + 1), (x, y, z + 1)],
    // top
    (0, 1, 0) => [
      (x, y + 1, z),
      (x, y + 1, z + 1),
      (x + 1, y + 1, z + 1),
      (x + 1, y + 1, z),
    ],
    // front
    (0, 0, -1) => [(x, y, z), (x, y + 1, z), (x + 1, y + 1, z), (x + 1, y, z)],
    // back
    (0, 0, 1) => [
      (x, y, z + 1),
      (x + 1, y, z + 1),
      (x + 1, y + 1, z + 1),
      (x, y + 1, z + 1),
    ],
    _ => unreachable!(),
  };

  builder.add_quad(v1, v2, v3, v4, x, y, z);
}

fn fill_voids([w, h, d]: [usize; 3], buf: &mut Vec<u8>) {
  let mut outside_mask = bitvec![usize, Lsb0; 0; w * h * d];
  let mut queue = VecDeque::new();

  // seed the queue with all boundary voxels that are empty (0)
  for z in 0..d {
    for y in 0..h {
      for x in 0..w {
        if x == 0 || x == w - 1 || y == 0 || y == h - 1 || z == 0 || z == d - 1 {
          let idx = z * w * h + y * w + x;
          if buf[idx] == 0 {
            outside_mask.set(idx, true);
            queue.push_back((x, y, z));
          }
        }
      }
    }
  }

  // BFS flood fill all empty voxels reachable from the outside
  while let Some((cx, cy, cz)) = queue.pop_front() {
    let neighbors = [
      (cx.wrapping_sub(1), cy, cz),
      (cx + 1, cy, cz),
      (cx, cy.wrapping_sub(1), cz),
      (cx, cy + 1, cz),
      (cx, cy, cz.wrapping_sub(1)),
      (cx, cy, cz + 1),
    ];

    for (nx, ny, nz) in neighbors {
      if nx >= w || ny >= h || nz >= d {
        continue;
      }
      let idx = nz * w * h + ny * w + nx;

      if buf[idx] == 0 && !outside_mask[idx] {
        outside_mask.set(idx, true);
        queue.push_back((nx, ny, nz));
      }
    }
  }

  // any empty voxels not marked in `outside_mask`` are internal voids
  //
  // Each internal void component is flood filled and assigned a material arbitrarily chosen
  // from its bordering solid voxels.
  let mut component_indices = Vec::new();

  for i in 0..buf.len() {
    if buf[i] == 0 && !outside_mask[i] {
      outside_mask.set(i, true);

      let z = i / (w * h);
      let rem = i % (w * h);
      let y = rem / w;
      let x = rem % w;

      queue.push_back((x, y, z));
      component_indices.clear();

      let mut found_material = None;

      while let Some((cx, cy, cz)) = queue.pop_front() {
        let c_idx = cz * w * h + cy * w + cx;
        component_indices.push(c_idx);

        let neighbors = [
          (cx.wrapping_sub(1), cy, cz),
          (cx + 1, cy, cz),
          (cx, cy.wrapping_sub(1), cz),
          (cx, cy + 1, cz),
          (cx, cy, cz.wrapping_sub(1)),
          (cx, cy, cz + 1),
        ];

        for (nx, ny, nz) in neighbors {
          if nx >= w || ny >= h || nz >= d {
            continue;
          }
          let n_idx = nz * w * h + ny * w + nx;

          if buf[n_idx] != 0 {
            if found_material.is_none() {
              found_material = Some(buf[n_idx]);
            }
          } else if !outside_mask[n_idx] {
            outside_mask.set(n_idx, true);
            queue.push_back((nx, ny, nz));
          }
        }
      }

      let fill_mat = found_material.unwrap_or(1);
      for idx in &component_indices {
        buf[*idx] = fill_mat;
      }
    }
  }
}

#[test]
fn test_diagonally_touching_manifoldness() {
  let cb = |x: usize, y: usize, z: usize| -> Result<u8, ErrorStack> {
    if (x == 0 && y == 0 && z == 0) || (x == 1 && y == 1 && z == 1) {
      Ok(1)
    } else {
      Ok(0)
    }
  };

  let meshes = sample_voxels([3, 3, 3], cb, false, Vec::new(), Some(false)).unwrap();
  assert_eq!(meshes.len(), 1);
  let mesh = &meshes[0];

  // the vertex at which the two voxels touch shouldn't be shared, but rather duplicated
  assert_eq!(mesh.mesh.vertices.len(), 16);
}
