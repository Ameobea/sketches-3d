use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use bitvec::prelude::*;
use fxhash::FxHashMap;
use mesh::{
  linked_mesh::{Mat4, Vec3},
  LinkedMesh,
};
use smallvec::SmallVec;

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

enum FaceDir {
  Left,
  Right,
  Bottom,
  Top,
  Front,
  Back,
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
    .map(|_| VoxelMeshBuilder::new())
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
          (FaceDir::Left, [-1, 0, 0]),
          (FaceDir::Right, [1, 0, 0]),
          (FaceDir::Bottom, [0, -1, 0]),
          (FaceDir::Top, [0, 1, 0]),
          (FaceDir::Front, [0, 0, -1]),
          (FaceDir::Back, [0, 0, 1]),
        ];

        for (dir, [nx, ny, nz]) in neighbors {
          let neighbor_mat = get_voxel(x as isize + nx, y as isize + ny, z as isize + nz);

          // if the neighbor voxel is different material, this is a border and we emit a face
          if neighbor_mat != current_mat {
            let builder = &mut mesh_builders[(current_mat - 1) as usize];
            builder.emit_face(x as u32, y as u32, z as u32, dir);
          }
        }
      }
    }
  }

  let mut out_meshes: Vec<MeshHandle> = Vec::with_capacity(max_material_ix as usize);

  for (i, builder) in mesh_builders.into_iter().enumerate() {
    let (raw_indices, raw_verts) = builder.finish();

    if raw_indices.is_empty() {
      continue;
    }

    let (indices, verts) = fix_manifold(raw_indices, raw_verts);

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
        let out_mesh = super::mesh_ops::read_cgal_output_mesh(Mat4::identity(), mat)?;
        out_meshes.push(out_mesh);
      }

      #[cfg(not(target_arch = "wasm32"))]
      return Err(ErrorStack::new_uninitialized_module(
        "cgal remeshing not supported outside of wasm",
      ));
    } else {
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

struct VoxelMeshBuilder {
  verts: Vec<f32>,
  indices: Vec<u32>,
  /// (grid_x, grid_y, grid_z) -> vtx_ix
  vert_lookup: FxHashMap<(u32, u32, u32), u32>,
}

impl VoxelMeshBuilder {
  fn new() -> Self {
    Self {
      verts: Vec::new(),
      indices: Vec::new(),
      vert_lookup: FxHashMap::default(),
    }
  }

  fn get_or_insert_vert(&mut self, gx: u32, gy: u32, gz: u32) -> u32 {
    let key = (gx, gy, gz);
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
  ) {
    let i1 = self.get_or_insert_vert(v1.0, v1.1, v1.2);
    let i2 = self.get_or_insert_vert(v2.0, v2.1, v2.2);
    let i3 = self.get_or_insert_vert(v3.0, v3.1, v3.2);
    let i4 = self.get_or_insert_vert(v4.0, v4.1, v4.2);

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

  /// Emits a quad for the face of voxel (x, y, z) facing `dir`
  fn emit_face(&mut self, x: u32, y: u32, z: u32, dir: FaceDir) {
    let [v1, v2, v3, v4] = match dir {
      FaceDir::Left => [(x, y, z), (x, y, z + 1), (x, y + 1, z + 1), (x, y + 1, z)],
      FaceDir::Right => [
        (x + 1, y, z),
        (x + 1, y + 1, z),
        (x + 1, y + 1, z + 1),
        (x + 1, y, z + 1),
      ],
      FaceDir::Bottom => [(x, y, z), (x + 1, y, z), (x + 1, y, z + 1), (x, y, z + 1)],
      FaceDir::Top => [
        (x, y + 1, z),
        (x, y + 1, z + 1),
        (x + 1, y + 1, z + 1),
        (x + 1, y + 1, z),
      ],
      FaceDir::Front => [(x, y, z), (x, y + 1, z), (x + 1, y + 1, z), (x + 1, y, z)],
      FaceDir::Back => [
        (x, y, z + 1),
        (x + 1, y, z + 1),
        (x + 1, y + 1, z + 1),
        (x, y + 1, z + 1),
      ],
    };

    self.add_quad(v1, v2, v3, v4);
  }
}

/// Post-processes the given indexed mesh to ensure manifoldness by splitting vertices as needed
fn fix_manifold(indices: Vec<u32>, verts: Vec<f32>) -> (Vec<u32>, Vec<f32>) {
  let num_verts = verts.len() / 3;
  let num_faces = indices.len() / 3;

  // vtx->face adjacency map
  let mut vert_to_faces = vec![SmallVec::<[usize; 8]>::new(); num_verts];
  for f in 0..num_faces {
    let base = f * 3;
    for c in 0..3 {
      let v = indices[base + c] as usize;
      vert_to_faces[v].push(f);
    }
  }

  // find non-manifold edges (face count > 2)
  let mut edge_counts = FxHashMap::default();
  for f in 0..num_faces {
    let base = f * 3;
    let vs = [indices[base], indices[base + 1], indices[base + 2]];
    for i in 0..3 {
      let v1 = vs[i];
      let v2 = vs[(i + 1) % 3];
      let key = if v1 < v2 { (v1, v2) } else { (v2, v1) };
      *edge_counts.entry(key).or_insert(0) += 1;
    }
  }

  // split vertices based on connectivity
  let mut new_verts = Vec::new();
  let mut new_indices = vec![0; indices.len()];

  let mut face_component: Vec<usize> = Vec::new();
  let mut comp_to_new_v_idx: Vec<u32> = Vec::new();
  for v_old in 0..num_verts {
    let incident_faces = &vert_to_faces[v_old];
    if incident_faces.is_empty() {
      continue;
    }

    // Determines if two faces sharing v_old are connected via a "good" edge.
    // Two faces are connected if they share a physical edge incident to v_old
    // AND that physical edge is manifold.
    let are_faces_connected = |f1: usize, f2: usize| -> bool {
      let base1 = f1 * 3;
      let base2 = f2 * 3;
      let idxs1 = [indices[base1], indices[base1 + 1], indices[base1 + 2]];
      let idxs2 = [indices[base2], indices[base2 + 1], indices[base2 + 2]];

      for &k1 in &idxs1 {
        if k1 == v_old as u32 {
          continue;
        }
        for &k2 in &idxs2 {
          if k1 == k2 {
            // They share edge (v_old, k1). Is it strictly 2-manifold?
            let key = if (v_old as u32) < k1 {
              (v_old as u32, k1)
            } else {
              (k1, v_old as u32)
            };
            if let Some(&count) = edge_counts.get(&key) {
              return count == 2;
            }
            return false;
          }
        }
      }
      // Special case: Faces might share the vertex but no edge (e.g. bowtie).
      // They are not connected.
      false
    };

    // Find connected components of faces around this vertex
    face_component.clear();
    face_component.resize(incident_faces.len(), 0);
    let mut visited = bitarr![usize, Lsb0; 0; 32];
    let mut comp_count = 0;

    for i in 0..incident_faces.len() {
      if visited[i] {
        continue;
      }

      let mut stack = vec![i];
      visited.set(i, true);
      face_component[i] = comp_count;

      while let Some(curr) = stack.pop() {
        for j in 0..incident_faces.len() {
          if !visited[j] {
            if are_faces_connected(incident_faces[curr], incident_faces[j]) {
              visited.set(j, true);
              face_component[j] = comp_count;
              stack.push(j);
            }
          }
        }
      }
      comp_count += 1;
    }

    comp_to_new_v_idx.clear();
    for _ in 0..comp_count {
      let idx = (new_verts.len() / 3) as u32;
      new_verts.push(verts[v_old * 3]);
      new_verts.push(verts[v_old * 3 + 1]);
      new_verts.push(verts[v_old * 3 + 2]);
      comp_to_new_v_idx.push(idx);
    }

    for (i, &f_idx) in incident_faces.iter().enumerate() {
      let new_v = comp_to_new_v_idx[face_component[i]];
      let base = f_idx * 3;
      if indices[base] == v_old as u32 {
        new_indices[base] = new_v;
      }
      if indices[base + 1] == v_old as u32 {
        new_indices[base + 1] = new_v;
      }
      if indices[base + 2] == v_old as u32 {
        new_indices[base + 2] = new_v;
      }
    }
  }

  (new_indices, new_verts)
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

#[test]
fn non_manifold_repro_2() {
  use mesh::linked_mesh::NonManifoldError;

  // This produces a single-voxel cup in the middle of the shape.  The bottom corner of the cup has
  // an edge which is shared between its bottom and back faces, making it non-manifold.  This is
  // fixed by the explicit `fix_manifold` post-pass.
  let cb = |x, y, z| {
    Ok(if z == 1 && y == 0 {
      1
    } else if z == 0 && y == 1 {
      1
    } else if z == 1 && y == 1 && (x == 0 || x == 2) {
      1
    } else {
      0
    })
  };

  let meshes = sample_voxels([3, 2, 2], cb, false, Vec::new(), Some(false)).unwrap();
  assert_eq!(meshes.len(), 1);
  let mesh = meshes.into_iter().next().unwrap();
  match mesh.mesh.check_is_manifold::<true>() {
    Ok(()) => (),
    Err(NonManifoldError::NonManifoldEdge {
      edge_key,
      face_count,
    }) => {
      let edge = mesh.mesh.edges.get(edge_key).unwrap();
      let v1 = mesh.mesh.vertices.get(edge.vertices[0]).unwrap();
      let v2 = mesh.mesh.vertices.get(edge.vertices[1]).unwrap();
      panic!(
        "Non-manifold edge {:?} -> {:?} with {face_count} faces",
        v1.position, v2.position,
      );
    }
    Err(err) => panic!("Non-manifold error: {err:?}"),
  }
}
