use std::rc::Rc;

use fxhash::FxHashMap;
use mesh::{
  linked_mesh::{mesh_flags, Arity, Channel, FlipXform, Interp, SpatialXform, Vec3, VertexKey},
  slotmap_utils::vkey,
  LinkedMesh, OwnedIndexedMesh,
};
use nalgebra::Matrix3;
use smallvec::SmallVec;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

use crate::ErrorStack;
#[cfg(target_arch = "wasm32")]
use crate::MeshHandle;
use crate::Sym;
use crate::{ArgRef, EvalCtx, Value};

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(module = "src/geoscript/manifold")]
extern "C" {
  /// Lane maps per `lane_map`; `num_prop` counts xyz plus the union lanes.
  pub fn apply_boolean(
    a_handle: usize,
    a_transform: &[f32],
    a_lane_map: &[i32],
    b_handle: usize,
    b_transform: &[f32],
    b_lane_map: &[i32],
    num_prop: u32,
    op: u8,
    handle_only: bool,
  ) -> Vec<u8>;
  pub fn create_manifold(
    vert_props: &[f32],
    num_prop: u32,
    indices: &[u32],
    merge_from: &[u32],
    merge_to: &[u32],
  ) -> isize;
  fn drop_mesh_handle(handle: usize);
  pub fn drop_all_mesh_handles();
  fn get_last_err() -> String;
}

#[cfg(target_arch = "wasm32")]
pub fn get_last_manifold_err() -> String {
  get_last_err()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn get_last_manifold_err() -> String {
  String::new()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn create_manifold(
  _vert_props: &[f32],
  _num_prop: u32,
  _indices: &[u32],
  _merge_from: &[u32],
  _merge_to: &[u32],
) -> isize {
  0
}

#[cfg(target_arch = "wasm32")]
pub fn drop_manifold_mesh_handle(handle: usize) {
  drop_mesh_handle(handle);
}

#[cfg(not(target_arch = "wasm32"))]
pub fn drop_manifold_mesh_handle(_handle: usize) {}

#[repr(u8)]
#[derive(Clone, Copy)]
pub enum MeshBooleanOp {
  Union = 0,
  Intersection = 1,
  Difference = 2,
}

impl MeshBooleanOp {
  pub(crate) fn from_str(name: &str) -> Self {
    match name {
      "union" => MeshBooleanOp::Union,
      "intersect" => MeshBooleanOp::Intersection,
      "difference" => MeshBooleanOp::Difference,
      _ => panic!("Unknown mesh boolean operation: {name}"),
    }
  }
}

/// One property-lane group carried by a Manifold: an attribute's name, lane count, and the channel
/// policy needed to rebuild it after an op.
#[derive(Clone, PartialEq, Debug)]
pub struct LaneSpec {
  pub name: String,
  pub lanes: usize,
  pub interp: Interp,
  pub flip: FlipXform,
  pub spatial: SpatialXform,
}

impl LaneSpec {
  pub fn for_channel(name: &str, lanes: usize, ch: &Channel<VertexKey>) -> Self {
    LaneSpec {
      name: name.to_owned(),
      lanes,
      interp: ch.interp,
      flip: ch.flip,
      spatial: ch.spatial,
    }
  }

  fn channel(&self) -> Channel<VertexKey> {
    let arity = match self.lanes {
      1 => Arity::Scalar,
      2 => Arity::Vec2,
      3 => Arity::Vec3,
      _ => Arity::Vec4,
    };
    Channel::new(arity, self.interp, self.flip, self.spatial)
  }
}

/// One triangle run of a Manifold output: `[start, end)` into the flat index buffer, the 3x4
/// column-major transform applied to the run's original mesh, and whether its winding was
/// reversed (the subtracted operand's surviving faces).
pub struct ManifoldRun {
  pub start: usize,
  pub end: usize,
  pub transform: [f32; 12],
  pub backside: bool,
  pub original_id: u32,
}

pub struct ManifoldOutput<'a> {
  pub handle: usize,
  pub num_prop: usize,
  /// `vtx_count * num_prop` interleaved xyz + property lanes.
  pub vert_props: &'a [f32],
  pub indices: &'a [u32],
  pub runs: Vec<ManifoldRun>,
  /// Position-coincident property-vertex pairs (`from` duplicates `to`): attribute seams.
  pub merge_from: &'a [u32],
  pub merge_to: &'a [u32],
}

impl ManifoldOutput<'_> {
  pub fn vtx_count(&self) -> usize {
    if self.num_prop == 0 {
      0
    } else {
      self.vert_props.len() / self.num_prop
    }
  }
}

/// Layout of the encoding produced by `encodeManifoldMesh` in `manifold.ts`:
/// u32 `[vtxCount, triCount, handle, numProp, numRun, mergeCount]`, then f32 vertProps, u32
/// triVerts, u32 runIndex (numRun + 1), f32 runTransform (12 per run), u32 backside flags, u32
/// runOriginalID, u32 mergeFromVert, u32 mergeToVert.
pub fn decode_manifold_output(encoded: &[u8]) -> ManifoldOutput<'_> {
  assert!(
    encoded.len() % 4 == 0,
    "manifold output must be 32-bit words"
  );
  let words = encoded.len() / 4;
  let u32s = unsafe { std::slice::from_raw_parts(encoded.as_ptr() as *const u32, words) };
  let f32s = unsafe { std::slice::from_raw_parts(encoded.as_ptr() as *const f32, words) };

  let [vtx_count, tri_count, handle, num_prop, num_run, merge_count] =
    [0, 1, 2, 3, 4, 5].map(|i| u32s[i] as usize);
  let mut at = 6;
  let vert_props = &f32s[at..at + vtx_count * num_prop];
  at += vtx_count * num_prop;
  let indices = &u32s[at..at + tri_count * 3];
  at += tri_count * 3;
  let run_index = &u32s[at..at + if num_run > 0 { num_run + 1 } else { 0 }];
  at += run_index.len();
  let run_xforms = &f32s[at..at + num_run * 12];
  at += num_run * 12;
  let backsides = &u32s[at..at + num_run];
  at += num_run;
  let originals = &u32s[at..at + num_run];
  at += num_run;
  let merge_from = &u32s[at..at + merge_count];
  at += merge_count;
  let merge_to = &u32s[at..at + merge_count];
  let runs = (0..num_run)
    .map(|r| ManifoldRun {
      start: run_index[r] as usize,
      end: run_index[r + 1] as usize,
      transform: run_xforms[r * 12..r * 12 + 12].try_into().unwrap(),
      backside: backsides[r] != 0,
      original_id: originals[r],
    })
    .collect();

  ManifoldOutput {
    handle,
    num_prop,
    vert_props,
    indices,
    runs,
    merge_from,
    merge_to,
  }
}

pub fn interleave_props(raw: &OwnedIndexedMesh) -> Vec<f32> {
  let n = raw.vertices.len() / 3;
  let lanes: usize = raw.attrs.iter().map(|a| a.arity).sum();
  let mut out = Vec::with_capacity(n * (3 + lanes));
  for i in 0..n {
    out.extend_from_slice(&raw.vertices[i * 3..i * 3 + 3]);
    for a in &raw.attrs {
      out.extend_from_slice(&a.data[i * a.arity..(i + 1) * a.arity]);
    }
  }
  out
}

/// `(mergeFromVert, mergeToVert)` pairing every position-coincident duplicate with the first
/// vertex at that exact position.
pub fn seam_merge_vectors(positions: &[f32]) -> (Vec<u32>, Vec<u32>) {
  let mut first: FxHashMap<[u32; 3], u32> = FxHashMap::default();
  let (mut from, mut to) = (Vec::new(), Vec::new());
  for (i, p) in positions.chunks_exact(3).enumerate() {
    let key = [p[0].to_bits(), p[1].to_bits(), p[2].to_bits()];
    match first.entry(key) {
      std::collections::hash_map::Entry::Vacant(v) => {
        v.insert(i as u32);
      }
      std::collections::hash_map::Entry::Occupied(o) => {
        from.push(i as u32);
        to.push(*o.get());
      }
    }
  }
  (from, to)
}

/// Sorted union of two lane layouts; the same name must carry the same lane count on both sides
/// (policy comes from the first operand that has it).
pub fn union_layout(a: &[LaneSpec], b: &[LaneSpec]) -> Result<Vec<LaneSpec>, ErrorStack> {
  let mut out: Vec<LaneSpec> = a.to_vec();
  for spec in b {
    match out.iter().find(|s| s.name == spec.name) {
      Some(s) if s.lanes != spec.lanes => {
        return Err(ErrorStack::new(format!(
          "Attribute `{}` has {} lanes on one boolean operand and {} on the other; give both \
           operands the same type",
          spec.name, s.lanes, spec.lanes
        )))
      }
      Some(_) => {}
      None => out.push(spec.clone()),
    }
  }
  out.sort_unstable_by(|x, y| x.name.cmp(&y.name));
  Ok(out)
}

/// Per union lane, the operand's source lane (0-based after xyz) or -1 to zero-fill.
pub fn lane_map(layout: &[LaneSpec], union: &[LaneSpec]) -> Vec<i32> {
  let mut map = Vec::new();
  for spec in union {
    let mut src = 0usize;
    let found = layout.iter().find(|s| {
      if s.name == spec.name {
        true
      } else {
        src += s.lanes;
        false
      }
    });
    for k in 0..spec.lanes {
      map.push(found.map_or(-1, |_| (src + k) as i32));
    }
  }
  map
}

/// Rebuilds a `LinkedMesh` from a Manifold output: xyz + one channel per layout entry, per-run
/// fixups (direction channels rotate by the run's transform, backside and mirrored runs flip),
/// then seam handling. Duplicates that Manifold's merge vectors pair across *different* originals
/// are the cut curve: they weld back to one vertex with their values blended per channel rule so
/// the result stays 2-manifold, unless `split_seams`. Pairs within one original are authored seams
/// and always survive; any surviving pair marks the mesh `NO_WELD`.
pub fn manifold_output_to_mesh(
  out: &ManifoldOutput,
  layout: &[LaneSpec],
  split_seams: bool,
) -> LinkedMesh<()> {
  let n = out.vtx_count();
  let stride = out.num_prop;
  let pkey = |i: usize| vkey(i as u32 + 1, 1);

  // per-property-vertex channels, so the fixups can use the channel policy helpers
  let mut tmp: Vec<Channel<VertexKey>> = layout.iter().map(LaneSpec::channel).collect();
  let mut offset = 3;
  for (spec, ch) in layout.iter().zip(&mut tmp) {
    for i in 0..n {
      let at = i * stride + offset;
      let mut v = [0f32; 4];
      v[..spec.lanes].copy_from_slice(&out.vert_props[at..at + spec.lanes]);
      ch.set(pkey(i), v);
    }
    offset += spec.lanes;
  }

  let needs_fixup = tmp
    .iter()
    .any(|ch| ch.spatial == SpatialXform::Direction || ch.flip == FlipXform::Negate);
  let mut orig_of = vec![u32::MAX; n];
  let mut seen = vec![false; n];
  for run in &out.runs {
    let linear = Matrix3::from_column_slice(&run.transform[..9]);
    let rotates = linear != Matrix3::identity();
    // Manifold reverses a mirrored operand's winding without marking its runs backside, so it
    // flips like `bake_transform`; mirrored *and* subtracted cancels out.
    let flips = run.backside != (linear.determinant() < 0.);
    for &ix in &out.indices[run.start..run.end] {
      let i = ix as usize;
      if orig_of[i] == u32::MAX {
        orig_of[i] = run.original_id;
      }
      if !needs_fixup || (!rotates && !flips) || std::mem::replace(&mut seen[i], true) {
        continue;
      }
      for ch in &mut tmp {
        if rotates {
          ch.transform_direction_slot(pkey(i), &linear);
        }
        if flips {
          ch.flip_slot(pkey(i));
        }
      }
    }
  }

  let mut alias: Vec<u32> = (0..n as u32).collect();
  let mut kept_seam = false;
  for (&from, &to) in out.merge_from.iter().zip(out.merge_to) {
    if split_seams || orig_of[from as usize] == orig_of[to as usize] {
      kept_seam = true;
    } else {
      alias[from as usize] = to;
    }
  }
  for i in 0..n {
    let mut a = alias[i];
    while alias[a as usize] != a {
      a = alias[a as usize];
    }
    alias[i] = a;
  }
  let mut new_ix = vec![u32::MAX; n];
  let mut positions: Vec<Vec3> = Vec::with_capacity(n);
  let mut groups: Vec<SmallVec<[u32; 2]>> = Vec::with_capacity(n);
  for i in 0..n {
    if alias[i] as usize == i {
      new_ix[i] = positions.len() as u32;
      let at = i * stride;
      positions.push(Vec3::new(
        out.vert_props[at],
        out.vert_props[at + 1],
        out.vert_props[at + 2],
      ));
      groups.push(SmallVec::from_elem(i as u32, 1));
    }
  }
  for i in 0..n {
    if alias[i] as usize != i {
      groups[new_ix[alias[i] as usize] as usize].push(i as u32);
    }
  }
  let indices: Vec<u32> = out
    .indices
    .iter()
    .map(|&ix| new_ix[alias[ix as usize] as usize])
    .collect();
  let mut mesh = LinkedMesh::from_indexed_vertices(&positions, &indices, None, None);

  for (spec, src) in layout.iter().zip(&tmp) {
    let mut ch = spec.channel();
    for (j, group) in groups.iter().enumerate() {
      let w = 1. / group.len() as f32;
      let srcs: SmallVec<[(VertexKey, f32); 4]> =
        group.iter().map(|&i| (pkey(i as usize), w)).collect();
      if let Some(v) = src.blend(&srcs) {
        ch.set(pkey(j), v);
      }
    }
    mesh.vertex_channels.insert(spec.name.clone(), ch);
  }
  if kept_seam {
    mesh.flags |= mesh_flags::NO_WELD;
  }
  mesh
}

#[cfg(target_arch = "wasm32")]
fn apply_mesh_boolean_op(
  a: &MeshHandle,
  b: &MeshHandle,
  op: MeshBooleanOp,
  handle_only: bool,
  split_seams: bool,
) -> Result<MeshHandle, ErrorStack> {
  use std::cell::RefCell;

  use nalgebra::Matrix4;

  use crate::ManifoldHandle;

  let a_handle = a
    .get_or_create_handle()
    .map_err(|err| err.wrap("Error applying mesh boolean op"))?;
  let b_handle = b
    .get_or_create_handle()
    .map_err(|err| err.wrap("Error applying mesh boolean op"))?;
  let (a_layout, b_layout) = (a.manifold_handle.layout(), b.manifold_handle.layout());
  let union = union_layout(&a_layout, &b_layout)?;
  let num_prop = 3 + union.iter().map(|s| s.lanes).sum::<usize>();

  let encoded_output = apply_boolean(
    a_handle,
    a.transform.as_slice(),
    &lane_map(&a_layout, &union),
    b_handle,
    b.transform.as_slice(),
    &lane_map(&b_layout, &union),
    num_prop as u32,
    op as u8,
    handle_only,
  );

  let out = decode_manifold_output(&encoded_output);
  let mesh = if handle_only {
    LinkedMesh::new(0, 0, None)
  } else {
    manifold_output_to_mesh(&out, &union, split_seams)
  };
  Ok(MeshHandle {
    mesh: Rc::new(mesh),
    transform: Matrix4::identity(),
    manifold_handle: Rc::new(ManifoldHandle::with_layout(out.handle, union)),
    aabb: RefCell::new(None),
    trimesh: RefCell::new(None),
    material: None,
  })
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn eval_mesh_boolean(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
  ctx: &EvalCtx,
  op: MeshBooleanOp,
  split_seams: bool,
) -> Result<Value, ErrorStack> {
  use std::rc::Rc;

  let mut meshes_iter = match def_ix {
    0 => {
      let a = arg_refs[0].resolve(&args, &kwargs).as_mesh().unwrap();
      let b = arg_refs[1].resolve(&args, &kwargs).as_mesh().unwrap();

      let out_mesh = apply_mesh_boolean_op(&*a, &*b, op, false, split_seams)?;
      return Ok(Value::Mesh(Rc::new(out_mesh)));
    }
    1 => {
      let sequence = arg_refs[0].resolve(&args, &kwargs).as_sequence().unwrap();
      sequence.consume(ctx)
    }
    _ => unimplemented!(),
  };

  let Some(acc_res) = meshes_iter.next() else {
    use std::rc::Rc;

    return Ok(Value::Mesh(Rc::new(MeshHandle::new(Rc::new(
      LinkedMesh::new(0, 0, None),
    )))));
  };
  let acc = acc_res.map_err(|err| err.wrap("Error evaluating mesh in boolean op"))?;
  let mut acc = acc
    .as_mesh()
    .ok_or_else(|| {
      ErrorStack::new(format!(
        "Non-mesh value produced in sequence passed to boolean op: {acc:?}"
      ))
    })?
    .clone(true, true, true);

  let mut meshes_iter = meshes_iter.peekable();
  while let Some(res) = meshes_iter.next() {
    let mesh = res
      .map_err(|err| err.wrap("Error produced from iterator passed to mesh boolean function"))?;
    if let Value::Mesh(mesh) = mesh {
      let handle_only = meshes_iter.peek().is_some();
      acc = apply_mesh_boolean_op(&acc, &mesh, op, handle_only, split_seams)?;
    } else {
      return Err(ErrorStack::new(
        "Non-mesh value produced in sequence passed to boolean op",
      ));
    }
  }

  Ok(Value::Mesh(Rc::new(acc)))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn eval_mesh_boolean(
  _def_ix: usize,
  _arg_refs: &[ArgRef],
  _args: &[Value],
  _kwargs: &FxHashMap<Sym, Value>,
  _ctx: &EvalCtx,
  _op: MeshBooleanOp,
  _split_seams: bool,
) -> Result<Value, ErrorStack> {
  // Err("mesh boolean ops are only supported in wasm".to_owned())
  Ok(Value::Mesh(Rc::new(crate::MeshHandle::new(Rc::new(
    mesh::LinkedMesh::new(0, 0, None),
  )))))
}

#[cfg(test)]
mod tests {
  use mesh::ExportedAttr;

  use super::*;

  /// `runs` = (transform, backside, original id, triangle count); merges pair `from` → `to`.
  fn encode(
    vtx_props: &[f32],
    num_prop: u32,
    indices: &[u32],
    runs: &[([f32; 12], bool, u32, u32)],
    merges: &[(u32, u32)],
  ) -> Vec<u8> {
    let mut words: Vec<u32> = vec![
      (vtx_props.len() as u32) / num_prop,
      indices.len() as u32 / 3,
      7,
      num_prop,
      runs.len() as u32,
      merges.len() as u32,
    ];
    words.extend(vtx_props.iter().map(|f| f.to_bits()));
    words.extend_from_slice(indices);
    if !runs.is_empty() {
      let mut at = 0;
      words.push(0);
      for (_, _, _, tris) in runs {
        at += tris * 3;
        words.push(at);
      }
    }
    for (t, ..) in runs {
      words.extend(t.iter().map(|f| f.to_bits()));
    }
    words.extend(runs.iter().map(|(_, b, ..)| *b as u32));
    words.extend(runs.iter().map(|(_, _, id, _)| *id));
    words.extend(merges.iter().map(|(f, _)| *f));
    words.extend(merges.iter().map(|(_, t)| *t));
    words.iter().flat_map(|w| w.to_ne_bytes()).collect()
  }

  #[test]
  fn decode_rotates_directions_and_flips_backside_runs() {
    // color(3) + tangent(4) lanes; one run rotated 90° about +Y (column-major 3x4), backside
    let rot_y = [0., 0., -1., 0., 1., 0., 1., 0., 0., 0., 0., 0.];
    let mut props = Vec::new();
    for (i, p) in [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]]
      .iter()
      .enumerate()
    {
      props.extend_from_slice(p);
      props.extend_from_slice(&[0.25, 0.5, i as f32]);
      props.extend_from_slice(&[1., 0., 0., 1.]);
    }
    let encoded = encode(&props, 10, &[0, 1, 2], &[(rot_y, true, 1, 1)], &[(2, 1)]);
    let out = decode_manifold_output(&encoded);
    assert_eq!((out.handle, out.vtx_count(), out.runs.len()), (7, 3, 1));
    assert_eq!(out.merge_from, &[2]);

    let spec =
      |name: &str, lanes| LaneSpec::for_channel(name, lanes, &Channel::for_attr(name, Arity::Vec4));
    let layout = vec![spec("color", 3), spec("tangent", 4)];
    // the (2 → 1) pair is within one original: an authored seam, kept
    let mesh = manifold_output_to_mesh(&out, &layout, false);
    assert!(mesh.has_flag(mesh_flags::NO_WELD));
    assert_eq!(mesh.vertices.len(), 3);
    let k1 = vkey(2, 1);
    assert_eq!(
      mesh.vertex_channels["color"].get(k1),
      Some([0.25, 0.5, 1., 0.])
    );
    // rotated (1,0,0) → (0,0,-1), then the backside flip negates every lane
    let t = mesh.vertex_channels["tangent"].get(k1).unwrap();
    assert!(
      t[0].abs() < 1e-6 && t[1].abs() < 1e-6 && (t[2] - 1.).abs() < 1e-6,
      "{t:?}"
    );
    assert_eq!(t[3], -1.);
  }

  #[test]
  fn mirrored_run_flips_like_bake_transform() {
    let mirror_x = [-1., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0.];
    let mut props = Vec::new();
    for p in [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]] {
      props.extend_from_slice(&p);
      props.extend_from_slice(&[1., 0., 0., 1.]);
    }
    let encoded = encode(&props, 7, &[0, 1, 2], &[(mirror_x, false, 1, 1)], &[]);
    let out = decode_manifold_output(&encoded);
    let layout = vec![LaneSpec::for_channel(
      "tangent",
      4,
      &Channel::for_attr("tangent", Arity::Vec4),
    )];
    let mesh = manifold_output_to_mesh(&out, &layout, false);
    // mirrored (1,0,0) → (-1,0,0), then the det<0 flip negates every lane
    assert_eq!(
      mesh.vertex_channels["tangent"].get(vkey(1, 1)),
      Some([1., 0., 0., -1.])
    );
    assert!(!mesh.has_flag(mesh_flags::NO_WELD));
  }

  #[test]
  fn cut_seams_weld_unless_split() {
    // two triangles from different originals sharing the edge (1,2) via duplicates 3,4
    let ident = [1., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0.];
    let pos = [
      [0., 0., 0.],
      [1., 0., 0.],
      [0., 1., 0.],
      [1., 0., 0.],
      [0., 1., 0.],
      [1., 1., 0.],
    ];
    let mut props = Vec::new();
    for (i, p) in pos.iter().enumerate() {
      props.extend_from_slice(p);
      props.push(if i < 3 { 1. } else { 0. });
    }
    let indices = [0, 1, 2, 4, 3, 5];
    let runs = [(ident, false, 1, 1), (ident, false, 2, 1)];
    let merges = [(3, 1), (4, 2)];
    let layout = vec![LaneSpec::for_channel(
      "w",
      1,
      &Channel::for_attr("w", Arity::Scalar),
    )];

    let encoded = encode(&props, 4, &indices, &runs, &merges);
    let welded = manifold_output_to_mesh(&decode_manifold_output(&encoded), &layout, false);
    assert_eq!(welded.vertices.len(), 4);
    assert!(!welded.has_flag(mesh_flags::NO_WELD));
    assert_eq!(
      welded.edges.values().filter(|e| e.faces.len() == 2).count(),
      1
    );
    // cut vertices average both sides; untouched ones keep their own value
    assert_eq!(
      welded.vertex_channels["w"].get(vkey(2, 1)),
      Some([0.5, 0., 0., 0.])
    );
    assert_eq!(
      welded.vertex_channels["w"].get(vkey(1, 1)),
      Some([1., 0., 0., 0.])
    );

    let split = manifold_output_to_mesh(&decode_manifold_output(&encoded), &layout, true);
    assert_eq!(split.vertices.len(), 6);
    assert!(split.has_flag(mesh_flags::NO_WELD));
  }

  #[test]
  fn union_layout_and_lane_maps() {
    let spec =
      |name: &str, lanes| LaneSpec::for_channel(name, lanes, &Channel::for_attr(name, Arity::Vec4));
    let a = vec![spec("uv", 2)];
    let b = vec![spec("color", 3), spec("uv", 2)];
    let union = union_layout(&a, &b).unwrap();
    assert_eq!(union, b);
    assert_eq!(lane_map(&a, &union), vec![-1, -1, -1, 0, 1]);
    assert_eq!(lane_map(&b, &union), vec![0, 1, 2, 3, 4]);
    assert!(union_layout(&a, &[spec("uv", 3)]).is_err());
  }

  #[test]
  fn seam_merge_vectors_pair_duplicates_with_first() {
    let pos = [0., 0., 0., 1., 0., 0., 0., 0., 0., 2., 0., 0., 1., 0., 0.];
    assert_eq!(seam_merge_vectors(&pos), (vec![2, 4], vec![0, 1]));
  }

  #[test]
  fn interleave_props_orders_lanes_per_vertex() {
    let raw = OwnedIndexedMesh {
      vertices: vec![0., 1., 2., 3., 4., 5.],
      shading_normals: None,
      displacement_normals: None,
      attrs: vec![
        ExportedAttr {
          name: "a".into(),
          arity: 1,
          data: vec![9., 8.],
        },
        ExportedAttr {
          name: "b".into(),
          arity: 2,
          data: vec![10., 11., 12., 13.],
        },
      ],
      indices: vec![],
      transform: None,
    };
    assert_eq!(
      interleave_props(&raw),
      vec![0., 1., 2., 9., 10., 11., 3., 4., 5., 8., 12., 13.]
    );
  }
}
