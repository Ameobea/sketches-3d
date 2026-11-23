use std::rc::Rc;

use fxhash::FxHashMap;
use mesh::{
  attrs::{self, SmoothWeights},
  bvh::TriBvh,
  linked_mesh::{mesh_flags, Arity, Channel, Interp, SpatialXform, Vec3, VertexKey},
  LinkedMesh,
};
use nalgebra::{Matrix3, Matrix4};

use crate::{
  mesh_ops::{
    bake_ao::{bake_ao, AoParams},
    refine::{refine_field, Refine, Site},
    split_field::cut_contour,
    surface_chart::PathIndex,
  },
  seq::EagerSeq,
  vertex_cb::{attr_value, VertexCb, IX_KEY},
  ArgRef, ErrorStack, EvalCtx, ManifoldHandle, MeshHandle, Sym, Value, Vec2,
};

// `position`/`normal` are three.js's own geometry attributes; the rest are bag keys.
const RESERVED: &[&str] = &["position", "pos", "normal", IX_KEY];

pub(crate) fn attr_name(name: &str) -> Result<&str, ErrorStack> {
  let mut chars = name.chars();
  let valid = chars
    .next()
    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
    && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
  if !valid {
    return Err(ErrorStack::new(format!(
      "Invalid attribute name `{name}`; use letters, digits, and underscores, starting with a \
       letter or underscore"
    )));
  }
  Ok(name)
}

pub(crate) fn available(mesh: &LinkedMesh<()>) -> String {
  let mut names: Vec<&str> = mesh.vertex_channels.keys().map(String::as_str).collect();
  names.sort_unstable();
  if names.is_empty() {
    "none".to_owned()
  } else {
    names.join(", ")
  }
}

fn arity_of(v: &Value) -> Option<Arity> {
  match v {
    Value::Int(_) | Value::Float(_) => Some(Arity::Scalar),
    Value::Vec2(_) => Some(Arity::Vec2),
    Value::Vec3(_) => Some(Arity::Vec3),
    Value::Vec4(_) => Some(Arity::Vec4),
    _ => None,
  }
}

fn arity_name(a: Arity) -> &'static str {
  match a {
    Arity::Scalar => "num",
    Arity::Vec2 => "vec2",
    Arity::Vec3 => "vec3",
    Arity::Vec4 => "vec4",
  }
}

fn lanes(v: &Value) -> [f32; 4] {
  match v {
    Value::Int(i) => [*i as f32, 0., 0., 0.],
    Value::Float(f) => [*f, 0., 0., 0.],
    Value::Vec2(v) => [v.x, v.y, 0., 0.],
    Value::Vec3(v) => [v.x, v.y, v.z, 0.],
    Value::Vec4(v) => [v.x, v.y, v.z, v.w],
    _ => unreachable!(),
  }
}

/// Refinement keeps the surface (so the AABB) but renumbers triangles under the cached BVH.
pub(crate) fn with_mesh(src: &MeshHandle, mesh: LinkedMesh<()>) -> Value {
  let same_topology = mesh.vertices.len() == src.mesh.vertices.len();
  Value::Mesh(Rc::new(MeshHandle {
    mesh: Rc::new(mesh),
    transform: src.transform,
    manifold_handle: Rc::new(ManifoldHandle::new_empty()),
    aabb: src.aabb.clone(),
    bvh: if same_topology {
      src.bvh.clone()
    } else {
      Default::default()
    },
    material: src.material.clone(),
  }))
}

pub(crate) fn refine_kwargs(
  builtin: &str,
  mesh: &MeshHandle,
  tol: &Value,
  min_edge: &Value,
  max_edge: &Value,
) -> Result<Option<Refine>, ErrorStack> {
  // `max_edge` alone is uniform subdivision to that length before the field is consulted.
  let tol = match (tol, max_edge) {
    (Value::Nil, Value::Nil) => return Ok(None),
    (Value::Nil, _) => f32::INFINITY,
    (tol, _) => tol.as_float().unwrap(),
  };
  if tol <= 0. {
    return Err(ErrorStack::new(format!(
      "`{builtin}` `refine` tolerance must be > 0"
    )));
  }
  let min_edge = match min_edge {
    Value::Nil => {
      let aabb = mesh.get_or_compute_aabb();
      (aabb.maxs - aabb.mins).norm() * 0.01
    }
    v => v.as_float().unwrap(),
  };
  if min_edge <= 0. {
    return Err(ErrorStack::new(format!(
      "`{builtin}` `min_edge` must be > 0"
    )));
  }
  let max_edge = match max_edge {
    Value::Nil => f32::INFINITY,
    v => v.as_float().unwrap(),
  };
  Ok(Some(Refine {
    tol,
    min_edge,
    max_edge,
  }))
}

pub(crate) fn set_attr_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let name = attr_name(arg_refs[0].resolve(args, kwargs).as_str().unwrap())?;
  if RESERVED.contains(&name) {
    return Err(ErrorStack::new(format!(
      "`{name}` is reserved and can't be used as an attribute name"
    )));
  }
  let value = arg_refs[1].resolve(args, kwargs);
  let mesh = arg_refs[2].resolve(args, kwargs).as_mesh().unwrap();
  let spatial = match arg_refs[3].resolve(args, kwargs) {
    Value::Nil => None,
    v => match v.as_str().unwrap() {
      "none" => Some(SpatialXform::Identity),
      "direction" => Some(SpatialXform::Direction),
      other => {
        return Err(ErrorStack::new(format!(
          "Invalid `spatial` for `set_attr`: `{other}`; expected \"none\" or \"direction\""
        )))
      }
    },
  };

  let refine = refine_kwargs(
    "set_attr",
    mesh,
    arg_refs[4].resolve(args, kwargs),
    arg_refs[5].resolve(args, kwargs),
    arg_refs[6].resolve(args, kwargs),
  )?;

  let mut out = (*mesh.mesh).clone();
  let mut keys: Vec<VertexKey> = out.vertices.keys().collect();
  let mut vals: Vec<[f32; 4]> = Vec::with_capacity(keys.len());
  let mut arity: Option<Arity> = None;
  let mut check = |v: Value| -> Result<[f32; 4], ErrorStack> {
    let a = arity_of(&v).ok_or_else(|| {
      ErrorStack::new(format!(
        "`set_attr` values must be numbers or vectors; got {v:?}"
      ))
    })?;
    match arity {
      Some(prev) if prev != a => {
        return Err(ErrorStack::new(format!(
          "`set_attr` values must all have the same type; got both {} and {}",
          arity_name(prev),
          arity_name(a)
        )))
      }
      _ => arity = Some(a),
    }
    Ok(lanes(&v))
  };

  match value {
    Value::Callable(cb) => {
      let cb = VertexCb::new(ctx, "set_attr", cb, &["pos", "normal"], &out)?;
      if refine.is_some() && cb.reads_vertex_data() {
        return Err(ErrorStack::new(
          "`set_attr` with `refine` probes the field between vertices, so the callback may only \
           read `pos` and `normal` (no `ix` or attribute bag keys)",
        ));
      }
      if cb.reads_fixed(1)
        && keys
          .first()
          .is_some_and(|&k| out.displacement_normal(k).is_none())
      {
        out.compute_vertex_displacement_normals();
      }
      for &k in &keys {
        let pos = out.vertices[k].position;
        let normal = out.displacement_normal(k).unwrap_or_default();
        let v = cb
          .invoke(ctx, &[Value::Vec3(pos), Value::Vec3(normal)], &out, k)
          .map_err(|err| err.wrap("error calling `set_attr` callback"))?;
        vals.push(check(v)?);
      }
      if let Some(r) = &refine {
        let mut by_key: FxHashMap<VertexKey, [f32; 4]> =
          keys.iter().copied().zip(vals.iter().copied()).collect();
        refine_field(
          &mut out,
          &mesh.transform,
          &mut by_key,
          r,
          None,
          |m, p, site| {
            let n = site.normal(m);
            let v = cb
              .invoke_fixed(ctx, &[Value::Vec3(p), Value::Vec3(n)])
              .map_err(|err| err.wrap("error calling `set_attr` callback"))?;
            check(v)
          },
        )?;
        keys = out.vertices.keys().collect();
        vals = keys.iter().map(|k| by_key[k]).collect();
      }
    }
    Value::Sequence(_) if refine.is_some() => {
      return Err(ErrorStack::new(
        "`set_attr` `refine` needs a callable value; a sequence can't be sampled between vertices",
      ))
    }
    Value::Sequence(seq) => {
      for v in seq.consume(ctx) {
        vals.push(check(v?)?);
      }
      if vals.len() != keys.len() {
        return Err(ErrorStack::new(format!(
          "`set_attr` sequence has {} values but the mesh has {} vertices",
          vals.len(),
          keys.len()
        )));
      }
    }
    _ => unreachable!(),
  }

  let arity = arity.unwrap_or(Arity::Scalar);
  if let Some(spec) = attrs::known(name) {
    if !spec.arities.contains(&arity) {
      let allowed: Vec<_> = spec.arities.iter().map(|&a| arity_name(a)).collect();
      return Err(ErrorStack::new(format!(
        "Attribute `{name}` must be {}; got {}",
        allowed.join(" or "),
        arity_name(arity)
      )));
    }
  }
  let mut ch = Channel::for_attr(name, arity);
  if let Some(spatial) = spatial {
    ch.spatial = spatial;
    if spatial == SpatialXform::Direction {
      ch.interp = Interp::LerpNormalize;
    }
  }
  for (k, v) in keys.iter().zip(vals) {
    ch.set(*k, v);
  }
  out.vertex_channels.insert(name.to_owned(), ch);
  Ok(with_mesh(mesh, out))
}

pub(crate) fn transfer_attrs_impl(
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let src = arg_refs[0].resolve(args, kwargs).as_mesh().unwrap();
  let dst = arg_refs[1].resolve(args, kwargs).as_mesh().unwrap();
  let mut out = (*dst.mesh).clone();
  crate::mesh_ops::mesh_ops::transfer_attrs_into(src, &mut out, &dst.transform)?;
  Ok(with_mesh(dst, out))
}

pub(crate) fn attr_impl(
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let name = arg_refs[0].resolve(args, kwargs).as_str().unwrap();
  let mesh = arg_refs[1].resolve(args, kwargs).as_mesh().unwrap();
  let ch = mesh.mesh.vertex_channels.get(name).ok_or_else(|| {
    ErrorStack::new(format!(
      "Mesh has no attribute `{name}`; available: {}",
      available(&mesh.mesh)
    ))
  })?;
  let vals: Vec<Value> = mesh
    .mesh
    .vertices
    .keys()
    .map(|k| attr_value(ch, k))
    .collect();
  Ok(Value::Sequence(Rc::new(EagerSeq {
    inner: Rc::new(vals),
  })))
}

pub(crate) fn attrs_impl(
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let mesh = arg_refs[0].resolve(args, kwargs).as_mesh().unwrap();
  let mut names: Vec<&String> = mesh.mesh.vertex_channels.keys().collect();
  names.sort_unstable();
  let vals = names
    .into_iter()
    .map(|n| Value::String(n.clone()))
    .collect();
  Ok(Value::Sequence(Rc::new(EagerSeq {
    inner: Rc::new(vals),
  })))
}

pub(crate) fn drop_attr_impl(
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let name = arg_refs[0].resolve(args, kwargs).as_str().unwrap();
  let mesh = arg_refs[1].resolve(args, kwargs).as_mesh().unwrap();
  let mut out = (*mesh.mesh).clone();
  if out.vertex_channels.remove(name).is_none() {
    return Err(ErrorStack::new(format!(
      "Mesh has no attribute `{name}` to drop; available: {}",
      available(&out)
    )));
  }
  Ok(with_mesh(mesh, out))
}

pub(crate) fn faces_impl(
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let mesh = arg_refs[0].resolve(args, kwargs).as_mesh().unwrap();
  let ix: FxHashMap<VertexKey, i64> = mesh
    .mesh
    .vertices
    .keys()
    .enumerate()
    .map(|(i, k)| (k, i as i64))
    .collect();
  let seq = |vals: Vec<Value>| {
    Value::Sequence(Rc::new(EagerSeq {
      inner: Rc::new(vals),
    }))
  };
  let faces = mesh
    .mesh
    .faces
    .values()
    .map(|f| seq(f.vertices.iter().map(|k| Value::Int(ix[k])).collect()))
    .collect();
  Ok(seq(faces))
}

pub(crate) fn smooth_attr_impl(
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let name = arg_refs[0].resolve(args, kwargs).as_str().unwrap();
  let mesh = arg_refs[1].resolve(args, kwargs).as_mesh().unwrap();
  let iterations = arg_refs[2].resolve(args, kwargs).as_int().unwrap().max(0) as usize;
  let lambda = arg_refs[3].resolve(args, kwargs).as_float().unwrap();
  let weights = match arg_refs[4].resolve(args, kwargs).as_str().unwrap() {
    "uniform" => SmoothWeights::Uniform,
    "cotan" => SmoothWeights::Cotan,
    other => {
      return Err(ErrorStack::new(format!(
        "Invalid `weights` for `smooth_attr`: `{other}`; expected \"uniform\" or \"cotan\""
      )))
    }
  };
  let pin = match arg_refs[5].resolve(args, kwargs) {
    Value::Nil => None,
    v => Some(v.as_str().unwrap()),
  };
  for attr in [Some(name), pin].into_iter().flatten() {
    if !mesh.mesh.vertex_channels.contains_key(attr) {
      return Err(ErrorStack::new(format!(
        "Mesh has no attribute `{attr}`; available: {}",
        available(&mesh.mesh)
      )));
    }
  }
  let mut out = (*mesh.mesh).clone();
  out.smooth_channel(name, iterations, lambda, weights, pin);
  Ok(with_mesh(mesh, out))
}

pub(crate) fn bake_ao_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let mesh = arg_refs[0].resolve(args, kwargs).as_mesh().unwrap();
  let refine_tol = match arg_refs[6].resolve(args, kwargs) {
    Value::Nil => None,
    v => {
      let tol = v.as_float().unwrap();
      if tol <= 0. {
        return Err(ErrorStack::new("`bake_ao` `refine` tolerance must be > 0"));
      }
      Some(tol)
    }
  };
  let samples = match arg_refs[1].resolve(args, kwargs) {
    Value::Nil if refine_tol.is_some() => 256,
    Value::Nil => 32,
    v => v.as_int().unwrap().max(1) as usize,
  };
  let max_dist = match arg_refs[2].resolve(args, kwargs) {
    Value::Nil => f32::MAX,
    v => v.as_float().unwrap(),
  };
  let occluders: Vec<Rc<MeshHandle>> = match arg_refs[3].resolve(args, kwargs) {
    Value::Nil => Vec::new(),
    Value::Mesh(m) => vec![Rc::clone(m)],
    Value::Sequence(seq) => seq
      .consume(ctx)
      .map(|v| match v? {
        Value::Mesh(m) => Ok(m),
        other => Err(ErrorStack::new(format!(
          "`bake_ao` occluders must be meshes; got {other:?}"
        ))),
      })
      .collect::<Result<_, _>>()?,
    _ => unreachable!(),
  };
  let diag = {
    let aabb = mesh.get_or_compute_aabb();
    (aabb.maxs - aabb.mins).norm()
  };
  let bias = match arg_refs[4].resolve(args, kwargs) {
    Value::Nil => (diag * 1e-4).max(1e-6),
    v => v.as_float().unwrap(),
  };
  let into = attr_name(arg_refs[5].resolve(args, kwargs).as_str().unwrap())?;
  if RESERVED.contains(&into) {
    return Err(ErrorStack::new(format!(
      "`{into}` is reserved and can't be used as an attribute name"
    )));
  }
  if attrs::known(into).is_some_and(|spec| !spec.arities.contains(&Arity::Scalar)) {
    return Err(ErrorStack::new(format!(
      "`bake_ao` writes a scalar attribute but `{into}` isn't one; bake into e.g. \"ao\" and map \
       it with `set_attr(\"{into}\", |p, n, {{ao}}| v3(ao))`"
    )));
  }
  let refine = refine_tol.map(|tol| {
    // Measured estimator RMS error is ~0.34·samples^-0.75 (nearly independent of the occlusion
    // level); a probe-vs-chord comparison sees ~1.25× that, and tolerances under ~3σ turn noise
    // into splits all the way down to `min_edge`.
    let floor = 1.3 * (samples as f32).powf(-0.75);
    if tol < floor {
      let msg = format!(
        "bake_ao: refine={tol} is below the sampling noise floor at {samples} samples \
         (~{floor:.3}); using {floor:.3}.  Raise `samples` to refine finer."
      );
      ctx.push_print(msg);
    }
    let min_edge = match arg_refs[7].resolve(args, kwargs) {
      Value::Nil => diag * 0.01,
      v => v.as_float().unwrap(),
    };
    let max_edge = match arg_refs[9].resolve(args, kwargs) {
      Value::Nil => f32::INFINITY,
      v => v.as_float().unwrap(),
    };
    Refine {
      tol: tol.max(floor),
      min_edge,
      max_edge,
    }
  });

  let mut out = (*mesh.mesh).clone();
  if arg_refs[8].resolve(args, kwargs).as_bool().unwrap() {
    out.recompute_shading_normals_preserving_seams(
      ctx.read_sharp_angle_threshold_degrees().to_radians(),
    );
    out.flags |= mesh_flags::NO_WELD;
  }
  let ao = bake_ao(
    mesh,
    &mut out,
    &occluders,
    &AoParams {
      samples,
      max_dist,
      bias,
      intensity: crate::builtins::bake::ao_intensity(
        "bake_ao",
        arg_refs[10].resolve(args, kwargs),
      )?,
    },
    refine.as_ref(),
  )?;
  let mut ch = Channel::for_attr(into, Arity::Scalar);
  for (k, v) in ao {
    ch.set(k, [v, 0., 0., 0.]);
  }
  out.vertex_channels.insert(into.to_owned(), ch);
  Ok(with_mesh(mesh, out))
}

pub(crate) fn sdf_impl(
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let mesh = arg_refs[0].resolve(args, kwargs).as_mesh().unwrap();
  let p = *arg_refs[1].resolve(args, kwargs).as_vec3().unwrap();
  let signed = arg_refs[2].resolve(args, kwargs).as_bool().unwrap();
  mesh
    .get_or_create_bvh()
    .signed_distance(&p, f32::INFINITY, signed)
    .map(Value::Float)
    .ok_or_else(|| ErrorStack::new("`sdf` needs a mesh with at least one face"))
}

enum Src<'a> {
  Mesh(Rc<TriBvh>),
  Cb(VertexCb<'a>),
  /// A filled 2D path, read in the mesh's `chart` coordinates.
  Path(PathIndex),
}

/// A region-selecting field (`split_by_field`, `emboss`), evaluable at vertices and at points
/// on edges and faces.
pub(crate) struct FieldSrc<'a> {
  src: Src<'a>,
  builtin: String,
  chart: String,
  /// Set when the field wants world-space points: the transform and its normal matrix.
  world: Option<(Matrix4<f32>, Matrix3<f32>)>,
}

impl<'a> FieldSrc<'a> {
  fn new(
    ctx: &EvalCtx,
    builtin: &str,
    field: &'a Value,
    mesh: &MeshHandle,
    out: &mut LinkedMesh<()>,
    refined: bool,
    transformed: bool,
    chart: &str,
  ) -> Result<Self, ErrorStack> {
    let src = match field {
      Value::Path(_) => {
        if out.vertex_channels.get(chart).map(|ch| ch.store.arity()) != Some(2) {
          return Err(ErrorStack::new(format!(
            "`{builtin}` with a 2D path reads surface coordinates from the vec2 vertex attribute \
             `{chart}`, which this mesh doesn't have; add it with `surface_chart`, or pass \
             `chart=\"uv\"` to use UVs"
          )));
        }
        let angle = ctx.resolve_curve_angle_degrees(&Value::Nil).to_radians();
        let loops = super::discretize_fillable_paths(ctx, field, angle, 64, builtin)?;
        Src::Path(PathIndex::new(&loops))
      }
      Value::Mesh(m) => {
        let bvh = m.get_or_create_bvh();
        if bvh.closest_point(&Vec3::zeros()).is_none() {
          return Err(ErrorStack::new(format!(
            "`{builtin}` mask mesh has no faces"
          )));
        }
        Src::Mesh(bvh)
      }
      Value::Callable(cb) => {
        let cb = VertexCb::new(ctx, builtin, cb, &["pos", "normal"], out)?;
        if refined && cb.reads_vertex_data() {
          return Err(ErrorStack::new(format!(
            "`{builtin}` with `refine` probes the field between vertices, so the callback may \
             only read `pos` and `normal` (no `ix` or attribute bag keys)"
          )));
        }
        if cb.reads_fixed(1)
          && out
            .vertices
            .keys()
            .next()
            .is_some_and(|k| out.displacement_normal(k).is_none())
        {
          out.compute_vertex_displacement_normals();
        }
        Src::Cb(cb)
      }
      _ => unreachable!(),
    };
    let linear = mesh.transform.fixed_view::<3, 3>(0, 0).into_owned();
    let normal_mat = linear.try_inverse().map_or(linear, |inv| inv.transpose());
    Ok(Self {
      src,
      builtin: builtin.to_owned(),
      chart: chart.to_owned(),
      world: transformed.then_some((mesh.transform, normal_mat)),
    })
  }

  fn point(&self, p: Vec3) -> Vec3 {
    self.world.map_or(p, |(xf, _)| (xf * p.push(1.)).xyz())
  }

  fn normal(&self, n: Vec3) -> Vec3 {
    self
      .world
      .map_or(n, |(_, normal_mat)| (normal_mat * n).normalize())
  }

  fn scalar(&self, v: Value) -> Result<f32, ErrorStack> {
    v.as_float().ok_or_else(|| {
      ErrorStack::new(format!(
        "`{}` field callback must return a number; got {v:?}",
        self.builtin
      ))
    })
  }

  fn chart_at(&self, m: &LinkedMesh<()>, k: VertexKey) -> Vec2 {
    let [u, v, ..] = m.vertex_channels[&self.chart].get(k).unwrap_or_default();
    Vec2::new(u, v)
  }

  pub(crate) fn at_vertex(
    &self,
    ctx: &EvalCtx,
    m: &LinkedMesh<()>,
    k: VertexKey,
  ) -> Result<f32, ErrorStack> {
    let p = self.point(m.vertices[k].position);
    match &self.src {
      Src::Mesh(bvh) => Ok(bvh.signed_distance(&p, f32::INFINITY, true).unwrap()),
      Src::Cb(cb) => {
        let n = self.normal(m.displacement_normal(k).unwrap_or_default());
        self.scalar(cb.invoke(ctx, &[Value::Vec3(p), Value::Vec3(n)], m, k)?)
      }
      Src::Path(path) => Ok(path.sdf(self.chart_at(m, k))),
    }
  }

  /// At `p` on an edge or at a face's centroid; requires a callback that reads no vertex data.
  pub(crate) fn at(
    &self,
    ctx: &EvalCtx,
    m: &LinkedMesh<()>,
    p: Vec3,
    site: &Site,
  ) -> Result<f32, ErrorStack> {
    match &self.src {
      Src::Mesh(bvh) => Ok(
        bvh
          .signed_distance(&self.point(p), f32::INFINITY, true)
          .unwrap(),
      ),
      Src::Cb(cb) => {
        let args = [
          Value::Vec3(self.point(p)),
          Value::Vec3(self.normal(site.normal(m))),
        ];
        self.scalar(cb.invoke_fixed(ctx, &args)?)
      }
      Src::Path(path) => Ok(path.sdf(self.chart_on(m, p, site))),
    }
  }

  /// The chart is linear over each face, so a point reads it barycentrically.
  fn chart_on(&self, m: &LinkedMesh<()>, p: Vec3, site: &Site) -> Vec2 {
    match site {
      Site::Edge(g) => {
        let [a, b] = m.edges[g[0]].vertices;
        let (pa, pb) = (m.vertices[a].position, m.vertices[b].position);
        let t = (p - pa).norm() / (pb - pa).norm().max(1e-20);
        self.chart_at(m, a).lerp(&self.chart_at(m, b), t)
      }
      Site::Face(f) => {
        m.faces[*f]
          .vertices
          .iter()
          .map(|&k| self.chart_at(m, k))
          .sum::<Vec2>()
          / 3.
      }
    }
  }

  /// Which part of the field's source is nearest a vertex, where the field is a distance to
  /// several: its value creases wherever that changes.
  pub(crate) fn feature(&self, m: &LinkedMesh<()>, k: VertexKey) -> Option<u32> {
    match &self.src {
      Src::Path(path) => Some(path.run_dist(self.chart_at(m, k), None).0),
      _ => None,
    }
  }

  /// Distance to feature `a` minus distance to `b`: zero on the crease between them.
  pub(crate) fn feature_gap(
    &self,
    m: &LinkedMesh<()>,
    p: Vec3,
    site: &Site,
    a: u32,
    b: u32,
  ) -> Option<f32> {
    match &self.src {
      Src::Path(path) => {
        let uv = self.chart_on(m, p, site);
        Some(path.run_dist(uv, Some(a)).1 - path.run_dist(uv, Some(b)).1)
      }
      _ => None,
    }
  }

  pub(crate) fn has_features(&self) -> bool {
    matches!(&self.src, Src::Path(..))
  }

  pub(crate) fn probes_freely(&self) -> bool {
    !matches!(&self.src, Src::Cb(cb) if cb.reads_vertex_data())
  }
}

/// Evaluates `field` at every vertex (refining first when asked) and cuts the mesh along its
/// `thr` contour, leaving each face wholly on one side.
#[allow(clippy::too_many_arguments)]
pub(crate) fn cut_by_field<'a>(
  ctx: &EvalCtx,
  builtin: &str,
  field: &'a Value,
  mesh: &MeshHandle,
  thr: f32,
  refine: Option<Refine>,
  transformed: bool,
  chart: &str,
) -> Result<(LinkedMesh<()>, FxHashMap<VertexKey, f32>, FieldSrc<'a>), ErrorStack> {
  let mut out = (*mesh.mesh).clone();
  let src = FieldSrc::new(
    ctx,
    builtin,
    field,
    mesh,
    &mut out,
    refine.is_some(),
    transformed,
    chart,
  )?;
  let mut vals: FxHashMap<VertexKey, f32> = FxHashMap::default();
  for k in out.vertices.keys() {
    vals.insert(k, src.at_vertex(ctx, &out, k)?);
  }
  if let Some(r) = &refine {
    let mut lanes: FxHashMap<VertexKey, [f32; 4]> =
      vals.iter().map(|(k, v)| (*k, [*v, 0., 0., 0.])).collect();
    refine_field(
      &mut out,
      &mesh.transform,
      &mut lanes,
      r,
      Some(thr),
      |m, p, site| Ok([src.at(ctx, m, p, &site)?, 0., 0., 0.]),
    )?;
    vals = lanes.into_iter().map(|(k, v)| (k, v[0])).collect();
  }
  cut_contour(&mut out, &mut vals, thr);
  Ok((out, vals, src))
}

pub(crate) fn split_by_field_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let field = arg_refs[0].resolve(args, kwargs);
  let mesh = arg_refs[1].resolve(args, kwargs).as_mesh().unwrap();
  let thr = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
  let refine = refine_kwargs(
    "split_by_field",
    mesh,
    arg_refs[3].resolve(args, kwargs),
    arg_refs[4].resolve(args, kwargs),
    arg_refs[5].resolve(args, kwargs),
  )?;
  let transformed = arg_refs[6].resolve(args, kwargs).as_bool().unwrap();
  let (out, vals, _) = cut_by_field(
    ctx,
    "split_by_field",
    field,
    mesh,
    thr,
    refine,
    transformed,
    "",
  )?;
  let mut parts = out.extract_face_partitions(2, |fk| {
    usize::from(!out.faces[fk].vertices.iter().any(|k| vals[k] < thr))
  });
  let wrap = |m: LinkedMesh<()>| {
    Value::Mesh(Rc::new(MeshHandle {
      mesh: Rc::new(m),
      transform: mesh.transform,
      manifold_handle: Rc::new(ManifoldHandle::new_empty()),
      aabb: Default::default(),
      bvh: Default::default(),
      material: mesh.material.clone(),
    }))
  };
  let outside = wrap(parts.pop().unwrap());
  let inside = wrap(parts.pop().unwrap());
  Ok(Value::Sequence(Rc::new(EagerSeq {
    inner: Rc::new(vec![inside, outside]),
  })))
}
