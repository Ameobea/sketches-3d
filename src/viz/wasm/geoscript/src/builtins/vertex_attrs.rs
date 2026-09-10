use std::rc::Rc;

use fxhash::FxHashMap;
use mesh::{
  attrs::{self, SmoothWeights},
  linked_mesh::{mesh_flags, Arity, Channel, Interp, SpatialXform, VertexKey},
  LinkedMesh,
};

use crate::{
  mesh_ops::bake_ao::{bake_ao, AoParams, Refine},
  seq::EagerSeq,
  vertex_cb::{attr_value, VertexCb, IX_KEY},
  ArgRef, ErrorStack, EvalCtx, ManifoldHandle, MeshHandle, Sym, Value,
};

// `position`/`normal` are three.js's own geometry attributes; the rest are bag keys.
const RESERVED: &[&str] = &["position", "pos", "normal", IX_KEY];

fn attr_name(v: &Value) -> Result<&str, ErrorStack> {
  let name = v.as_str().unwrap();
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

fn available(mesh: &LinkedMesh<()>) -> String {
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

fn with_mesh(src: &MeshHandle, mesh: LinkedMesh<()>) -> Value {
  Value::Mesh(Rc::new(MeshHandle {
    mesh: Rc::new(mesh),
    transform: src.transform,
    manifold_handle: Rc::new(ManifoldHandle::new_empty()),
    aabb: src.aabb.clone(),
    bvh: src.bvh.clone(),
    material: src.material.clone(),
  }))
}

pub(crate) fn set_attr_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let name = attr_name(arg_refs[0].resolve(args, kwargs))?;
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

  let mut out = (*mesh.mesh).clone();
  let keys: Vec<VertexKey> = out.vertices.keys().collect();
  let mut vals: Vec<[f32; 4]> = Vec::with_capacity(keys.len());
  let mut arity: Option<Arity> = None;
  let mut push = |v: Value| -> Result<(), ErrorStack> {
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
    vals.push(lanes(&v));
    Ok(())
  };

  match value {
    Value::Callable(cb) => {
      let cb = VertexCb::new(ctx, "set_attr", cb, &["pos", "normal"], &out)?;
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
        push(v)?;
      }
    }
    Value::Sequence(seq) => {
      for v in seq.consume(ctx) {
        push(v?)?;
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
  let into = attr_name(arg_refs[5].resolve(args, kwargs))?;
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
      ctx.prints.borrow_mut().push(msg.clone());
      (ctx.log_fn)(&msg);
    }
    let min_edge = match arg_refs[7].resolve(args, kwargs) {
      Value::Nil => diag * 0.01,
      v => v.as_float().unwrap(),
    };
    Refine {
      tol: tol.max(floor),
      min_edge,
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
    },
    refine.as_ref(),
  )?;
  let mut ch = Channel::for_attr(into, Arity::Scalar);
  for (k, v) in ao {
    ch.set(k, [v, 0., 0., 0.]);
  }
  out.vertex_channels.insert(into.to_owned(), ch);
  // Splits keep the surface, but cached BVH triangle indices no longer map onto the new faces.
  let unchanged = out.vertices.len() == mesh.mesh.vertices.len();
  Ok(Value::Mesh(Rc::new(MeshHandle {
    mesh: Rc::new(out),
    transform: mesh.transform,
    manifold_handle: Rc::new(ManifoldHandle::new_empty()),
    aabb: mesh.aabb.clone(),
    bvh: if unchanged {
      mesh.bvh.clone()
    } else {
      Default::default()
    },
    material: mesh.material.clone(),
  })))
}
