use std::{cell::RefCell, f32::consts::TAU, rc::Rc, str::FromStr};

use fxhash::FxHashMap;
use mesh::{
  csg::Plane,
  linked_mesh::{mesh_flags, Arity, Channel, FlipXform, Interp, SpatialXform, Vec3, VertexKey},
  slotmap_utils::vkey,
  LinkedMesh,
};
use nalgebra::Matrix3;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

use crate::{ErrorStack, ManifoldHandle, MeshHandle, Value};

pub(crate) fn new_uv_channel() -> Channel<VertexKey> {
  Channel::new(Arity::Vec2, Interp::Lerp, FlipXform::Identity, SpatialXform::Identity)
}

pub(crate) fn new_tangent_channel() -> Channel<VertexKey> {
  Channel::new(Arity::Vec4, Interp::Lerp, FlipXform::Negate, SpatialXform::Direction)
}

pub(crate) fn orthonormal_basis(normal: Vec3) -> (Vec3, Vec3) {
  Plane { normal, w: 0. }.compute_basis()
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(module = "src/geoscript/uvUnwrap")]
extern "C" {
  fn unwrap_uvs(
    verts: &[f32],
    indices: &[u32],
    n_cones: u32,
    flatten_to_disk: bool,
    map_to_sphere: bool,
    island_rotation: bool,
  ) -> String;
  fn uv_unwrap_get_verts() -> Vec<f32>;
  fn uv_unwrap_get_indices() -> Vec<u32>;
  fn uv_unwrap_get_uvs() -> Vec<f32>;
  fn uv_unwrap_get_tangents() -> Vec<f32>;
  pub fn get_uv_unwrap_loaded() -> bool;
}

#[cfg(not(target_arch = "wasm32"))]
pub fn get_uv_unwrap_loaded() -> bool {
  false
}

pub enum UvType {
  Auto,
  Unwrap,
  Disk,
  Sphere,
  Planar,
  Cylindrical,
  Tube,
  Strip,
  Toroidal,
}

impl FromStr for UvType {
  type Err = ErrorStack;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    match s.to_lowercase().as_str() {
      "auto" => Ok(UvType::Auto),
      "unwrap" | "conformal" | "atlas" => Ok(UvType::Unwrap),
      "disk" => Ok(UvType::Disk),
      "sphere" | "spherical" => Ok(UvType::Sphere),
      "planar" | "plane" => Ok(UvType::Planar),
      "cylindrical" | "cylinder" => Ok(UvType::Cylindrical),
      "tube" | "pipe" => Ok(UvType::Tube),
      "strip" | "trim" => Ok(UvType::Strip),
      "toroidal" | "torus" => Ok(UvType::Toroidal),
      _ => Err(ErrorStack::new(format!(
        "Invalid `type` for `compute_uvs`: {s:?}.  Expected one of: auto, unwrap, disk, sphere, \
         planar, cylindrical, tube, strip, toroidal"
      ))),
    }
  }
}

pub fn compute_uvs(
  mesh: &MeshHandle,
  uv_type: UvType,
  scale: f32,
  n_cones: u32,
  island_rotation: bool,
  sharp_threshold_rad: f32,
  options: Option<&FxHashMap<String, Value>>,
) -> Result<MeshHandle, ErrorStack> {
  match uv_type {
    UvType::Planar => planar_uvs(mesh, scale),
    UvType::Cylindrical => cylindrical_uvs(mesh, scale, sharp_threshold_rad, options),
    UvType::Tube => super::tube_uvs::tube_uvs(mesh, scale, sharp_threshold_rad, options),
    UvType::Strip => super::strip_uvs::strip_uvs(mesh, scale, sharp_threshold_rad, options),
    UvType::Toroidal => Err(ErrorStack::new(
      "`compute_uvs(type='toroidal')` (closed genus-1) is not yet implemented; for a capped \
       tube-like mesh use type='tube'",
    )),
    UvType::Auto | UvType::Unwrap | UvType::Disk | UvType::Sphere => {
      bff_uvs(mesh, uv_type, scale, n_cones, island_rotation, sharp_threshold_rad)
    }
  }
}

#[cfg(target_arch = "wasm32")]
fn verify_uv_unwrap_loaded() -> Result<(), ErrorStack> {
  crate::or_async_dep_bit(crate::DEP_BIT_UV_UNWRAP);
  if get_uv_unwrap_loaded() {
    Ok(())
  } else {
    Err(ErrorStack::new_uninitialized_module("uv_unwrap"))
  }
}

#[cfg(target_arch = "wasm32")]
fn bff_uvs(
  mesh: &MeshHandle,
  uv_type: UvType,
  scale: f32,
  n_cones: u32,
  island_rotation: bool,
  sharp_threshold_rad: f32,
) -> Result<MeshHandle, ErrorStack> {
  verify_uv_unwrap_loaded()?;

  let (flatten_to_disk, map_to_sphere) = match uv_type {
    UvType::Disk => (true, false),
    UvType::Sphere => (false, true),
    _ => (false, false),
  };

  // Drop degenerate faces: BFF's cotangent-Laplacian divides by triangle area, so a zero-area
  // sliver (common out of CSG) would inject NaN into the conformal solve.
  let raw = mesh.mesh.to_raw_indexed(false, false, false);
  let in_indices =
    unsafe { std::slice::from_raw_parts(raw.indices.as_ptr() as *const u32, raw.indices.len()) };

  let err = unwrap_uvs(
    &raw.vertices,
    in_indices,
    n_cones,
    flatten_to_disk,
    map_to_sphere,
    island_rotation,
  );
  if !err.is_empty() {
    return Err(ErrorStack::new(format!("`compute_uvs` BFF unwrap failed: {err}")));
  }

  let out_verts = uv_unwrap_get_verts();
  let out_indices = uv_unwrap_get_indices();
  let out_uvs = uv_unwrap_get_uvs();
  let out_tangents = uv_unwrap_get_tangents();

  let mut out_mesh = LinkedMesh::from_raw_indexed(&out_verts, &out_indices, None, None);
  attach_uv_tangent(&mut out_mesh, &out_uvs, Some(&out_tangents), scale);
  // Compute shading normals with sharp-edge splitting (same as the render pipeline's finalize), but
  // WITHOUT the merge-by-distance that would weld BFF's coincident UV-seam duplicates.
  // `separate_vertices_and_compute_normals` clones the uv/tangent channels onto split verts.
  out_mesh.mark_edge_sharpness(sharp_threshold_rad);
  out_mesh.separate_vertices_and_compute_normals();
  out_mesh.flags |= mesh_flags::NO_WELD;

  Ok(MeshHandle {
    mesh: Rc::new(out_mesh),
    transform: mesh.transform,
    manifold_handle: Rc::new(ManifoldHandle::new(0)),
    aabb: RefCell::new(None),
    trimesh: RefCell::new(None),
    material: mesh.material.clone(),
  })
}

#[cfg(not(target_arch = "wasm32"))]
fn bff_uvs(
  _mesh: &MeshHandle,
  _uv_type: UvType,
  _scale: f32,
  _n_cones: u32,
  _island_rotation: bool,
  _sharp_threshold_rad: f32,
) -> Result<MeshHandle, ErrorStack> {
  Err(ErrorStack::new(
    "`compute_uvs` with type=auto/unwrap/disk/sphere is only supported in wasm (backed by the BFF \
     unwrap module)",
  ))
}

/// Native planar projection onto the plane orthogonal to the mesh's average face normal.  No seam,
/// so shading normals are left for the render pipeline to compute.
fn planar_uvs(mesh: &MeshHandle, scale: f32) -> Result<MeshHandle, ErrorStack> {
  let mut out = (*mesh.mesh).clone();
  if out.vertices.is_empty() {
    return Err(ErrorStack::new("`compute_uvs`: mesh has no vertices"));
  }

  let (u_axis, v_axis) = orthonormal_basis(average_face_normal(&out));
  let centroid = out.iter_vertices().fold(Vec3::zeros(), |acc, (_, v)| acc + v.position)
    / out.vertices.len() as f32;

  let mut uv_ch = new_uv_channel();
  let mut tan_ch = new_tangent_channel();
  for (key, v) in out.iter_vertices() {
    let d = v.position - centroid;
    uv_ch.set(key, [d.dot(&u_axis) * scale, d.dot(&v_axis) * scale, 0., 0.]);
    tan_ch.set(key, [u_axis.x, u_axis.y, u_axis.z, 1.]);
  }
  out.vertex_channels.insert("uv".to_owned(), uv_ch);
  out.vertex_channels.insert("tangent".to_owned(), tan_ch);

  Ok(MeshHandle {
    mesh: Rc::new(out),
    transform: mesh.transform,
    manifold_handle: Rc::new(ManifoldHandle::new(0)),
    aabb: RefCell::new(None),
    trimesh: RefCell::new(None),
    material: mesh.material.clone(),
  })
}

#[cfg(target_arch = "wasm32")]
fn attach_uv_tangent(
  mesh: &mut LinkedMesh<()>,
  uvs: &[f32],
  tangents: Option<&[f32]>,
  scale: f32,
) {
  let vtx_count = uvs.len() / 2;
  let mut uv_ch = new_uv_channel();
  for i in 0..vtx_count {
    uv_ch.set(vkey(i as u32 + 1, 1), [uvs[i * 2] * scale, uvs[i * 2 + 1] * scale, 0., 0.]);
  }
  mesh.vertex_channels.insert("uv".to_owned(), uv_ch);

  if let Some(t) = tangents.filter(|t| t.len() == vtx_count * 4) {
    let mut tan_ch = new_tangent_channel();
    for i in 0..vtx_count {
      tan_ch.set(vkey(i as u32 + 1, 1), [t[i * 4], t[i * 4 + 1], t[i * 4 + 2], t[i * 4 + 3]]);
    }
    mesh.vertex_channels.insert("tangent".to_owned(), tan_ch);
  }
}

fn average_face_normal(mesh: &LinkedMesh<()>) -> Vec3 {
  let mut acc = Vec3::zeros();
  for (_, face) in mesh.iter_faces() {
    let [a, b, c] = face.vertices;
    let pa = mesh.vertices[a].position;
    let pb = mesh.vertices[b].position;
    let pc = mesh.vertices[c].position;
    acc += (pb - pa).cross(&(pc - pa));
  }
  if acc.norm_squared() > 1e-20 {
    acc.normalize()
  } else {
    Vec3::new(0., 1., 0.)
  }
}

/// Axis of a roughly-cylindrical mesh: the direction of least face-normal variance (a cylinder's
/// normals are radial, spanning the plane orthogonal to the axis). Area-weighted so tessellation
/// density doesn't bias it.
fn cylinder_axis(mesh: &LinkedMesh<()>) -> Result<Vec3, ErrorStack> {
  let mut cov = Matrix3::zeros();
  let mut total = 0f32;
  for (_, face) in mesh.iter_faces() {
    let [a, b, c] = face.vertices;
    let n = (mesh.vertices[b].position - mesh.vertices[a].position)
      .cross(&(mesh.vertices[c].position - mesh.vertices[a].position));
    let area2 = n.norm();
    if area2 < 1e-12 {
      continue;
    }
    let nn = n / area2;
    cov += nn * nn.transpose() * area2;
    total += area2;
  }
  if total < 1e-12 {
    return Err(ErrorStack::new(
      "`compute_uvs(type='cylindrical')`: mesh has no non-degenerate faces",
    ));
  }
  let eig = cov.symmetric_eigen();
  let min_i = (0..3)
    .min_by(|&i, &j| eig.eigenvalues[i].total_cmp(&eig.eigenvalues[j]))
    .unwrap();
  let axis = Vec3::new(
    eig.eigenvectors[(0, min_i)],
    eig.eigenvectors[(1, min_i)],
    eig.eigenvectors[(2, min_i)],
  );
  Ok(axis.normalize())
}

struct CylindricalOptions {
  /// Stretch V to span 0..1 across the axial extent instead of the isotropic (circumference-scaled)
  /// default.  Useful for mapping a texture exactly once along the length of the tube.
  normalize_v: bool,
}

fn parse_cylindrical_options(
  options: Option<&FxHashMap<String, Value>>,
) -> Result<CylindricalOptions, ErrorStack> {
  let mut out = CylindricalOptions { normalize_v: false };
  let Some(map) = options else { return Ok(out) };
  for key in map.keys() {
    if key != "normalize_v" {
      return Err(ErrorStack::new(format!(
        "Unknown option {key:?} for `compute_uvs(type='cylindrical')`; supported options: \
         `normalize_v`"
      )));
    }
  }
  if let Some(v) = map.get("normalize_v") {
    out.normalize_v = v
      .as_bool()
      .ok_or_else(|| ErrorStack::new("`compute_uvs` option `normalize_v` must be a boolean"))?;
  }
  Ok(out)
}

/// Native cylindrical projection.  U wraps the auto-detected axis exactly once (seamless 0/1, meridian
/// seam split).  V is scaled by the tube circumference so texels stay square (isotropic) by default;
/// the `normalize_v` option instead stretches V to span 0..1 across the axial extent.  Cap faces
/// (normal ≈ ±axis) are split off and given a planar disk projection at the tube's texel density,
/// mirroring `rail_sweep`'s cap handling.  `scale` multiplies the whole map uniformly (integer values
/// preserve the seam).
fn cylindrical_uvs(
  mesh: &MeshHandle,
  scale: f32,
  sharp_threshold_rad: f32,
  options: Option<&FxHashMap<String, Value>>,
) -> Result<MeshHandle, ErrorStack> {
  let opts = parse_cylindrical_options(options)?;
  let axis = cylinder_axis(&mesh.mesh)?;
  let (u_ref, v_ref) = orthonormal_basis(axis);

  let raw = mesh.mesh.to_raw_indexed(false, false, false);
  let n_verts = raw.vertices.len() / 3;
  if n_verts == 0 {
    return Err(ErrorStack::new("`compute_uvs`: mesh has no vertices"));
  }
  let pos = |i: usize| {
    Vec3::new(
      raw.vertices[3 * i],
      raw.vertices[3 * i + 1],
      raw.vertices[3 * i + 2],
    )
  };
  let center = (0..n_verts).fold(Vec3::zeros(), |acc, i| acc + pos(i)) / n_verts as f32;

  // Per-vertex tube coordinates + a robust tube radius (on-axis verts excluded) whose circumference
  // sets both the isotropic V scale and the cap texel density.
  let mut turns = Vec::with_capacity(n_verts);
  let mut axials = Vec::with_capacity(n_verts);
  let mut radial_dirs: Vec<Vec3> = Vec::with_capacity(n_verts);
  let (mut radius_sum, mut radius_count) = (0f32, 0usize);
  let (mut axial_min, mut axial_max) = (f32::MAX, f32::MIN);
  for i in 0..n_verts {
    let d = pos(i) - center;
    let ax = d.dot(&axis);
    let radial = d - ax * axis;
    let r = radial.norm();
    if r > 1e-4 {
      radius_sum += r;
      radius_count += 1;
    }
    turns.push(d.dot(&v_ref).atan2(d.dot(&u_ref)) / TAU + 0.5);
    axials.push(ax);
    radial_dirs.push(if r > 1e-6 { radial / r } else { u_ref });
    axial_min = axial_min.min(ax);
    axial_max = axial_max.max(ax);
  }
  let radius = if radius_count > 0 { radius_sum / radius_count as f32 } else { 1. };
  let circumference = (TAU * radius).max(1e-6);
  let axial_span = (axial_max - axial_min).max(1e-6);

  // Unscaled tube UVs: U = turns (one wrap), V isotropic (÷circumference) or normalized (0..1). The
  // tangent points along +U (azimuthal).  `scale` is applied later, uniformly.
  let mut us = turns;
  let mut vs: Vec<f32> = (0..n_verts)
    .map(|i| {
      if opts.normalize_v {
        (axials[i] - axial_min) / axial_span
      } else {
        axials[i] / circumference
      }
    })
    .collect();
  let mut tangents: Vec<[f32; 4]> = (0..n_verts)
    .map(|i| {
      let t = axis.cross(&radial_dirs[i]).normalize();
      [t.x, t.y, t.z, 1.]
    })
    .collect();

  // Classify each face as a cap (normal ≈ ±axis) or tube, then note which verts any tube face uses.
  let is_cap: Vec<bool> = raw
    .indices
    .chunks(3)
    .map(|tri| {
      let n = (pos(tri[1] as usize) - pos(tri[0] as usize))
        .cross(&(pos(tri[2] as usize) - pos(tri[0] as usize)));
      n.norm() > 1e-12 && n.normalize().dot(&axis).abs() > 0.5
    })
    .collect();
  let mut used_by_tube = vec![false; n_verts];
  for (t, tri) in raw.indices.chunks(3).enumerate() {
    if !is_cap[t] {
      for &v in tri {
        used_by_tube[v as usize] = true;
      }
    }
  }

  let mut verts = raw.vertices.clone();
  let mut indices: Vec<u32> = raw.indices.iter().map(|&i| i as u32).collect();
  let push_vert = |verts: &mut Vec<f32>, o: usize| {
    verts.extend_from_slice(&[raw.vertices[3 * o], raw.vertices[3 * o + 1], raw.vertices[3 * o + 2]]);
  };

  // Tube meridian seam split: wrapping tube tris get their low-U corners cloned at U+1.
  let mut seam_dup: FxHashMap<u32, u32> = FxHashMap::default();
  for (t, tri) in indices.chunks_mut(3).enumerate() {
    if is_cap[t] {
      continue;
    }
    let (mn, mx) = tri.iter().fold((f32::MAX, f32::MIN), |(mn, mx), &i| {
      let u = us[i as usize];
      (mn.min(u), mx.max(u))
    });
    if mx - mn <= 0.5 {
      continue;
    }
    for slot in tri {
      if us[*slot as usize] < 0.5 {
        let orig = *slot;
        *slot = *seam_dup.entry(orig).or_insert_with(|| {
          let new_i = (verts.len() / 3) as u32;
          let o = orig as usize;
          push_vert(&mut verts, o);
          us.push(us[o] + 1.);
          vs.push(vs[o]);
          tangents.push(tangents[o]);
          new_i
        });
      }
    }
  }

  // Caps: planar disk projection at the tube's texel density (÷circumference).  Shared rim verts are
  // cloned into cap-owned verts; cap-only verts (the fan centers) are re-projected in place.
  let disk = |o: usize| -> ([f32; 2], [f32; 4]) {
    let d = pos(o) - center;
    (
      [d.dot(&u_ref) / circumference, d.dot(&v_ref) / circumference],
      [u_ref.x, u_ref.y, u_ref.z, 1.],
    )
  };
  let mut cap_dup: FxHashMap<u32, u32> = FxHashMap::default();
  for (t, tri) in indices.chunks_mut(3).enumerate() {
    if !is_cap[t] {
      continue;
    }
    for slot in tri {
      let o = *slot as usize;
      let (uv, tan) = disk(o);
      if used_by_tube[o] {
        *slot = *cap_dup.entry(*slot).or_insert_with(|| {
          let new_i = (verts.len() / 3) as u32;
          push_vert(&mut verts, o);
          us.push(uv[0]);
          vs.push(uv[1]);
          tangents.push(tan);
          new_i
        });
      } else {
        us[o] = uv[0];
        vs[o] = uv[1];
        tangents[o] = tan;
      }
    }
  }

  let mut out = LinkedMesh::from_raw_indexed(&verts, &indices, None, None);
  let mut uv_ch = new_uv_channel();
  let mut tan_ch = new_tangent_channel();
  for i in 0..(verts.len() / 3) {
    let key = vkey(i as u32 + 1, 1);
    uv_ch.set(key, [us[i] * scale, vs[i] * scale, 0., 0.]);
    tan_ch.set(key, tangents[i]);
  }
  out.vertex_channels.insert("uv".to_owned(), uv_ch);
  out.vertex_channels.insert("tangent".to_owned(), tan_ch);
  out.mark_edge_sharpness(sharp_threshold_rad);
  out.separate_vertices_and_compute_normals();
  out.flags |= mesh_flags::NO_WELD;

  Ok(MeshHandle {
    mesh: Rc::new(out),
    transform: mesh.transform,
    manifold_handle: Rc::new(ManifoldHandle::new(0)),
    aabb: RefCell::new(None),
    trimesh: RefCell::new(None),
    material: mesh.material.clone(),
  })
}

// The BFF-backed types (auto/unwrap/disk/sphere) only run under wasm, so these E2E tests cover the
// native `planar` path plus argument/dispatch handling.  The BFF path is verified in-browser.
#[cfg(test)]
mod tests {
  use mesh::linked_mesh::ChannelStore;

  fn render_uvs(src: &str) -> Vec<[f32; 2]> {
    let ctx = crate::parse_and_eval_program(src).unwrap();
    let rendered = ctx.rendered_meshes.into_inner();
    let ChannelStore::Vec2(uv) = &rendered[0].mesh.mesh.vertex_channels["uv"].store else {
      panic!("expected a Vec2 `uv` channel on the output mesh");
    };
    uv.values().copied().collect()
  }

  #[test]
  fn planar_attaches_uv_and_tangent() {
    let ctx =
      crate::parse_and_eval_program("box(2, 2, 2) | compute_uvs(type='planar') | render").unwrap();
    let mesh = &ctx.rendered_meshes.into_inner()[0].mesh.mesh;
    assert!(mesh.vertex_channels.contains_key("uv"), "planar attaches a uv channel");
    assert!(mesh.vertex_channels.contains_key("tangent"), "planar attaches a tangent channel");

    let ChannelStore::Vec2(uv) = &mesh.vertex_channels["uv"].store else {
      panic!("uv channel should be Vec2");
    };
    let spread = uv.values().map(|v| v[0].abs().max(v[1].abs())).fold(0f32, f32::max);
    assert!(spread > 0.1, "planar UVs should span the projection plane, got spread {spread}");
    // planar authors no seams/normals, so it keeps default render finalize (weld + recompute).
    assert_eq!(mesh.flags, 0, "planar should not set any mesh flags");
  }

  #[test]
  fn planar_scale_multiplies_uvs() {
    let max_radius = |scale: &str| {
      let src = format!("box(2, 2, 2) | compute_uvs(type='planar', scale={scale}) | render");
      render_uvs(&src)
        .iter()
        .map(|v| v[0].hypot(v[1]))
        .fold(0f32, f32::max)
    };
    let (a, b) = (max_radius("1"), max_radius("3"));
    assert!((b / a - 3.).abs() < 1e-3, "scale=3 should triple UVs: {a} -> {b}");
  }

  #[test]
  fn unknown_type_errors() {
    let err = crate::parse_and_eval_program("box(1, 1, 1) | compute_uvs(type='nope') | render")
      .unwrap_err();
    assert!(format!("{err}").contains("Invalid `type`"), "got: {err}");
  }

  fn v_bounds(uv: &[[f32; 2]]) -> (f32, f32) {
    let (mut min_v, mut max_v) = (f32::MAX, f32::MIN);
    for v in uv {
      assert!(v[0].is_finite() && v[1].is_finite(), "UV must be finite, got {v:?}");
      min_v = min_v.min(v[1]);
      max_v = max_v.max(v[1]);
    }
    (min_v, max_v)
  }

  /// Widest U spread of any face — the seam split's job is to keep this below 0.5 (no face bridges
  /// the U = 0/1 discontinuity), which is the property that actually makes a tiling texture seamless.
  fn max_face_u_span(mesh: &mesh::LinkedMesh<()>) -> f32 {
    let uv = &mesh.vertex_channels["uv"];
    let mut worst = 0f32;
    for (_, face) in mesh.iter_faces() {
      let (mut lo, mut hi) = (f32::MAX, f32::MIN);
      for &v in &face.vertices {
        let u = uv.get(v).unwrap()[0];
        lo = lo.min(u);
        hi = hi.max(u);
      }
      worst = worst.max(hi - lo);
    }
    worst
  }

  fn collect_uvs(mesh: &mesh::LinkedMesh<()>) -> Vec<[f32; 2]> {
    let ChannelStore::Vec2(uv) = &mesh.vertex_channels["uv"].store else {
      panic!("uv channel should be Vec2");
    };
    uv.values().copied().collect()
  }

  #[test]
  fn cylindrical_isotropic_wraps_and_splits_seam() {
    use mesh::linked_mesh::mesh_flags;

    // Tall cylinder (height 4 > diameter 2) so face-normal PCA reliably resolves the axis.
    let ctx =
      crate::parse_and_eval_program("cylinder(1, 4, 24) | compute_uvs(type='cylindrical') | render")
        .unwrap();
    let mesh = &ctx.rendered_meshes.into_inner()[0].mesh.mesh;
    assert!(mesh.has_flag(mesh_flags::NO_WELD), "cylindrical opts out of the weld to keep its seam");
    assert!(max_face_u_span(mesh) < 0.5, "a face bridges the U seam: {}", max_face_u_span(mesh));

    let (min_v, max_v) = v_bounds(&collect_uvs(mesh));
    // Isotropic V: circumference-scaled (≈axial/2π ≈ ±0.32 here), NOT the raw ±2 world height.
    assert!(
      max_v < 0.5 && min_v > -0.5 && max_v > 0.2,
      "isotropic V should be circumference-scaled, got {min_v}..{max_v}"
    );
  }

  #[test]
  fn cylindrical_normalize_v_spans_unit() {
    let ctx = crate::parse_and_eval_program(
      "cylinder(1, 4, 24) | compute_uvs(type='cylindrical', options={ normalize_v: true }) | render",
    )
    .unwrap();
    let mesh = &ctx.rendered_meshes.into_inner()[0].mesh.mesh;
    assert!(max_face_u_span(mesh) < 0.5, "seam split still holds in normalize_v mode");
    // V is stretched to span the full axial extent (tube reaches ~1), unlike the isotropic ~0.32.
    let (_, max_v) = v_bounds(&collect_uvs(mesh));
    assert!(max_v > 0.9, "normalize_v should stretch V toward 1, got max_v {max_v}");
  }

  #[test]
  fn cylindrical_unknown_option_errors() {
    let err = crate::parse_and_eval_program(
      "cylinder(1, 4, 24) | compute_uvs(type='cylindrical', options={ bogus: 1 }) | render",
    )
    .unwrap_err();
    assert!(format!("{err}").contains("Unknown option"), "got: {err}");
  }

  #[test]
  fn nonpositive_scale_errors() {
    let err = crate::parse_and_eval_program(
      "box(1, 1, 1) | compute_uvs(type='planar', scale=0) | render",
    )
    .unwrap_err();
    assert!(format!("{err}").contains("scale"), "got: {err}");
  }
}
