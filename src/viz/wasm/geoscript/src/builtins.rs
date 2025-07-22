use paste::paste;
use std::cell::RefCell;
use std::cmp::Reverse;
use std::marker::ConstParamTy;
use std::rc::Rc;

use fxhash::FxHashMap;
use mesh::{
  linked_mesh::{DisplacementNormalMethod, FaceKey, Plane, Vec3, Vertex, VertexKey},
  LinkedMesh, OwnedIndexedMesh,
};
use nalgebra::{Matrix3, Matrix4, Point3, Rotation3, UnitQuaternion};
use parry3d::bounding_volume::Aabb;
use parry3d::math::{Isometry, Point};
use parry3d::query::Ray;
use rand::Rng;

use crate::{
  lights::{AmbientLight, DirectionalLight, Light},
  mesh_ops::{
    extrude::extrude,
    extrude_pipe::{extrude_pipe, EndMode},
    fan_fill::fan_fill,
    mesh_boolean::{eval_mesh_boolean, MeshBooleanOp},
    mesh_ops::{
      convex_hull_from_verts, get_geodesic_error, simplify_mesh, split_mesh_by_plane,
      trace_geodesic_path,
    },
    stitch_contours::stitch_contours,
  },
  noise::fbm,
  path_building::{build_torus_knot_path, cubic_bezier_3d_path, superellipse_path},
  seq::{
    ChainSeq, EagerSeq, FilterSeq, FlattenSeq, IteratorSeq, MeshVertsSeq, PointDistributeSeq,
    ScanSeq, SkipSeq, SkipWhileSeq, TakeSeq, TakeWhileSeq,
  },
  seq_as_eager, ArgRef, Callable, ComposedFn, ErrorStack, EvalCtx, MapSeq, Value, Vec2,
};
use crate::{ManifoldHandle, MeshHandle};

pub(crate) mod fn_defs;

pub(crate) static FUNCTION_ALIASES: phf::Map<&'static str, &'static str> = phf::phf_map! {
  "trans" => "translate",
  "v2" => "vec2",
  "v3" => "vec3",
  "subdivide" => "tessellate",
  "tess" => "tessellate",
  "length" => "len",
  "dist" => "distance",
  "mag" => "len",
  "magnitude" => "len",
  "bezier" => "bezier3d",
  "sphere" => "icosphere",
  "cyl" => "cylinder",
  "superellipse" => "superellipse_path",
  "rounded_rectangle" => "superellipse_path",
  "rounded_rect" => "superellipse_path",
  "mix" => "lerp",
};

#[derive(ConstParamTy, PartialEq, Eq, Clone, Copy)]
pub(crate) enum BoolOp {
  Gte,
  Lte,
  Gt,
  Lt,
}

pub(crate) fn numeric_bool_op_impl<const OP: BoolOp>(
  def_ix: usize,
  a: &Value,
  b: &Value,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let a = a.as_int().unwrap();
      let b = b.as_int().unwrap();
      let result = match OP {
        BoolOp::Gte => a >= b,
        BoolOp::Lte => a <= b,
        BoolOp::Gt => a > b,
        BoolOp::Lt => a < b,
      };
      Ok(Value::Bool(result))
    }
    1 => {
      let a = a.as_float().unwrap();
      let b = b.as_float().unwrap();
      let result = match OP {
        BoolOp::Gte => a >= b,
        BoolOp::Lte => a <= b,
        BoolOp::Gt => a > b,
        BoolOp::Lt => a < b,
      };
      Ok(Value::Bool(result))
    }
    _ => unimplemented!(),
  }
}

fn eval_numeric_bool_op(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
  op: BoolOp,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let a = arg_refs[0].resolve(args, &kwargs).as_int().unwrap();
      let b = arg_refs[1].resolve(args, &kwargs).as_int().unwrap();
      let result = match op {
        BoolOp::Gte => a >= b,
        BoolOp::Lte => a <= b,
        BoolOp::Gt => a > b,
        BoolOp::Lt => a < b,
      };
      Ok(Value::Bool(result))
    }
    1 => {
      let a = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      let b = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
      let result = match op {
        BoolOp::Gte => a >= b,
        BoolOp::Lte => a <= b,
        BoolOp::Gt => a > b,
        BoolOp::Lt => a < b,
      };
      Ok(Value::Bool(result))
    }
    _ => unimplemented!(),
  }
}

pub(crate) fn add_impl(def_ix: usize, lhs: &Value, rhs: &Value) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      // vec3 + vec3
      let a = lhs.as_vec3().unwrap();
      let b = rhs.as_vec3().unwrap();
      Ok(Value::Vec3(*a + *b))
    }
    1 => {
      // float + float
      let a = lhs.as_float().unwrap();
      let b = rhs.as_float().unwrap();
      Ok(Value::Float(a + b))
    }
    2 => {
      // float + int
      let a = lhs.as_float().unwrap();
      let b = rhs.as_int().unwrap();
      Ok(Value::Float(a + b as f32))
    }
    3 => {
      // int + int
      let a = lhs.as_int().unwrap();
      let b = rhs.as_int().unwrap();
      Ok(Value::Int(a + b))
    }
    4 => {
      // combine meshes w/o boolean operation
      let lhs = lhs.as_mesh().unwrap();
      let rhs = rhs.as_mesh().unwrap();

      let mut combined = (*lhs.mesh).clone();
      let mut new_vtx_key_by_old: FxHashMap<VertexKey, VertexKey> = FxHashMap::default();
      for face in rhs.mesh.faces.values() {
        let new_vtx_keys = std::array::from_fn(|i| {
          let old_vtx_key = face.vertices[i];
          *new_vtx_key_by_old.entry(old_vtx_key).or_insert_with(|| {
            let old_vtx = &rhs.mesh.vertices[old_vtx_key];

            // transform from rhs local space -> world space -> lhs local space
            let transformed_pos =
              lhs.transform.try_inverse().unwrap() * rhs.transform * old_vtx.position.push(1.);

            let new_vtx_key = combined.vertices.insert(Vertex {
              position: transformed_pos.xyz(),
              displacement_normal: old_vtx.displacement_normal,
              shading_normal: old_vtx.shading_normal,
              edges: Vec::new(),
            });
            new_vtx_key
          })
        });
        combined.add_face(new_vtx_keys, ());
      }

      let maybe_combined_aabb = match (&*lhs.aabb.borrow(), &*rhs.aabb.borrow()) {
        (Some(lhs_aabb), Some(rhs_aabb)) => Some(Aabb {
          mins: Point3::new(
            lhs_aabb.mins.x.min(rhs_aabb.mins.x),
            lhs_aabb.mins.y.min(rhs_aabb.mins.y),
            lhs_aabb.mins.z.min(rhs_aabb.mins.z),
          ),
          maxs: Point3::new(
            lhs_aabb.maxs.x.max(rhs_aabb.maxs.x),
            lhs_aabb.maxs.y.max(rhs_aabb.maxs.y),
            lhs_aabb.maxs.z.max(rhs_aabb.maxs.z),
          ),
        }),
        _ => None,
      };

      Ok(Value::Mesh(Rc::new(MeshHandle {
        mesh: Rc::new(combined),
        transform: lhs.transform,
        manifold_handle: Rc::new(ManifoldHandle::new(0)),
        aabb: RefCell::new(maybe_combined_aabb),
        trimesh: RefCell::new(None),
        material: lhs.material.clone(),
      })))
    }
    5 => translate_impl(
      0,
      &[ArgRef::Positional(1), ArgRef::Positional(0)],
      &[lhs.clone(), rhs.clone()],
      &Default::default(),
    ),
    // vec3 + float
    6 => {
      let a = lhs.as_vec3().unwrap();
      let b = rhs.as_float().unwrap();
      Ok(Value::Vec3(a + Vec3::new(b, b, b)))
    }
    _ => unimplemented!(),
  }
}

pub(crate) fn sub_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  lhs: &Value,
  rhs: &Value,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      // vec3 - vec3
      let a = lhs.as_vec3().unwrap();
      let b = rhs.as_vec3().unwrap();
      Ok(Value::Vec3(*a - *b))
    }
    1 => {
      // float - float
      let a = lhs.as_float().unwrap();
      let b = rhs.as_float().unwrap();
      Ok(Value::Float(a - b))
    }
    2 => {
      // float - int
      let a = lhs.as_float().unwrap();
      let b = rhs.as_int().unwrap();
      Ok(Value::Float(a - b as f32))
    }
    3 => {
      // int - int
      let a = lhs.as_int().unwrap();
      let b = rhs.as_int().unwrap();
      Ok(Value::Int(a - b))
    }
    4 => {
      // mesh - mesh
      eval_mesh_boolean(
        0,
        &[ArgRef::Positional(0), ArgRef::Positional(1)],
        &[lhs.clone(), rhs.clone()],
        &Default::default(),
        ctx,
        MeshBooleanOp::Difference,
      )
    }
    5 => translate_impl(
      0,
      &[ArgRef::Positional(1), ArgRef::Positional(0)],
      &[lhs.clone(), Value::Vec3(-rhs.as_vec3().unwrap())],
      &Default::default(),
    ),
    // vec3 - float
    6 => {
      let a = lhs.as_vec3().unwrap();
      let b = rhs.as_float().unwrap();
      Ok(Value::Vec3(a - Vec3::new(b, b, b)))
    }
    _ => unimplemented!(),
  }
}

pub(crate) fn mul_impl(def_ix: usize, lhs: &Value, rhs: &Value) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      // vec3 * vec3
      let a = lhs.as_vec3().unwrap();
      let b = rhs.as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(a.x * b.x, a.y * b.y, a.z * b.z)))
    }
    1 => {
      // vec3 * float
      let a = lhs.as_vec3().unwrap();
      let b = rhs.as_float().unwrap();
      Ok(Value::Vec3(a * b))
    }
    2 => {
      // float * float
      let a = lhs.as_float().unwrap();
      let b = rhs.as_float().unwrap();
      Ok(Value::Float(a * b))
    }
    3 => {
      // float * int
      let a = lhs.as_float().unwrap();
      let b = rhs.as_int().unwrap();
      Ok(Value::Float(a * b as f32))
    }
    4 => {
      // int * int
      let a = lhs.as_int().unwrap();
      let b = rhs.as_int().unwrap();
      Ok(Value::Int(a * b))
    }
    // mesh * num, mesh * vec3
    5 | 6 => scale_impl(
      1,
      &[ArgRef::Positional(1), ArgRef::Positional(0)],
      &[lhs.clone(), rhs.clone()],
      &Default::default(),
    ),
    // vec2 * vec2
    7 => {
      let a = lhs.as_vec2().unwrap();
      let b = rhs.as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(a.x * b.x, a.y * b.y)))
    }
    // vec2 * float
    8 => {
      let a = lhs.as_vec2().unwrap();
      let b = rhs.as_float().unwrap();
      Ok(Value::Vec2(a * b))
    }
    _ => unimplemented!(),
  }
}

pub(crate) fn div_impl(def_ix: usize, lhs: &Value, rhs: &Value) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      // vec3 / vec3
      let a = lhs.as_vec3().unwrap();
      let b = rhs.as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(a.x / b.x, a.y / b.y, a.z / b.z)))
    }
    1 => {
      // vec3 / float
      let a = lhs.as_vec3().unwrap();
      let b = rhs.as_float().unwrap();
      Ok(Value::Vec3(a / b))
    }
    2 => {
      // float / float
      let a = lhs.as_float().unwrap();
      let b = rhs.as_float().unwrap();
      Ok(Value::Float(a / b))
    }
    3 => {
      // float / int
      let a = lhs.as_float().unwrap();
      let b = rhs.as_int().unwrap();
      Ok(Value::Float(a / b as f32))
    }
    4 => {
      // int / int
      let a = lhs.as_int().unwrap();
      let b = rhs.as_int().unwrap();
      // there's basically no reason to do real integer division, so just treating things as
      // floats in this case makes so much more sense
      Ok(Value::Float((a as f32) / (b as f32)))
    }
    _ => unimplemented!(),
  }
}

pub(crate) fn mod_impl(def_ix: usize, lhs: &Value, rhs: &Value) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      // int % int
      let a = lhs.as_int().unwrap();
      let b = rhs.as_int().unwrap();
      Ok(Value::Int(a % b))
    }
    1 => {
      // float % float
      let a = lhs.as_float().unwrap();
      let b = rhs.as_float().unwrap();
      Ok(Value::Float(a % b))
    }
    _ => unimplemented!(),
  }
}

pub(crate) fn eq_impl(def_ix: usize, lhs: &Value, rhs: &Value) -> Result<Value, ErrorStack> {
  match def_ix {
    // int == int
    0 => {
      let a = lhs.as_int().unwrap();
      let b = rhs.as_int().unwrap();
      Ok(Value::Bool(a == b))
    }
    // float == float
    1 => {
      let a = lhs.as_float().unwrap();
      let b = rhs.as_float().unwrap();
      Ok(Value::Bool(a == b))
    }
    // either one of a or b is nil
    2 | 3 => {
      let a_is_nil = lhs.is_nil();
      let b_is_nil = rhs.is_nil();

      Ok(Value::Bool(a_is_nil && b_is_nil))
    }
    // a and b are both bools
    4 => {
      let a = lhs.as_bool().unwrap();
      let b = rhs.as_bool().unwrap();
      Ok(Value::Bool(a == b))
    }
    // a and b are both strings
    5 => {
      let a = lhs.as_str().unwrap();
      let b = rhs.as_str().unwrap();
      Ok(Value::Bool(a == b))
    }
    _ => unimplemented!(),
  }
}

pub(crate) fn neq_impl(def_ix: usize, lhs: &Value, rhs: &Value) -> Result<Value, ErrorStack> {
  match def_ix {
    // int != int
    0 => {
      let a = lhs.as_int().unwrap();
      let b = rhs.as_int().unwrap();
      Ok(Value::Bool(a == b))
    }
    // float != float
    1 => {
      let a = lhs.as_float().unwrap();
      let b = rhs.as_float().unwrap();
      Ok(Value::Bool(a == b))
    }
    // either one of a or b is nil
    2 | 3 => {
      let a_is_nil = lhs.is_nil();
      let b_is_nil = rhs.is_nil();

      Ok(Value::Bool(!(a_is_nil && b_is_nil)))
    }
    // a and b are both bools
    4 => {
      let a = lhs.as_bool().unwrap();
      let b = rhs.as_bool().unwrap();
      Ok(Value::Bool(a != b))
    }
    // a and b are both strings
    5 => {
      let a = lhs.as_str().unwrap();
      let b = rhs.as_str().unwrap();
      Ok(Value::Bool(a != b))
    }
    _ => unimplemented!(),
  }
}

pub(crate) fn and_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  lhs: &Value,
  rhs: &Value,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let a = lhs.as_bool().unwrap();
      let b = rhs.as_bool().unwrap();
      Ok(Value::Bool(a && b))
    }
    1 => eval_mesh_boolean(
      0,
      &[ArgRef::Positional(0), ArgRef::Positional(1)],
      &[lhs.clone(), rhs.clone()],
      &Default::default(),
      ctx,
      MeshBooleanOp::Intersection,
    ),
    _ => unimplemented!(),
  }
}

pub(crate) fn or_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  lhs: &Value,
  rhs: &Value,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let a = lhs.as_bool().unwrap();
      let b = rhs.as_bool().unwrap();
      Ok(Value::Bool(a || b))
    }
    1 => eval_mesh_boolean(
      0,
      &[ArgRef::Positional(0), ArgRef::Positional(1)],
      &[lhs.clone(), rhs.clone()],
      &Default::default(),
      ctx,
      MeshBooleanOp::Union,
    ),
    _ => unimplemented!(),
  }
}

pub(crate) fn bit_and_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  lhs: &Value,
  rhs: &Value,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let a = lhs.as_int().unwrap();
      let b = rhs.as_int().unwrap();
      Ok(Value::Int(a & b))
    }
    1 => eval_mesh_boolean(
      0,
      &[ArgRef::Positional(0), ArgRef::Positional(1)],
      &[lhs.clone(), rhs.clone()],
      &Default::default(),
      ctx,
      MeshBooleanOp::Intersection,
    ),
    _ => unimplemented!(),
  }
}

pub(crate) fn bit_or_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  lhs: &Value,
  rhs: &Value,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let a = lhs.as_int().unwrap();
      let b = rhs.as_int().unwrap();
      Ok(Value::Int(a | b))
    }
    1 => eval_mesh_boolean(
      0,
      &[ArgRef::Positional(0), ArgRef::Positional(1)],
      &[lhs.clone(), rhs.clone()],
      &Default::default(),
      ctx,
      MeshBooleanOp::Union,
    ),
    _ => unimplemented!(),
  }
}

pub(crate) fn map_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  fn_value: &Value,
  seq: &Value,
) -> Result<Value, ErrorStack> {
  match def_ix {
    // map(fn, seq)
    0 => {
      let fn_value = fn_value.as_callable().unwrap();
      let seq = seq.as_sequence().unwrap();

      Ok(Value::Sequence(Box::new(MapSeq {
        cb: fn_value.clone(),
        inner: seq.clone_box(),
      })))
    }
    // map(fn, mesh), alias for warp
    1 => warp_impl(
      ctx,
      0,
      &[ArgRef::Positional(0), ArgRef::Positional(1)],
      &[fn_value.clone(), seq.clone()],
      &Default::default(),
    ),
    _ => unimplemented!(),
  }
}

pub(crate) fn warp_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let warp_fn = arg_refs[0].resolve(args, &kwargs).as_callable().unwrap();
      let mesh = arg_refs[1].resolve(args, &kwargs).as_mesh().unwrap();

      let mut needs_displacement_normals_computed = false;
      let mut new_mesh = (*mesh.mesh).clone();
      if let Some(v) = new_mesh.vertices.values().next() {
        if v.displacement_normal.is_none() {
          needs_displacement_normals_computed = true
        }
      }
      if needs_displacement_normals_computed {
        new_mesh.compute_vertex_displacement_normals();
      }

      for vtx in new_mesh.vertices.values_mut() {
        let warped_pos = ctx
          .invoke_callable(
            warp_fn,
            &[
              Value::Vec3(vtx.position),
              Value::Vec3(vtx.displacement_normal.unwrap_or(Vec3::zeros())),
            ],
            &Default::default(),
            &ctx.globals,
          )
          .map_err(|err| err.wrap("error calling warp cb"))?;
        let warped_pos = warped_pos.as_vec3().ok_or_else(|| {
          ErrorStack::new(format!(
            "warp callback must return Vec3, got: {warped_pos:?}",
          ))
        })?;
        vtx.position = *warped_pos;
      }

      Ok(Value::Mesh(Rc::new(MeshHandle {
        mesh: Rc::new(new_mesh),
        transform: mesh.transform.clone(),
        manifold_handle: Rc::new(ManifoldHandle::new(0)),
        aabb: RefCell::new(None),
        trimesh: RefCell::new(None),
        material: mesh.material.clone(),
      })))
    }
    _ => unimplemented!(),
  }
}

pub(crate) fn neg_impl(def_ix: usize, val: &Value) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      // negate int
      let value = val.as_int().unwrap();
      Ok(Value::Int(-value))
    }
    1 => {
      // negate float
      let value = val.as_float().unwrap();
      Ok(Value::Float(-value))
    }
    2 => {
      // negate vec3
      let value = val.as_vec3().unwrap();
      Ok(Value::Vec3(-*value))
    }
    3 => {
      // negate bool
      let value = val.as_bool().unwrap();
      Ok(Value::Bool(!value))
    }
    _ => unimplemented!(),
  }
}

pub(crate) fn pos_impl(def_ix: usize, val: &Value) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      // pass through numeric value
      Ok(val.clone())
    }
    1 => {
      // pass through vec3
      Ok(val.clone())
    }
    _ => unimplemented!(),
  }
}

pub(crate) fn not_impl(def_ix: usize, val: &Value) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = val.as_bool().unwrap();
      Ok(Value::Bool(!value))
    }
    _ => unimplemented!(),
  }
}

pub(crate) fn translate_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  let (translation, obj) = match def_ix {
    0 => {
      let translation = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      let obj = arg_refs[1].resolve(args, &kwargs);
      (*translation, obj)
    }
    1 => {
      let x = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      let y = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
      let z = arg_refs[2].resolve(args, &kwargs).as_float().unwrap();
      let translation = Vec3::new(x, y, z);
      let obj = arg_refs[3].resolve(args, &kwargs);
      (translation, obj)
    }
    _ => unimplemented!(),
  };

  match obj {
    Value::Mesh(mesh) => {
      let mut mesh = (**mesh).clone(true, false, false);
      mesh.transform.append_translation_mut(&translation);

      Ok(Value::Mesh(Rc::new(mesh)))
    }
    Value::Light(light) => {
      let mut light = (**light).clone();
      light.transform_mut().append_translation_mut(&translation);
      Ok(Value::Light(Box::new(light)))
    }
    _ => unreachable!(),
  }
}

pub(crate) fn scale_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  let (scale, mesh) = match def_ix {
    0 => {
      let x = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      let y = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
      let z = arg_refs[2].resolve(args, &kwargs).as_float().unwrap();
      (Vec3::new(x, y, z), arg_refs[3].resolve(args, &kwargs))
    }
    1 => {
      let val = arg_refs[0].resolve(args, &kwargs);
      let scale = match val {
        Value::Vec3(scale) => *scale,
        Value::Float(scale) => Vec3::new(*scale, *scale, *scale),
        Value::Int(scale) => {
          let scale = *scale as f32;
          Vec3::new(scale, scale, scale)
        }
        _ => {
          return Err(ErrorStack::new(format!(
            "Invalid argument for scale: expected Vec3 or Float, found {val:?}",
          )))
        }
      };

      let mesh = arg_refs[1].resolve(args, &kwargs);
      (scale, mesh)
    }
    _ => unimplemented!(),
  };

  let mut mesh = mesh.as_mesh().unwrap().clone(true, false, false);
  mesh.transform.m11 *= scale.x;
  mesh.transform.m22 *= scale.y;
  mesh.transform.m33 *= scale.z;

  Ok(Value::Mesh(mesh.into()))
}

fn spot_light_impl(
  def_ix: usize,
  _arg_refs: &[ArgRef],
  _args: &[Value],
  _kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      todo!()
    }
    _ => unimplemented!(),
  }
}

fn point_light_impl(
  def_ix: usize,
  _arg_refs: &[ArgRef],
  _args: &[Value],
  _kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      todo!()
    }
    _ => unimplemented!(),
  }
}

fn ambient_light_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let color = arg_refs[0].resolve(args, &kwargs); // vec3 or int
      let intensity = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();

      let light = AmbientLight::new(color, intensity)
        .map_err(|err| ErrorStack::new(format!("Error creating ambient light: {err}")))?;
      Ok(Value::Light(Box::new(Light::Ambient(light))))
    }
    _ => unimplemented!(),
  }
}

fn dir_light_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let target = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      let color = arg_refs[1].resolve(args, &kwargs); // vec3 or int
      let intensity = arg_refs[2].resolve(args, &kwargs).as_float().unwrap();
      let cast_shadow = arg_refs[3].resolve(args, &kwargs).as_bool().unwrap();
      let shadow_map_size = arg_refs[4].resolve(args, &kwargs); // map or int
      let shadow_map_radius = arg_refs[5].resolve(args, &kwargs).as_float().unwrap();
      let shadow_map_blur_samples = arg_refs[6].resolve(args, &kwargs).as_int().unwrap() as usize;
      let shadow_map_type = arg_refs[7].resolve(args, &kwargs).as_str().unwrap();
      let shadow_map_bias = arg_refs[8].resolve(args, &kwargs).as_float().unwrap();
      let shadow_camera = arg_refs[9].resolve(args, &kwargs).as_map().unwrap();
      let light = DirectionalLight::new(
        target,
        color,
        intensity,
        cast_shadow,
        shadow_map_size,
        shadow_map_radius,
        shadow_map_blur_samples,
        shadow_map_type,
        shadow_map_bias,
        shadow_camera,
      )
      .map_err(|err| ErrorStack::new(format!("Error creating directional light: {err}")))?;
      Ok(Value::Light(Box::new(Light::Directional(light))))
    }
    _ => unimplemented!(),
  }
}

fn set_material_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let material = arg_refs[0]
        .resolve(args, &kwargs)
        .as_material(ctx)
        .unwrap()?;
      let mesh = arg_refs[1].resolve(args, &kwargs).as_mesh().unwrap();

      let mut mesh_handle = mesh.clone(true, true, true);
      mesh_handle.material = Some(material.clone());

      Ok(Value::Mesh(Rc::new(mesh_handle)))
    }
    _ => unimplemented!(),
  }
}

fn set_default_material_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let material = arg_refs[0]
        .resolve(args, &kwargs)
        .as_material(ctx)
        .unwrap()?;
      ctx.default_material.replace(Some(material.clone()));
      Ok(Value::Nil)
    }
    _ => unimplemented!(),
  }
}

fn mesh_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let verts = arg_refs[0]
        .resolve(args, &kwargs)
        .as_sequence()
        .unwrap()
        .clone_box();
      let indices = arg_refs[1]
        .resolve(args, &kwargs)
        .as_sequence()
        .unwrap()
        .clone_box();

      let verts: Vec<Vec3> = verts
        .consume(ctx)
        .map(|v| -> Result<Vec3, ErrorStack> {
          v?.as_vec3().copied().ok_or_else(|| {
            ErrorStack::new(
              "`verts` sequence produced invalid value in call to `mesh`.  Expected Vec3, found: \
               {v:?}",
            )
          })
        })
        .collect::<Result<_, _>>()?;
      let indices: Vec<u32> = indices
        .consume(ctx)
        .enumerate()
        .map(|(ix, v)| -> Result<u32, ErrorStack> {
          let i = v?.as_int().ok_or_else(|| {
            ErrorStack::new(
              "`indices` sequence produced invalid value in call to `mesh`.  Expected usize, \
               found: {v:?}",
            )
          })?;
          if i < 0 {
            return Err(ErrorStack::new(format!(
              "Found negative vtx ix in element {ix} of `indices` sequence passed to `mesh`: {i}",
            )));
          } else if i >= verts.len() as i64 {
            return Err(ErrorStack::new(format!(
              "Found vtx ix {i} in element {ix} of `indices` sequence passed to `mesh`, but there \
               are only {} vertices in the `verts` sequence",
              verts.len()
            )));
          }

          Ok(i as u32)
        })
        .collect::<Result<_, _>>()?;

      if indices.len() % 3 != 0 {
        return Err(ErrorStack::new(format!(
          "Indices sequence passed to `mesh` must have a length that is a multiple of 3 as each \
           set of 3 indices defines a face; found: {}",
          indices.len()
        )));
      }

      let mesh = LinkedMesh::from_indexed_vertices(&verts, &indices, None, None);
      Ok(Value::Mesh(Rc::new(MeshHandle {
        mesh: Rc::new(mesh),
        transform: Matrix4::identity(),
        manifold_handle: Rc::new(ManifoldHandle::new(0)),
        aabb: RefCell::new(None),
        trimesh: RefCell::new(None),
        material: None,
      })))
    }
    _ => unimplemented!(),
  }
}

fn call_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let callable = arg_refs[0].resolve(args, &kwargs).as_callable().unwrap();
      ctx.invoke_callable(callable, &[], &Default::default(), &ctx.globals)
    }
    1 => {
      let callable = arg_refs[0].resolve(args, &kwargs).as_callable().unwrap();
      let call_args = arg_refs[1]
        .resolve(args, &kwargs)
        .as_sequence()
        .unwrap()
        .clone_box();
      let args = call_args.consume(ctx).collect::<Result<Vec<_>, _>>()?;
      ctx.invoke_callable(callable, &args, &Default::default(), &ctx.globals)
    }
    _ => unimplemented!(),
  }
}

fn fbm_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let pos = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      Ok(Value::Float(fbm(0, 4, 1., 0.5, 2., *pos)))
    }
    1 => {
      let seed = arg_refs[0].resolve(args, &kwargs).as_int().unwrap();
      if seed < 0 || seed > u32::MAX as i64 {
        return Err(ErrorStack::new(format!(
          "Seed for fbm must be in range [0, {}], found: {seed}",
          u32::MAX
        )));
      }
      let seed = seed as u32;
      let octaves = arg_refs[1].resolve(args, &kwargs).as_int().unwrap() as usize;
      let frequency = arg_refs[2].resolve(args, &kwargs).as_float().unwrap();
      let lacunarity = arg_refs[3].resolve(args, &kwargs).as_float().unwrap();
      let persistence = arg_refs[4].resolve(args, &kwargs).as_float().unwrap();
      let pos = arg_refs[5].resolve(args, &kwargs).as_vec3().unwrap();

      Ok(Value::Float(fbm(
        seed,
        octaves,
        frequency,
        persistence,
        lacunarity,
        *pos,
      )))
    }
    _ => unimplemented!(),
  }
}

fn randi_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let min = arg_refs[0].resolve(args, &kwargs).as_int().unwrap();
      let max = arg_refs[1].resolve(args, &kwargs).as_int().unwrap();

      if max < min {
        return Err(ErrorStack::new(format!(
          "Invalid range for rand_int: min ({min}) must be less than or equal to max ({max})"
        )));
      } else if min == max {
        return Ok(Value::Int(min));
      }

      Ok(Value::Int(ctx.rng().gen_range(min..max)))
    }
    1 => Ok(Value::Int(ctx.rng().gen())),
    _ => unimplemented!(),
  }
}

fn randv_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let min = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      let max = arg_refs[1].resolve(args, &kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        ctx.rng().gen_range(min.x..max.x),
        ctx.rng().gen_range(min.y..max.y),
        ctx.rng().gen_range(min.z..max.z),
      )))
    }
    1 => {
      let min = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      let max = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Vec3(Vec3::new(
        ctx.rng().gen_range(min..max),
        ctx.rng().gen_range(min..max),
        ctx.rng().gen_range(min..max),
      )))
    }
    2 => Ok(Value::Vec3(Vec3::new(
      ctx.rng().gen(),
      ctx.rng().gen(),
      ctx.rng().gen(),
    ))),
    _ => unimplemented!(),
  }
}

fn randf_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let min = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      let max = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Float(ctx.rng().gen_range(min..max)))
    }
    1 => Ok(Value::Float(ctx.rng().gen())),
    _ => unimplemented!(),
  }
}

fn verts_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let mesh = arg_refs[0]
        .resolve(args, &kwargs)
        .as_mesh()
        .unwrap()
        .clone(false, false, true);
      Ok(Value::Sequence(Box::new(MeshVertsSeq { mesh })))
    }
    _ => unimplemented!(),
  }
}

fn convex_hull_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let verts_seq = arg_refs[0].resolve(args, &kwargs).as_sequence().unwrap();
      let verts = verts_seq
        .clone_box()
        .consume(ctx)
        .map(|res| match res {
          Ok(Value::Vec3(v)) => Ok(v),
          Ok(val) => Err(ErrorStack::new(format!(
            "Expected Vec3 in sequence passed to `convex_hull`, found: {val:?}"
          ))),
          Err(err) => Err(err),
        })
        .collect::<Result<Vec<_>, _>>()?;
      let out_mesh = convex_hull_from_verts(&verts)
        .map_err(|err| ErrorStack::new(err).wrap("Error in `convex_hull` function"))?;
      Ok(Value::Mesh(Rc::new(out_mesh)))
    }
    _ => unimplemented!(),
  }
}

fn simplify_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let tolerance = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      let mesh = arg_refs[1].resolve(args, &kwargs).as_mesh().unwrap();

      let out_mesh_handle =
        simplify_mesh(&mesh, tolerance).map_err(|err| err.wrap("Error in `simplify` function"))?;
      Ok(Value::Mesh(Rc::new(out_mesh_handle)))
    }
    _ => unimplemented!(),
  }
}

fn fan_fill_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let path = arg_refs[0]
        .resolve(args, &kwargs)
        .as_sequence()
        .unwrap()
        .clone_box()
        .consume(ctx)
        .map(|res| match res {
          Ok(Value::Vec3(v)) => Ok(v),
          Ok(val) => Err(ErrorStack::new(format!(
            "Expected Vec3 in sequence passed to `fan_fill`, found: {val:?}"
          ))),
          Err(err) => Err(err),
        })
        .collect::<Result<Vec<_>, _>>()?;
      let closed = arg_refs[1].resolve(args, &kwargs).as_bool().unwrap();
      let flipped = arg_refs[2].resolve(args, &kwargs).as_bool().unwrap();
      let center = match arg_refs[3].resolve(args, &kwargs) {
        Value::Vec3(v) => Some(*v),
        Value::Nil => None,
        _ => None,
      };

      let mesh =
        fan_fill(&path, closed, flipped, center).map_err(|err| err.wrap("Error in `fan_fill`"))?;
      Ok(Value::Mesh(Rc::new(MeshHandle {
        mesh: Rc::new(mesh),
        transform: Matrix4::identity(),
        manifold_handle: Rc::new(ManifoldHandle::new(0)),
        aabb: RefCell::new(None),
        trimesh: RefCell::new(None),
        material: None,
      })))
    }
    _ => unimplemented!(),
  }
}

fn stitch_contours_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let mut contours = arg_refs[0]
        .resolve(args, &kwargs)
        .as_sequence()
        .unwrap()
        .clone_box()
        .consume(ctx)
        .enumerate()
        .map(|(contour_ix, res)| match res {
          Ok(Value::Sequence(seq)) => Ok(seq.consume(ctx)),
          Ok(val) => Err(ErrorStack::new(format!(
            "Expected sequence of sequences in `stitch_contours`, found: {val:?} at contour index \
             {contour_ix}",
          ))),
          Err(err) => Err(err.wrap(format!(
            "Error evaluating contour sequence at index {contour_ix} in `stitch_contours`",
          ))),
        })
        .collect::<Result<Vec<_>, _>>()?;
      let flipped = arg_refs[1].resolve(args, &kwargs).as_bool().unwrap();
      let closed = arg_refs[2].resolve(args, &kwargs).as_bool().unwrap();
      let cap_start = arg_refs[3].resolve(args, &kwargs).as_bool().unwrap();
      let cap_end = arg_refs[4].resolve(args, &kwargs).as_bool().unwrap();
      let cap_ends = arg_refs[5].resolve(args, &kwargs).as_bool().unwrap();

      let cap_start = cap_ends || cap_start;
      let cap_end = cap_ends || cap_end;

      let mesh = stitch_contours(&mut contours, flipped, closed, cap_start, cap_end)
        .map_err(|err| err.wrap("Error in `stitch_contours`"))?;
      Ok(Value::Mesh(Rc::new(MeshHandle {
        mesh: Rc::new(mesh),
        transform: Matrix4::identity(),
        manifold_handle: Rc::new(ManifoldHandle::new(0)),
        aabb: RefCell::new(None),
        trimesh: RefCell::new(None),
        material: None,
      })))
    }
    _ => unimplemented!(),
  }
}

fn trace_geodesic_path_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let path = arg_refs[0]
        .resolve(args, &kwargs)
        .as_sequence()
        .unwrap()
        .clone_box()
        .consume(ctx);
      let mesh = arg_refs[1].resolve(args, &kwargs).as_mesh().unwrap();
      let world_space = arg_refs[2].resolve(args, &kwargs).as_bool().unwrap();
      let full_path = arg_refs[3].resolve(args, &kwargs).as_bool().unwrap();
      let start_pos_local_space = arg_refs[4].resolve(args, &kwargs).as_vec3();
      let start_pos_local_space = start_pos_local_space
        .map(|v| v.as_slice())
        .unwrap_or_default();
      let up_dir_world_space = arg_refs[5].resolve(args, &kwargs).as_vec3();
      let up_dir_world_space = up_dir_world_space.map(|v| v.as_slice()).unwrap_or_default();

      let path = path
        .map(|res| match res {
          Ok(Value::Vec2(v)) => Ok(v),
          Ok(val) => Err(ErrorStack::new(format!(
            "Expected Vec2 in sequence passed to `trace_geodesic_path`, found: {val:?}"
          ))),
          Err(err) => Err(err),
        })
        .collect::<Result<Vec<_>, _>>()?;
      let path_slice: &[f32] =
        unsafe { std::slice::from_raw_parts(path.as_ptr() as *const f32, path.len() * 2) };

      let OwnedIndexedMesh {
        vertices, indices, ..
      } = mesh.mesh.to_raw_indexed(false, false, true);
      let mut out_points = if std::mem::size_of::<usize>() == std::mem::size_of::<u32>() {
        let indices = unsafe { std::mem::transmute::<Vec<usize>, Vec<u32>>(indices) };
        trace_geodesic_path(
          &vertices,
          &indices,
          path_slice,
          full_path,
          start_pos_local_space,
          up_dir_world_space,
        )
      } else {
        let indices: Vec<u32> = indices.iter().map(|&ix| ix as u32).collect();
        trace_geodesic_path(
          &vertices,
          &indices,
          path_slice,
          full_path,
          start_pos_local_space,
          up_dir_world_space,
        )
      };

      if out_points.len() == 1 {
        let err = get_geodesic_error();
        return Err(ErrorStack::new(err).wrap("Error calling geodesic path tracing kernel"));
      }

      assert_eq!(out_points.len() % 3, 0);
      if out_points.capacity() % 3 != 0 {
        out_points.shrink_to_fit();
      }
      let mut out_points_v3: Vec<Vec3> = unsafe {
        Vec::from_raw_parts(
          out_points.as_ptr() as *mut Vec3,
          out_points.len() / 3,
          out_points.capacity() / 3,
        )
      };
      std::mem::forget(out_points);

      if world_space {
        for vtx in &mut out_points_v3 {
          *vtx = (mesh.transform * vtx.push(1.)).xyz();
        }
      }

      Ok(Value::Sequence(Box::new(IteratorSeq {
        inner: out_points_v3.into_iter().map(|v| Ok(Value::Vec3(v))),
      })))
    }
    _ => unimplemented!(),
  }
}

fn extrude_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let up = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      let mesh = arg_refs[1].resolve(args, &kwargs).as_mesh().unwrap();
      let mut out_mesh = (*mesh.mesh).clone();

      extrude(&mut out_mesh, *up);

      Ok(Value::Mesh(Rc::new(MeshHandle {
        mesh: Rc::new(out_mesh),
        transform: mesh.transform.clone(),
        manifold_handle: Rc::new(ManifoldHandle::new(0)),
        aabb: RefCell::new(None),
        trimesh: RefCell::new(None),
        material: mesh.material.clone(),
      })))
    }
    _ => unimplemented!(),
  }
}

fn torus_knot_path_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let radius = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      let tube_radius = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
      let p = arg_refs[2].resolve(args, &kwargs).as_int().unwrap() as usize;
      let q = arg_refs[3].resolve(args, &kwargs).as_int().unwrap() as usize;
      let count = arg_refs[4].resolve(args, &kwargs).as_int().unwrap() as usize;

      Ok(Value::Sequence(Box::new(IteratorSeq {
        inner: build_torus_knot_path(radius, tube_radius, p, q, count).map(|v| Ok(Value::Vec3(v))),
      })))
    }
    _ => unreachable!(),
  }
}

fn extrude_pipe_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let radius = arg_refs[0].resolve(args, &kwargs);
      let resolution = arg_refs[1].resolve(args, &kwargs).as_int().unwrap() as usize;
      let path = arg_refs[2].resolve(args, &kwargs).as_sequence().unwrap();
      let close_ends = arg_refs[3].resolve(args, &kwargs).as_bool().unwrap();
      let connect_ends = arg_refs[4].resolve(args, &kwargs).as_bool().unwrap();

      enum Twist<'a> {
        Const(f32),
        Dyn(&'a Callable),
      }

      let twist = match arg_refs[5].resolve(args, &kwargs) {
        Value::Float(f) => Twist::Const(*f),
        Value::Int(i) => Twist::Const(*i as f32),
        Value::Callable(cb) => Twist::Dyn(cb),
        _ => {
          return Err(ErrorStack::new(format!(
            "Invalid twist argument for `extrude_pipe`; expected Numeric or Callable, found: {:?}",
            arg_refs[4].resolve(args, &kwargs)
          )))
        }
      };

      fn build_twist_or_radius_callable<'a>(
        ctx: &'a EvalCtx,
        get_twist: &'a Callable,
        param_name: &'static str,
      ) -> impl Fn(usize, Vec3) -> Result<f32, ErrorStack> + 'a {
        move |i, pos| {
          let out = ctx
            .invoke_callable(
              get_twist,
              &[Value::Int(i as i64), Value::Vec3(pos)],
              &Default::default(),
              &ctx.globals,
            )
            .map_err(|err| {
              err.wrap(format!("Error calling `{param_name}` cb in `extrude_pipe`"))
            })?;
          out.as_float().ok_or_else(|| {
            ErrorStack::new(format!(
              "Expected Float from `{param_name}` cb in `extrude_pipe`, found: {out:?}"
            ))
          })
        }
      }

      let path = path.clone_box().consume(ctx).map(|res| match res {
        Ok(Value::Vec3(v)) => Ok(v),
        Ok(val) => Err(ErrorStack::new(format!(
          "Expected Vec3 in path seq passed to `extrude_pipe`, found: {val:?}"
        ))),
        Err(err) => Err(err),
      });

      let end_mode = if connect_ends {
        EndMode::Connect
      } else if close_ends {
        EndMode::Close
      } else {
        EndMode::Open
      };

      let mesh = match radius {
        _ if let Some(radius) = radius.as_float() => {
          let get_radius = |_, _| Ok(radius);
          match twist {
            Twist::Const(twist) => {
              extrude_pipe(get_radius, resolution, path, end_mode, |_, _| Ok(twist))?
            }
            Twist::Dyn(get_twist) => extrude_pipe(
              get_radius,
              resolution,
              path,
              end_mode,
              build_twist_or_radius_callable(ctx, get_twist, "twist"),
            )?,
          }
        }
        _ if let Some(get_radius) = radius.as_callable() => {
          let get_radius = build_twist_or_radius_callable(ctx, get_radius, "radius");
          match twist {
            Twist::Const(twist) => {
              extrude_pipe(get_radius, resolution, path, end_mode, |_, _| Ok(twist))?
            }
            Twist::Dyn(get_twist) => extrude_pipe(
              get_radius,
              resolution,
              path,
              end_mode,
              build_twist_or_radius_callable(ctx, get_twist, "twist"),
            )?,
          }
        }
        _ => {
          return Err(ErrorStack::new(format!(
            "Invalid radius argument for `extrude_pipe`; expected Float, Int, or Callable, found: \
             {radius:?}",
          )))
        }
      };
      Ok(Value::Mesh(Rc::new(MeshHandle {
        mesh: Rc::new(mesh),
        transform: Matrix4::identity(),
        manifold_handle: Rc::new(ManifoldHandle::new(0)),
        aabb: RefCell::new(None),
        trimesh: RefCell::new(None),
        material: None,
      })))
    }
    _ => unimplemented!(),
  }
}

fn bezier3d_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let p0 = *arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      let p1 = *arg_refs[1].resolve(args, &kwargs).as_vec3().unwrap();
      let p2 = *arg_refs[2].resolve(args, &kwargs).as_vec3().unwrap();
      let p3 = *arg_refs[3].resolve(args, &kwargs).as_vec3().unwrap();
      let count = arg_refs[4].resolve(args, &kwargs).as_int().unwrap() as usize;

      let curve = cubic_bezier_3d_path(p0, p1, p2, p3, count).map(|v| Ok(Value::Vec3(v)));
      Ok(Value::Sequence(Box::new(IteratorSeq { inner: curve })))
    }
    _ => unimplemented!(),
  }
}

fn superellipse_path_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let width = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      let height = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
      let n = arg_refs[2].resolve(args, &kwargs).as_float().unwrap();
      let point_count = arg_refs[3].resolve(args, &kwargs).as_int().unwrap() as usize;

      let curve = superellipse_path(width, height, n, point_count).map(|v| Ok(Value::Vec2(v)));
      Ok(Value::Sequence(Box::new(IteratorSeq { inner: curve })))
    }
    _ => unimplemented!(),
  }
}

fn normalize_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let v = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(v.normalize()))
    }
    _ => unimplemented!(),
  }
}

fn distance_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let a = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      let b = arg_refs[1].resolve(args, &kwargs).as_vec3().unwrap();
      Ok(Value::Float((*a - *b).magnitude()))
    }
    1 => {
      let a = arg_refs[0].resolve(args, &kwargs).as_vec2().unwrap();
      let b = arg_refs[1].resolve(args, &kwargs).as_vec2().unwrap();
      Ok(Value::Float((*a - *b).magnitude()))
    }
    _ => unimplemented!(),
  }
}

fn len_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let v = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      Ok(Value::Float(v.magnitude()))
    }
    1 => {
      let v = arg_refs[0].resolve(args, &kwargs).as_vec2().unwrap();
      Ok(Value::Float(v.magnitude()))
    }
    _ => unimplemented!(),
  }
}

fn intersects_ray_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let ray_origin = *arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      let ray_direction = *arg_refs[1].resolve(args, &kwargs).as_vec3().unwrap();
      let mesh = arg_refs[2].resolve(args, &kwargs).as_mesh().unwrap();
      let max_distance = arg_refs[3].resolve(args, &kwargs).as_float();

      let trimesh = mesh
        .get_or_create_trimesh()
        .map_err(|err| ErrorStack::new(format!("Error creating trimesh for raycast: {err}")))?;

      let has_hit = parry3d::query::RayCast::intersects_ray(
        &*trimesh,
        &Isometry::default(),
        &Ray {
          dir: ray_direction,
          origin: Point::new(ray_origin.x, ray_origin.y, ray_origin.z),
        },
        max_distance.unwrap_or(f32::INFINITY),
      );
      Ok(Value::Bool(has_hit))
    }
    _ => unimplemented!(),
  }
}

fn intersects_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let a = arg_refs[0].resolve(args, &kwargs).as_mesh().unwrap();
      let b = arg_refs[1].resolve(args, &kwargs).as_mesh().unwrap();

      if a.mesh.is_empty() || b.mesh.is_empty() {
        return Ok(Value::Bool(false));
      }

      let a_aabb = a.get_or_compute_aabb();
      let b_aabb = b.get_or_compute_aabb();

      if a_aabb.intersection(&b_aabb).is_none() {
        return Ok(Value::Bool(false));
      }

      let a_trimesh = a.get_or_create_trimesh().map_err(|err| {
        ErrorStack::new(format!(
          "Error creating trimesh for mesh `a` in `intersects`: {err}"
        ))
      })?;
      let b_trimesh = b.get_or_create_trimesh().map_err(|err| {
        ErrorStack::new(format!(
          "Error creating trimesh for mesh `b` in `intersects`: {err}"
        ))
      })?;

      let result = parry3d::query::intersection_test(
        &Isometry::default(),
        &*a_trimesh,
        &Isometry::default(),
        &*b_trimesh,
      )
      .unwrap();

      Ok(Value::Bool(result))
    }
    _ => unimplemented!(),
  }
}

fn connected_components_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let mesh_handle = arg_refs[0].resolve(args, &kwargs).as_mesh().unwrap();
      let transform = mesh_handle.transform.clone();
      let mesh = Rc::clone(&mesh_handle.mesh);
      let mut components: Vec<Vec<FaceKey>> = mesh.connected_components();
      components.sort_unstable_by_key(|c| Reverse(c.len()));
      let material = mesh_handle.material.clone();
      Ok(Value::Sequence(Box::new(IteratorSeq {
        inner: components.into_iter().map(move |c| {
          let mut sub_vkey_by_old_vkey: FxHashMap<VertexKey, VertexKey> = FxHashMap::default();
          let mut sub_mesh = LinkedMesh::new(0, c.len(), None);

          let mut map_vtx = |sub_mesh: &mut LinkedMesh<()>, vkey: VertexKey| {
            *sub_vkey_by_old_vkey.entry(vkey).or_insert_with(|| {
              sub_mesh.vertices.insert(Vertex {
                position: mesh.vertices[vkey].position,
                shading_normal: None,
                displacement_normal: None,
                edges: Vec::new(),
              })
            })
          };

          for face_key in c {
            let face = &mesh.faces[face_key];
            let vtx0 = map_vtx(&mut sub_mesh, face.vertices[0]);
            let vtx1 = map_vtx(&mut sub_mesh, face.vertices[1]);
            let vtx2 = map_vtx(&mut sub_mesh, face.vertices[2]);
            sub_mesh.add_face([vtx0, vtx1, vtx2], ());
          }
          Ok(Value::Mesh(Rc::new(MeshHandle {
            mesh: Rc::new(sub_mesh),
            transform,
            manifold_handle: Rc::new(ManifoldHandle::new(0)),
            aabb: RefCell::new(None),
            trimesh: RefCell::new(None),
            material: material.clone(),
          })))
        }),
      })))
    }
    _ => unimplemented!(),
  }
}

fn tessellate_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let target_edge_length = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      if target_edge_length <= 0. {
        return Err(ErrorStack::new("`target_edge_length` must be > 0"));
      }
      let mesh_handle = arg_refs[1].resolve(args, &kwargs).as_mesh().unwrap();
      let transform = mesh_handle.transform.clone();

      let mut mesh = (*mesh_handle.mesh).clone();
      tessellation::tessellate_mesh(
        &mut mesh,
        target_edge_length,
        DisplacementNormalMethod::Interpolate,
      );
      Ok(Value::Mesh(Rc::new(MeshHandle {
        mesh: Rc::new(mesh),
        transform: transform,
        manifold_handle: Rc::new(ManifoldHandle::new(0)),
        aabb: RefCell::new(None),
        trimesh: RefCell::new(None),
        material: mesh_handle.material.clone(),
      })))
    }
    _ => unimplemented!(),
  }
}

fn subdivide_by_plane_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  fn unhandled_transform_error() -> ErrorStack {
    ErrorStack::new(
      "subdivide_by_plane does not currently support meshes with transforms.  Either apply before \
       transforming or use `apply_transforms` to bake the transforms into the mesh vertex \
       positions.",
    )
  }

  match def_ix {
    0 => {
      let plane_normal = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      let plane_offset = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
      let mesh_handle = arg_refs[2].resolve(args, &kwargs).as_mesh().unwrap();

      // TODO: handle transform
      if mesh_handle.transform != Matrix4::identity() {
        return Err(unhandled_transform_error());
      }

      let mut mesh = (*mesh_handle.mesh).clone();
      mesh.subdivide_by_plane(&Plane {
        normal: plane_normal.normalize(),
        w: plane_offset,
      });

      Ok(Value::Mesh(Rc::new(MeshHandle {
        mesh: Rc::new(mesh),
        transform: mesh_handle.transform.clone(),
        manifold_handle: Rc::new(ManifoldHandle::new(0)),
        aabb: RefCell::new(None),
        trimesh: RefCell::new(None),
        material: mesh_handle.material.clone(),
      })))
    }
    1 => {
      let mesh_handle = arg_refs[2].resolve(args, &kwargs).as_mesh().unwrap();
      let plane_normals = arg_refs[0]
        .resolve(args, &kwargs)
        .as_sequence()
        .unwrap()
        .clone_box()
        .consume(&EvalCtx::default())
        .map(|res| match res {
          Ok(Value::Vec3(v)) => Ok(v),
          Ok(val) => Err(ErrorStack::new(format!(
            "Expected Vec3 in sequence passed to `subdivide_by_plane`, found: {val:?}"
          ))),
          Err(err) => Err(err),
        })
        .collect::<Result<Vec<_>, _>>()?;
      let plane_offsets = arg_refs[1]
        .resolve(args, &kwargs)
        .as_sequence()
        .unwrap()
        .clone_box()
        .consume(&EvalCtx::default())
        .map(|res| match res {
          Ok(Value::Float(f)) => Ok(f),
          Ok(Value::Int(i)) => Ok(i as f32),
          Ok(val) => Err(ErrorStack::new(format!(
            "Expected Float in sequence passed to `subdivide_by_plane`, found: {val:?}"
          ))),
          Err(err) => Err(err),
        })
        .collect::<Result<Vec<_>, _>>()?;
      if plane_normals.len() != plane_offsets.len() {
        return Err(ErrorStack::new(format!(
          "Expected same number of normals and offsets in sequence passed to `subdivide_by_plane`,
           found {} normals and {} offsets",
          plane_normals.len(),
          plane_offsets.len()
        )));
      }

      // TODO: handle transform
      if mesh_handle.transform != Matrix4::identity() {
        return Err(unhandled_transform_error());
      }

      let mut mesh = (*mesh_handle.mesh).clone();
      for (normal, offset) in plane_normals.iter().zip(plane_offsets.iter()) {
        mesh.subdivide_by_plane(&Plane {
          normal: normal.normalize(),
          w: *offset,
        });
      }
      Ok(Value::Mesh(Rc::new(MeshHandle {
        mesh: Rc::new(mesh),
        transform: mesh_handle.transform.clone(),
        manifold_handle: Rc::new(ManifoldHandle::new(0)),
        aabb: RefCell::new(None),
        trimesh: RefCell::new(None),
        material: mesh_handle.material.clone(),
      })))
    }
    _ => unimplemented!(),
  }
}

fn split_by_plane_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let plane_normal = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      let plane_offset = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
      let mesh_handle = arg_refs[2].resolve(args, &kwargs).as_mesh().unwrap();

      let (a, b) = split_mesh_by_plane(mesh_handle, *plane_normal, plane_offset)
        .map_err(|err| ErrorStack::new(format!("Error in `split_by_plane`: {err}")))?;

      Ok(Value::Sequence(Box::new(EagerSeq {
        inner: vec![Value::Mesh(Rc::new(a)), Value::Mesh(Rc::new(b))],
      })))
    }
    _ => unimplemented!(),
  }
}

fn compose_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      if !kwargs.is_empty() {
        return Err(ErrorStack::new(
          "compose function does not accept keyword arguments",
        ));
      }

      if args.is_empty() {
        return Err(ErrorStack::new(
          "compose function requires at least one argument",
        ));
      }

      let inner: Vec<Value> = if args.len() == 1 {
        if matches!(args[0], Value::Callable(_)) {
          return Ok(args[0].clone());
        } else if let Some(seq) = args[0].as_sequence() {
          // have to eagerly evaluate the sequence to get the inner callables
          seq
            .clone_box()
            .consume(ctx)
            .collect::<Result<Vec<_>, _>>()?
        } else {
          return Err(ErrorStack::new(format!(
            "compose function requires a sequence or callable if a single arg is provided, found: \
             {:?}",
            args[0]
          )));
        }
      } else {
        args.to_owned()
      };

      let inner = inner
        .into_iter()
        .map(|val| {
          if let Value::Callable(callable) = val {
            Ok(callable)
          } else {
            Err(ErrorStack::new(format!(
              "Non-callable found in sequence passed to compose, found: {val:?}"
            )))
          }
        })
        .collect::<Result<Vec<_>, _>>()?;

      Ok(Value::Callable(Rc::new(Callable::ComposedFn(ComposedFn {
        inner,
      }))))
    }
    _ => unreachable!(),
  }
}

fn point_distribute_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let count = arg_refs[0].resolve(args, &kwargs);
      let point_count = match count {
        _ if let Some(count) = count.as_int() => {
          if count < 0 {
            return Err(ErrorStack::new(
              "negative point count is not valid for point_distribute",
            ));
          }

          Some(count as usize)
        }
        _ if count.is_nil() => None,
        _ => unreachable!(),
      };
      let mesh = arg_refs[1].resolve(args, &kwargs).as_mesh().unwrap();
      let seed = arg_refs[2].resolve(args, &kwargs).as_int().unwrap().abs() as u64;
      let cb = arg_refs[3].resolve(args, &kwargs).as_callable().cloned();
      let world_space = arg_refs[4].resolve(args, &kwargs).as_bool().unwrap();

      let sampler_seq = PointDistributeSeq {
        mesh: mesh.clone(false, false, true),
        point_count,
        seed,
        cb,
        world_space,
      };
      Ok(Value::Sequence(Box::new(sampler_seq)))
    }
    _ => unimplemented!(),
  }
}

fn render_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let Value::Mesh(mesh) = arg_refs[0].resolve(args, &kwargs) else {
        unreachable!()
      };
      ctx.rendered_meshes.push(Rc::clone(mesh));
      Ok(Value::Nil)
    }
    1 => {
      let light = arg_refs[0].resolve(args, &kwargs).as_light().unwrap();
      ctx.rendered_lights.push(light.clone());
      Ok(Value::Nil)
    }
    2 => {
      // This is expected to be a `seq<Vec3> | seq<Mesh | seq<Vec3>>`
      let sequence = arg_refs[0]
        .resolve(args, &kwargs)
        .as_sequence()
        .unwrap()
        .clone_box();

      fn render_path(
        ctx: &EvalCtx,
        iter: impl Iterator<Item = Result<Value, ErrorStack>>,
      ) -> Result<(), ErrorStack> {
        let path = iter
          .map(|res| -> Result<Vec3, ErrorStack> {
            match res {
              Ok(Value::Vec3(v)) => Ok(v),
              Ok(other) => Err(ErrorStack::new(format!(
                "Inner sequence yielded a value of {other:?}; expected a sequence of `Vec3`",
              ))),
              Err(err) => Err(err.wrap("Error evaluating inner sequence in render")),
            }
          })
          .collect::<Result<Vec<_>, _>>()?;
        if path.is_empty() {
          return Ok(());
        }

        ctx.rendered_paths.push(path);
        Ok(())
      }

      let render_single = |res: Result<Value, ErrorStack>| -> Result<(), ErrorStack> {
        match res {
          Ok(Value::Mesh(mesh)) => {
            ctx.rendered_meshes.push(mesh);
          }
          Ok(Value::Sequence(inner_seq)) => {
            let iter = inner_seq.consume(ctx);
            render_path(ctx, iter)?;
          }
          other => {
            return Err(ErrorStack::new(format!(
              "Invalid type yielded from sequence passed to `render`; expected seq<Mesh> or \
               seq<seq<Vec3>> representing a path, found: {other:?}",
            )))
          }
        }
        Ok(())
      };

      let mut iter = sequence.consume(ctx).peekable();
      match iter.peek() {
        // rendering a sequence of meshes and/or paths
        Some(Ok(Value::Mesh(_) | Value::Sequence(_))) | Some(Err(_)) => (),
        // rendering a single top-level path
        Some(Ok(Value::Vec3(_))) => {
          render_path(ctx, iter)?;
          return Ok(Value::Nil);
        }
        Some(Ok(other)) => {
          return Err(ErrorStack::new(format!(
            "Invalid type yielded from sequence passed to `render`; expected sequence of meshes \
             and/or paths (`seq<Mesh | seq<Vec3>>`), found: {other:?}",
          )));
        }
        None => {
          return Err(ErrorStack::new("Empty sequence passed to render function"));
        }
      }

      for res in iter {
        render_single(res)?;
      }
      Ok(Value::Nil)
    }
    _ => unimplemented!(),
  }
}

fn print_impl(
  ctx: &EvalCtx,
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  let formatted_pos_ags = args
    .iter()
    .map(|v| format!("{v:?}"))
    .collect::<Vec<_>>()
    .join(", ");
  let formatted_kwargs = kwargs
    .iter()
    .map(|(k, v)| format!("{k}={v:?}"))
    .collect::<Vec<_>>()
    .join(", ");

  (ctx.log_fn)(&format!("{}, {}", formatted_pos_ags, formatted_kwargs));
  Ok(Value::Nil)
}

fn lerp_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let t = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      let a = arg_refs[1].resolve(args, &kwargs).as_vec3().unwrap();
      let b = arg_refs[2].resolve(args, &kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(a.lerp(b, t)))
    }
    1 => {
      let t = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      let a = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
      let b = arg_refs[2].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Float(a + (b - a) * t))
    }
    _ => unimplemented!(),
  }
}

fn smoothstep_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let edge0 = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      let edge1 = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
      let x = arg_refs[2].resolve(args, &kwargs).as_float().unwrap();
      let t = ((x - edge0) / (edge1 - edge0)).clamp(0., 1.);
      Ok(Value::Float(t * t * (3. - 2. * t)))
    }
    _ => unimplemented!(),
  }
}

fn linearstep_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let edge0 = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      let edge1 = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
      let x = arg_refs[2].resolve(args, &kwargs).as_float().unwrap();
      let t = ((x - edge0) / (edge1 - edge0)).clamp(0., 1.);
      Ok(Value::Float(t))
    }
    _ => unimplemented!(),
  }
}

fn deg2rad_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Float(value.to_radians()))
    }
    _ => unimplemented!(),
  }
}

fn rad2deg_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Float(value.to_degrees()))
    }
    _ => unimplemented!(),
  }
}

fn fix_float_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      if value.is_normal() {
        Ok(Value::Float(value))
      } else {
        Ok(Value::Float(0.))
      }
    }
    1 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        if value.x.is_normal() { value.x } else { 0. },
        if value.y.is_normal() { value.y } else { 0. },
        if value.z.is_normal() { value.z } else { 0. },
      )))
    }
    _ => unimplemented!(),
  }
}

fn round_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Float(value.round()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.round(),
        value.y.round(),
        value.z.round(),
      )))
    }
    _ => unimplemented!(),
  }
}

fn floor_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Float(value.floor()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.floor(),
        value.y.floor(),
        value.z.floor(),
      )))
    }
    _ => unimplemented!(),
  }
}

fn ceil_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Float(value.ceil()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.ceil(),
        value.y.ceil(),
        value.z.ceil(),
      )))
    }
    _ => unimplemented!(),
  }
}

fn fract_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Float(value.fract()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.fract(),
        value.y.fract(),
        value.z.fract(),
      )))
    }
    _ => unimplemented!(),
  }
}

fn trunc_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Float(value.trunc()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.trunc(),
        value.y.trunc(),
        value.z.trunc(),
      )))
    }
    _ => unimplemented!(),
  }
}

fn pow_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let base = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      let exponent = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Float(base.powf(exponent)))
    }
    1 => {
      let base = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      let exponent = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Vec3(Vec3::new(
        base.x.powf(exponent),
        base.y.powf(exponent),
        base.z.powf(exponent),
      )))
    }
    _ => unimplemented!(),
  }
}

fn exp_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Float(value.exp()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.exp(),
        value.y.exp(),
        value.z.exp(),
      )))
    }
    _ => unimplemented!(),
  }
}

fn log10_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Float(value.log10()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.log10(),
        value.y.log10(),
        value.z.log10(),
      )))
    }
    _ => unimplemented!(),
  }
}

fn log2_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Float(value.log2()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.log2(),
        value.y.log2(),
        value.z.log2(),
      )))
    }
    _ => unimplemented!(),
  }
}

fn ln_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Float(value.ln()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.ln(),
        value.y.ln(),
        value.z.ln(),
      )))
    }
    _ => unimplemented!(),
  }
}

fn tan_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Float(value.tan()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.tan(),
        value.y.tan(),
        value.z.tan(),
      )))
    }
    _ => unimplemented!(),
  }
}

fn cos_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Float(value.cos()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.cos(),
        value.y.cos(),
        value.z.cos(),
      )))
    }
    _ => unimplemented!(),
  }
}

fn sin_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Float(value.sin()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.sin(),
        value.y.sin(),
        value.z.sin(),
      )))
    }
    _ => unimplemented!(),
  }
}

fn sinh_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Float(value.sinh()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.sinh(),
        value.y.sinh(),
        value.z.sinh(),
      )))
    }
    _ => unimplemented!(),
  }
}

fn cosh_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Float(value.cosh()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.cosh(),
        value.y.cosh(),
        value.z.cosh(),
      )))
    }
    _ => unimplemented!(),
  }
}

fn tanh_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Float(value.tanh()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.tanh(),
        value.y.tanh(),
        value.z.tanh(),
      )))
    }
    _ => unimplemented!(),
  }
}

fn box_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  {
    let (width, height, depth) = match def_ix {
      0 => {
        let w = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        let h = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
        let d = arg_refs[2].resolve(args, &kwargs).as_float().unwrap();
        (w, h, d)
      }
      1 => {
        let val = arg_refs[0].resolve(args, &kwargs);
        match val {
          Value::Vec3(v3) => (v3.x, v3.y, v3.z),
          Value::Float(size) => (*size, *size, *size),
          Value::Int(size) => {
            let size = *size as f32;
            (size, size, size)
          }
          _ => {
            return Err(ErrorStack::new(format!(
              "Invalid argument for box size: expected Vec3 or Float, found {val:?}",
            )))
          }
        }
      }
      _ => unimplemented!(),
    };
    Ok(Value::Mesh(Rc::new(MeshHandle::new(Rc::new(
      LinkedMesh::new_box(width, height, depth),
    )))))
  }
}

fn icosphere_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let radius = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      let resolution = arg_refs[1].resolve(args, &kwargs).as_int().unwrap();
      if resolution < 0 {
        return Err(ErrorStack::new("Resolution must be a non-negative integer"));
      }

      Ok(Value::Mesh(Rc::new(MeshHandle::new(Rc::new(
        LinkedMesh::new_icosphere(radius, resolution as u32),
      )))))
    }
    _ => unimplemented!(),
  }
}

fn cylinder_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let radius = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      let height = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
      let radial_segments = arg_refs[2].resolve(args, &kwargs).as_int().unwrap();
      let height_segments = arg_refs[3].resolve(args, &kwargs).as_int().unwrap();

      if radial_segments < 3 {
        return Err(ErrorStack::new("`radial_segments` must be >= 3"));
      } else if height_segments < 1 {
        return Err(ErrorStack::new("`height_segments` must be >= 1"));
      }

      Ok(Value::Mesh(Rc::new(MeshHandle::new(Rc::new(
        LinkedMesh::new_cylinder(
          radius,
          height,
          radial_segments as usize,
          height_segments as usize,
        ),
      )))))
    }
    _ => unimplemented!(),
  }
}

fn grid_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let size = match arg_refs[0].resolve(args, &kwargs) {
        _ if let Some(v) = arg_refs[0].resolve(args, &kwargs).as_vec2() => *v,
        _ if let Some(f) = arg_refs[0].resolve(args, &kwargs).as_float() => Vec2::new(f, f),
        other => {
          return Err(ErrorStack::new(format!(
            "Invalid type for grid size: expected Vec2 or Float, found {other:?}",
          )))
        }
      };
      let divisions = match arg_refs[1].resolve(args, &kwargs) {
        _ if let Some(v) = arg_refs[1].resolve(args, &kwargs).as_vec2() => {
          if v.x < 1. || v.y < 1. {
            return Err(ErrorStack::new("Grid divisions must be >= 1"));
          }
          (v.x as usize, v.y as usize)
        }
        _ if let Some(i) = arg_refs[1].resolve(args, &kwargs).as_int() => {
          if i < 1 {
            return Err(ErrorStack::new("Grid divisions must be >= 1"));
          }
          (i as usize, i as usize)
        }
        other => {
          return Err(ErrorStack::new(format!(
            "Invalid type for grid divisions: expected Vec2 or Int, found {other:?}",
          )))
        }
      };
      let flipped = arg_refs[2].resolve(args, &kwargs).as_bool().unwrap();

      let mesh = LinkedMesh::new_grid(size.x, size.y, divisions.0, divisions.1, flipped);
      Ok(Value::Mesh(Rc::new(MeshHandle::new(Rc::new(mesh)))))
    }
    _ => unimplemented!(),
  }
}

fn rot_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  {
    let (mesh, rotation) = match def_ix {
      0 => {
        let rotation = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
        let mesh = arg_refs[1].resolve(args, &kwargs).as_mesh().unwrap();

        (
          mesh,
          UnitQuaternion::from_euler_angles(rotation.x, rotation.y, rotation.z),
        )
      }
      1 => {
        let x = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        let y = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
        let z = arg_refs[2].resolve(args, &kwargs).as_float().unwrap();
        let mesh = arg_refs[3].resolve(args, &kwargs).as_mesh().unwrap();

        (mesh, UnitQuaternion::from_euler_angles(x, y, z))
      }
      _ => unimplemented!(),
    };

    // apply rotation by translating to origin, rotating, then translating back
    let mut rotated_mesh = mesh.clone(true, false, false);
    let back: Matrix4<f32> = Matrix4::new_translation(&mesh.transform.column(3).xyz());
    let to_origin: Matrix4<f32> = Matrix4::new_translation(&-mesh.transform.column(3).xyz());
    rotated_mesh.transform = back * rotation.to_homogeneous() * to_origin * rotated_mesh.transform;

    Ok(Value::Mesh(Rc::new(rotated_mesh)))
  }
}

fn look_at_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    // TODO: I'm pretty sure this isn't working like I was expecting it to
    0 => {
      let pos = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      let target = arg_refs[1].resolve(args, &kwargs).as_vec3().unwrap();

      let dir = target - *pos;
      let up = Vec3::new(0., 1., 0.);
      let rot = UnitQuaternion::look_at_rh(&dir, &up);
      let (x, y, z) = rot.euler_angles();
      Ok(Value::Vec3(Vec3::new(x, y, z)))
    }
    1 => {
      let mesh = arg_refs[0].resolve(args, &kwargs).as_mesh().unwrap();
      let target = arg_refs[1].resolve(args, &kwargs).as_vec3().unwrap();
      let up = arg_refs[2].resolve(args, &kwargs).as_vec3().unwrap();

      let mut mesh = mesh.clone(true, false, false);

      // extract translation
      let translation = mesh.transform.column(3).xyz();

      // extract current scale
      let basis3 = mesh.transform.fixed_view::<3, 3>(0, 0).clone_owned();
      let scale_x = basis3.column(0).norm();
      let scale_y = basis3.column(1).norm();
      let scale_z = basis3.column(2).norm();

      let dir = (target - translation).normalize();

      let rotation = Rotation3::rotation_between(&up, &dir).ok_or_else(|| {
        ErrorStack::new(format!(
          "Error computing rotation; degenerate direction or parallel to up? dir={dir:?}, \
           up={up:?}"
        ))
      })?;

      let rot_mat = rotation
        .to_homogeneous()
        .fixed_view::<3, 3>(0, 0)
        .clone_owned();
      let new_rs = Matrix3::from_columns(&[
        rot_mat.column(0) * scale_x,
        rot_mat.column(1) * scale_y,
        rot_mat.column(2) * scale_z,
      ]);

      mesh
        .transform
        .fixed_view_mut::<3, 3>(0, 0)
        .copy_from(&new_rs);
      mesh.transform[(0, 3)] = translation.x;
      mesh.transform[(1, 3)] = translation.y;
      mesh.transform[(2, 3)] = translation.z;

      Ok(Value::Mesh(Rc::new(mesh)))
    }
    _ => unimplemented!(),
  }
}

fn origin_to_geometry_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let mesh = arg_refs[0].resolve(args, &kwargs).as_mesh().unwrap();
      let mut new_mesh = (*mesh.mesh).clone();
      let center = mesh
        .mesh
        .vertices
        .values()
        .fold(Vec3::zeros(), |acc, vtx| acc + vtx.position)
        / new_mesh.vertices.len() as f32;
      for vtx in new_mesh.vertices.values_mut() {
        vtx.position -= center;
      }
      Ok(Value::Mesh(Rc::new(MeshHandle {
        aabb: RefCell::new(None),
        manifold_handle: Rc::new(ManifoldHandle::new_empty()),
        trimesh: RefCell::new(None),
        transform: mesh.transform,
        mesh: Rc::new(new_mesh),
        material: mesh.material.clone(),
      })))
    }
    _ => unimplemented!(),
  }
}

fn apply_transforms_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let mesh = arg_refs[0].resolve(args, &kwargs).as_mesh().unwrap();
      let mut new_mesh = (*mesh.mesh).clone();
      for vtx in new_mesh.vertices.values_mut() {
        vtx.position = (mesh.transform * vtx.position.push(1.)).xyz();
      }
      Ok(Value::Mesh(Rc::new(MeshHandle::new(Rc::new(new_mesh)))))
    }
    _ => unimplemented!(),
  }
}

fn vec2_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let x = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      let y = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Vec2(Vec2::new(x, y)))
    }
    1 => {
      let x = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Vec2(Vec2::new(x, x)))
    }
    _ => unimplemented!(),
  }
}

fn vec3_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let x = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      let y = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
      let z = arg_refs[2].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Vec3(Vec3::new(x, y, z)))
    }
    1 => {
      let x = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Vec3(Vec3::new(x, x, x)))
    }
    _ => unimplemented!(),
  }
}

fn join_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let mut iter = arg_refs[0]
        .resolve(args, &kwargs)
        .as_sequence()
        .unwrap()
        .clone_box()
        .consume(ctx);

      let Some(first) = iter.next() else {
        return Ok(Value::Mesh(Rc::new(MeshHandle::new(Rc::new(
          LinkedMesh::new(0, 0, None),
        )))));
      };
      let base = match first? {
        Value::Mesh(m) => m,
        other => {
          return Err(ErrorStack::new(format!(
            "Non-mesh value produced in sequence passed to join: {other:?}"
          )))
        }
      };

      let out_transform = base.transform;
      let out_transform_inv = out_transform.try_inverse().unwrap();
      let mut combined = (*base.mesh).clone();

      for val in iter {
        let rhs = match val? {
          Value::Mesh(m) => m,
          other => {
            return Err(ErrorStack::new(format!(
              "Non-mesh value produced in sequence passed to join: {other:?}"
            )))
          }
        };

        let mut new_vtx_key_by_old: FxHashMap<VertexKey, VertexKey> = FxHashMap::default();
        for face in rhs.mesh.faces.values() {
          let new_vtx_keys = std::array::from_fn(|i| {
            let old_vtx_key = face.vertices[i];
            *new_vtx_key_by_old.entry(old_vtx_key).or_insert_with(|| {
              let old_vtx = &rhs.mesh.vertices[old_vtx_key];

              // transform from rhs local space -> world space -> lhs local space
              let transformed_pos = out_transform_inv * rhs.transform * old_vtx.position.push(1.);

              let new_vtx_key = combined.vertices.insert(Vertex {
                position: transformed_pos.xyz(),
                displacement_normal: old_vtx.displacement_normal,
                shading_normal: old_vtx.shading_normal,
                edges: Vec::new(),
              });
              new_vtx_key
            })
          });
          combined.add_face(new_vtx_keys, ());
        }
      }

      Ok(Value::Mesh(Rc::new(MeshHandle {
        mesh: Rc::new(combined),
        transform: out_transform,
        manifold_handle: Rc::new(ManifoldHandle::new(0)),
        aabb: RefCell::new(None),
        trimesh: RefCell::new(None),
        material: base.material.clone(),
      })))
    }
    _ => unimplemented!(),
  }
}

fn filter_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let fn_value = arg_refs[0].resolve(args, &kwargs).as_callable().unwrap();
      let sequence = arg_refs[1].resolve(args, &kwargs).as_sequence().unwrap();

      Ok(Value::Sequence(Box::new(FilterSeq {
        cb: fn_value.clone(),
        inner: sequence.clone_box(),
      })))
    }
    _ => unimplemented!(),
  }
}

fn scan_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let init = arg_refs[0].resolve(args, &kwargs);
      let fn_value = arg_refs[1].resolve(args, &kwargs).as_callable().unwrap();
      let sequence = arg_refs[2].resolve(args, &kwargs).as_sequence().unwrap();
      Ok(Value::Sequence(Box::new(ScanSeq {
        acc: init.clone(),
        cb: fn_value.clone(),
        inner: sequence.clone_box(),
      })))
    }
    _ => unimplemented!(),
  }
}

fn take_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let count = arg_refs[0].resolve(args, &kwargs).as_int().unwrap();
      let count = if count < 0 { 0 } else { count as usize };
      let sequence = arg_refs[1].resolve(args, &kwargs).as_sequence().unwrap();
      Ok(Value::Sequence(Box::new(TakeSeq {
        count,
        inner: sequence.clone_box(),
      })))
    }
    _ => unimplemented!(),
  }
}

fn skip_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let count = arg_refs[0].resolve(args, &kwargs).as_int().unwrap();
      let count = if count < 0 { 0 } else { count as usize };
      let sequence = arg_refs[1].resolve(args, &kwargs).as_sequence().unwrap();
      Ok(Value::Sequence(Box::new(SkipSeq {
        count,
        inner: sequence.clone_box(),
      })))
    }
    _ => unimplemented!(),
  }
}

fn take_while_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let fn_value = arg_refs[0].resolve(args, &kwargs).as_callable().unwrap();
      let sequence = arg_refs[1].resolve(args, &kwargs).as_sequence().unwrap();
      Ok(Value::Sequence(Box::new(TakeWhileSeq {
        cb: fn_value.clone(),
        inner: sequence.clone_box(),
      })))
    }
    _ => unimplemented!(),
  }
}

fn skip_while_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let fn_value = arg_refs[0].resolve(args, &kwargs).as_callable().unwrap();
      let sequence = arg_refs[1].resolve(args, &kwargs).as_sequence().unwrap();
      Ok(Value::Sequence(Box::new(SkipWhileSeq {
        cb: fn_value.clone(),
        inner: sequence.clone_box(),
      })))
    }
    _ => unimplemented!(),
  }
}

fn chain_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let seqs = arg_refs[0]
        .resolve(args, &kwargs)
        .as_sequence()
        .unwrap()
        .clone_box();
      Ok(Value::Sequence(Box::new(ChainSeq::new(ctx, seqs)?)))
    }
    _ => unimplemented!(),
  }
}

fn first_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let sequence = arg_refs[0].resolve(args, &kwargs).as_sequence().unwrap();
      let mut iter = sequence.clone_box().consume(ctx);
      match iter.next() {
        Some(res) => res,
        None => Ok(Value::Nil),
      }
    }
    _ => unimplemented!(),
  }
}

fn reverse_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let sequence = arg_refs[0].resolve(args, &kwargs).as_sequence().unwrap();
      let vals: Vec<Value> = sequence
        .clone_box()
        .consume(ctx)
        .collect::<Result<Vec<_>, _>>()?;
      Ok(Value::Sequence(Box::new(IteratorSeq {
        inner: vals.into_iter().rev().map(Ok),
      })))
    }
    _ => unimplemented!(),
  }
}

fn collect_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let seq = arg_refs[0].resolve(args, &kwargs).as_sequence().unwrap();
      match seq_as_eager(seq) {
        Some(_) => Ok(Value::Sequence(seq.clone_box())),
        None => {
          let iter = seq.clone_box().consume(ctx);
          let collected = iter
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| err.wrap("error produced during `collect`"))?;
          Ok(Value::Sequence(Box::new(EagerSeq { inner: collected })))
        }
      }
    }
    _ => unimplemented!(),
  }
}

fn any_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let cb = arg_refs[0].resolve(args, &kwargs).as_callable().unwrap();
      let sequence = arg_refs[1].resolve(args, &kwargs).as_sequence().unwrap();
      let iter = sequence.clone_box().consume(ctx);
      for (i, res) in iter.enumerate() {
        let val = res?;
        let val = ctx
          .invoke_callable(cb, &[val], &Default::default(), &ctx.globals)
          .map_err(|err| err.wrap("error calling user-provided callback passed to `any`"))?;
        match val {
          Value::Bool(b) => {
            if b {
              return Ok(Value::Bool(true));
            }
          }
          other => {
            return Err(ErrorStack::new(format!(
              "Non-bool value produced at index {i} by cb passed to `any`: {other:?}"
            )));
          }
        }
      }
      Ok(Value::Bool(false))
    }
    _ => unimplemented!(),
  }
}

fn all_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let cb = arg_refs[0].resolve(args, &kwargs).as_callable().unwrap();
      let sequence = arg_refs[1].resolve(args, &kwargs).as_sequence().unwrap();
      let iter = sequence.clone_box().consume(ctx);
      for (i, res) in iter.enumerate() {
        let val = res?;
        let val = ctx
          .invoke_callable(cb, &[val], &Default::default(), &ctx.globals)
          .map_err(|err| err.wrap("error calling user-provided callback passed to `all`"))?;
        match val {
          Value::Bool(b) => {
            if !b {
              return Ok(Value::Bool(false));
            }
          }
          other => {
            return Err(ErrorStack::new(format!(
              "Non-bool value produced at index {i} by cb passed to `all`: {other:?}"
            )));
          }
        }
      }
      Ok(Value::Bool(true))
    }
    _ => unimplemented!(),
  }
}

fn for_each_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let cb = arg_refs[0].resolve(args, &kwargs).as_callable().unwrap();
      let sequence = arg_refs[1].resolve(args, &kwargs).as_sequence().unwrap();
      let iter = sequence.clone_box().consume(ctx);
      for res in iter {
        let val = res?;
        ctx
          .invoke_callable(cb, &[val], &Default::default(), &ctx.globals)
          .map_err(|err| err.wrap("error calling user-provided callback passed to `for_each`"))?;
      }
      Ok(Value::Nil)
    }
    _ => unimplemented!(),
  }
}

fn flatten_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let seq = arg_refs[0].resolve(args, &kwargs).as_sequence().unwrap();
      Ok(Value::Sequence(Box::new(FlattenSeq {
        inner: seq.clone_box(),
      })))
    }
    _ => unimplemented!(),
  }
}

fn abs_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_int().unwrap();
      Ok(Value::Int(value.abs()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Float(value.abs()))
    }
    2 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.abs(),
        value.y.abs(),
        value.z.abs(),
      )))
    }
    _ => unimplemented!(),
  }
}

fn sqrt_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Float(value.sqrt()))
    }
    _ => unimplemented!(),
  }
}

fn max_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      // int, int
      let a = arg_refs[0].resolve(args, &kwargs).as_int().unwrap();
      let b = arg_refs[1].resolve(args, &kwargs).as_int().unwrap();
      Ok(Value::Int(a.max(b)))
    }
    1 => {
      // float, float
      let a = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      let b = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Float(a.max(b)))
    }
    2 => {
      // vec3, vec3
      let a = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      let b = arg_refs[1].resolve(args, &kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        a.x.max(b.x),
        a.y.max(b.y),
        a.z.max(b.z),
      )))
    }
    _ => unimplemented!(),
  }
}

fn min_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      // int, int
      let a = arg_refs[0].resolve(args, &kwargs).as_int().unwrap();
      let b = arg_refs[1].resolve(args, &kwargs).as_int().unwrap();
      Ok(Value::Int(a.min(b)))
    }
    1 => {
      // float, float
      let a = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      let b = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Float(a.min(b)))
    }
    2 => {
      // vec3, vec3
      let a = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      let b = arg_refs[1].resolve(args, &kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        a.x.min(b.x),
        a.y.min(b.y),
        a.z.min(b.z),
      )))
    }
    _ => unimplemented!(),
  }
}

fn clamp_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      // int, int, int
      let value = arg_refs[0].resolve(args, &kwargs).as_int().unwrap();
      let min = arg_refs[1].resolve(args, &kwargs).as_int().unwrap();
      let max = arg_refs[2].resolve(args, &kwargs).as_int().unwrap();
      Ok(Value::Int(value.clamp(min, max)))
    }
    1 => {
      // float, float, float
      let value = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
      let min = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
      let max = arg_refs[2].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Float(value.clamp(min, max)))
    }
    2 => {
      // vec3, float, float
      let value = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
      let min = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
      let max = arg_refs[2].resolve(args, &kwargs).as_float().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.clamp(min, max),
        value.y.clamp(min, max),
        value.z.clamp(min, max),
      )))
    }
    _ => unimplemented!(),
  }
}

fn float_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let val = arg_refs[0].resolve(args, &kwargs);
      let val = match val {
        _ if let Some(float) = val.as_float() => float,
        _ if let Some(int) = val.as_int() => int as f32,
        Value::String(s) => s
          .parse::<f32>()
          .map_err(|_| ErrorStack::new(format!("Failed to parse string as float: {s}")))?,
        _ => unreachable!(),
      };
      Ok(Value::Float(val))
    }
    _ => unimplemented!(),
  }
}

fn int_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let val = arg_refs[0].resolve(args, &kwargs);
      let val = match val {
        _ if let Some(int) = val.as_int() => int,
        _ if let Some(float) = val.as_float() => float as i64,
        Value::String(s) => s
          .parse::<i64>()
          .map_err(|_| ErrorStack::new(format!("Failed to parse string as int: {s}")))?,
        _ => unreachable!(),
      };
      Ok(Value::Int(val))
    }
    _ => unimplemented!(),
  }
}

macro_rules! builtin_fn {
  ($name:ident, $impl:expr) => {
    paste! {{
      fn [<builtin_ $name _impl>] (
        def_ix: usize,
        arg_refs: &[ArgRef],
        args: &[Value],
        kwargs: &FxHashMap<String, Value>,
        ctx: &EvalCtx,
      ) -> Result<Value, ErrorStack> {
        $impl(def_ix, arg_refs, args, kwargs, ctx)
      }

      [<builtin_ $name _impl>]
    }}
  };
}

pub(crate) static BUILTIN_FN_IMPLS: phf::Map<
  &'static str,
  fn(
    def_ix: usize,
    arg_refs: &[ArgRef],
    args: &[Value],
    kwargs: &FxHashMap<String, Value>,
    ctx: &EvalCtx,
  ) -> Result<Value, ErrorStack>,
> = phf::phf_map! {
  "box" => builtin_fn!(box, |def_ix, arg_refs, args, kwargs, _ctx| {
    box_impl(def_ix, arg_refs, args, kwargs)
  }),
  "icosphere" => builtin_fn!(icosphere, |def_ix, arg_refs, args, kwargs, _ctx| {
    icosphere_impl(def_ix, arg_refs, args, kwargs)
  }),
  "cylinder" => builtin_fn!(cylinder, |def_ix, arg_refs, args, kwargs, _ctx| {
    cylinder_impl(def_ix, arg_refs, args, kwargs)
  }),
  "grid" => builtin_fn!(grid, |def_ix, arg_refs, args, kwargs, _ctx| {
    grid_impl(def_ix, arg_refs, args, kwargs)
  }),
  "translate" => builtin_fn!(translate, |def_ix, arg_refs, args, kwargs, _ctx| {
    translate_impl(def_ix, arg_refs, args, kwargs)
  }),
  "scale" => builtin_fn!(scale, |def_ix, arg_refs, args, kwargs, _ctx| {
    scale_impl(def_ix, arg_refs, args, kwargs)
  }),
  "rot" => builtin_fn!(rot, |def_ix, arg_refs, args, kwargs, _ctx| {
    rot_impl(def_ix, arg_refs, args, kwargs)
  }),
  "look_at" => builtin_fn!(look_at, |def_ix, arg_refs, args, kwargs, _ctx| {
    look_at_impl(def_ix, arg_refs, args, kwargs)
  }),
  "origin_to_geometry" => builtin_fn!(origin_to_geometry, |def_ix, arg_refs, args, kwargs, _ctx| {
    origin_to_geometry_impl(def_ix, arg_refs, args, kwargs)
  }),
  "apply_transforms" => builtin_fn!(apply_transforms, |def_ix, arg_refs, args, kwargs, _ctx| {
    apply_transforms_impl(def_ix, arg_refs, args, kwargs)
  }),
  "vec2" => builtin_fn!(vec2, |def_ix, arg_refs, args, kwargs, _ctx| {
    vec2_impl(def_ix, arg_refs, args, kwargs)
  }),
  "vec3" => builtin_fn!(vec3, |def_ix, arg_refs, args, kwargs, _ctx| {
    vec3_impl(def_ix, arg_refs, args, kwargs)
  }),
  "join" => builtin_fn!(join, |def_ix, arg_refs, args, kwargs, ctx| {
    join_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "union" => builtin_fn!(union, |def_ix, arg_refs, args, kwargs, ctx| {
    eval_mesh_boolean(def_ix, arg_refs, args, kwargs, ctx, MeshBooleanOp::Union)
  }),
  "difference" => builtin_fn!(difference, |def_ix, arg_refs, args, kwargs, ctx| {
    eval_mesh_boolean(
      def_ix,
      arg_refs,
      args,
      kwargs,
      ctx,
      MeshBooleanOp::Difference,
    )
  }),
  "intersect" => builtin_fn!(intersect, |def_ix, arg_refs, args, kwargs, ctx| {
    eval_mesh_boolean(
      def_ix,
      arg_refs,
      args,
      kwargs,
      ctx,
      MeshBooleanOp::Intersection,
    )
  }),
  "fold" => builtin_fn!(fold, |_def_ix, arg_refs: &[ArgRef], args, kwargs, ctx: &EvalCtx| {
    let initial_val = arg_refs[0].resolve(args, kwargs).clone();
    let fn_value = arg_refs[1].resolve(args, kwargs).as_callable().unwrap();
    let sequence = arg_refs[2].resolve(args, kwargs).as_sequence().unwrap();
    ctx.fold(initial_val, fn_value, sequence.clone_box())
  }),
  "reduce" => builtin_fn!(reduce, |_def_ix, arg_refs: &[ArgRef], args, kwargs, ctx: &EvalCtx| {
    let fn_value = arg_refs[0].resolve(args, kwargs).as_callable().unwrap();
    let seq = arg_refs[1].resolve(args, kwargs).as_sequence().unwrap();
    ctx.reduce(fn_value, seq.clone_box())
  }),
  "map" => builtin_fn!(map, |def_ix, arg_refs: &[ArgRef], args, kwargs, ctx| {
    let fn_value = arg_refs[0].resolve(args, kwargs);
    let seq = arg_refs[1].resolve(args, kwargs);
    map_impl(ctx, def_ix, fn_value, seq)
  }),
  "filter" => builtin_fn!(filter, |def_ix, arg_refs, args, kwargs, _ctx| {
    filter_impl(def_ix, arg_refs, args, kwargs)
  }),
  "scan" => builtin_fn!(scan, |def_ix, arg_refs: &[ArgRef], args, kwargs, _ctx| {
    scan_impl(def_ix, arg_refs, args, kwargs)
  }),
  "take" => builtin_fn!(take, |def_ix, arg_refs, args, kwargs, _ctx| {
    take_impl(def_ix, arg_refs, args, kwargs)
  }),
  "skip" => builtin_fn!(skip, |def_ix, arg_refs, args, kwargs, _ctx| {
    skip_impl(def_ix, arg_refs, args, kwargs)
  }),
  "take_while" => builtin_fn!(take_while, |def_ix, arg_refs, args, kwargs, _ctx| {
    take_while_impl(def_ix, arg_refs, args, kwargs)
  }),
  "skip_while" => builtin_fn!(skip_while, |def_ix, arg_refs, args, kwargs, _ctx| {
    skip_while_impl(def_ix, arg_refs, args, kwargs)
  }),
  "chain" => builtin_fn!(chain, |def_ix, arg_refs, args, kwargs, ctx| {
    chain_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "first" => builtin_fn!(first, |def_ix, arg_refs, args, kwargs, ctx| {
    first_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "reverse" => builtin_fn!(reverse, |def_ix, arg_refs: &[ArgRef], args, kwargs, ctx| {
    reverse_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "collect" => builtin_fn!(collect, |def_ix, arg_refs: &[ArgRef], args, kwargs, ctx| {
    collect_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "any" => builtin_fn!(any, |def_ix, arg_refs, args, kwargs, ctx| {
    any_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "all" => builtin_fn!(all, |def_ix, arg_refs, args, kwargs, ctx| {
    all_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "for_each" => builtin_fn!(for_each, |def_ix, arg_refs: &[ArgRef], args, kwargs, ctx| {
    for_each_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "flatten" => builtin_fn!(flatten, |def_ix, arg_refs: &[ArgRef], args, kwargs, _ctx| {
    flatten_impl(def_ix, arg_refs, args, kwargs)
  }),
  "neg" => builtin_fn!(neg, |def_ix, arg_refs: &[ArgRef], args, kwargs, _ctx| {
    let val = arg_refs[0].resolve(args, kwargs);
    neg_impl(def_ix, val)
  }),
  "pos" => builtin_fn!(pos, |def_ix, arg_refs: &[ArgRef], args, kwargs, _ctx| {
    let val = arg_refs[0].resolve(args, kwargs);
    pos_impl(def_ix, val)
  }),
  "abs" => builtin_fn!(abs, |def_ix, arg_refs, args, kwargs, _ctx| {
    abs_impl(def_ix, arg_refs, args, kwargs)
  }),
  "sqrt" => builtin_fn!(sqrt, |def_ix, arg_refs, args, kwargs, _ctx| {
    sqrt_impl(def_ix, arg_refs, args, kwargs)
  }),
  "add" => builtin_fn!(add, |def_ix, arg_refs: &[ArgRef], args, kwargs, _ctx| {
    let lhs = arg_refs[0].resolve(args, kwargs);
    let rhs = arg_refs[1].resolve(args, kwargs);
    add_impl(def_ix, lhs, rhs)
  }),
  "sub" => builtin_fn!(sub, |def_ix, arg_refs: &[ArgRef], args, kwargs, ctx| {
    let lhs = arg_refs[0].resolve(args, kwargs);
    let rhs = arg_refs[1].resolve(args, kwargs);
    sub_impl(ctx, def_ix, lhs, rhs)
  }),
  "mul" => builtin_fn!(mul, |def_ix, arg_refs: &[ArgRef], args, kwargs, _ctx| {
    let lhs = arg_refs[0].resolve(args, kwargs);
    let rhs = arg_refs[1].resolve(args, kwargs);
    mul_impl(def_ix, lhs, rhs)
  }),
  "div" => builtin_fn!(div, |def_ix, arg_refs: &[ArgRef], args, kwargs, _ctx| {
    let lhs = arg_refs[0].resolve(args, kwargs);
    let rhs = arg_refs[1].resolve(args, kwargs);
    div_impl(def_ix, lhs, rhs)
  }),
  "mod" => builtin_fn!(max, |def_ix, arg_refs: &[ArgRef], args, kwargs, _ctx| {
    let lhs = arg_refs[0].resolve(args, kwargs);
    let rhs = arg_refs[1].resolve(args, kwargs);
    mod_impl(def_ix, lhs, rhs)
  }),
  "max" => builtin_fn!(max, |def_ix, arg_refs, args, kwargs, _ctx| {
    max_impl(def_ix, arg_refs, args, kwargs)
  }),
  "min" => builtin_fn!(min, |def_ix, arg_refs, args, kwargs, _ctx| {
    min_impl(def_ix, arg_refs, args, kwargs)
  }),
  "clamp" => builtin_fn!(clamp, |def_ix, arg_refs: &[ArgRef], args, kwargs, _ctx| {
    clamp_impl(def_ix, arg_refs, args, kwargs)
  }),
  "float" => builtin_fn!(float, |def_ix, arg_refs, args, kwargs, _ctx| {
    float_impl(def_ix, arg_refs, args, kwargs)
  }),
  "int" => builtin_fn!(int, |def_ix, arg_refs, args, kwargs, _ctx| {
    int_impl(def_ix, arg_refs, args, kwargs)
  }),
  "gte" => builtin_fn!(gte, |def_ix, arg_refs, args, kwargs, _ctx| {
    eval_numeric_bool_op(def_ix, arg_refs, args, kwargs, BoolOp::Gte)
  }),
  "lte" => builtin_fn!(lte, |def_ix, arg_refs, args, kwargs, _ctx| {
    eval_numeric_bool_op(def_ix, arg_refs, args, kwargs, BoolOp::Lte)
  }),
  "gt" => builtin_fn!(gt, |def_ix, arg_refs, args, kwargs, _ctx| {
    eval_numeric_bool_op(def_ix, arg_refs, args, kwargs, BoolOp::Gt)
  }),
  "lt" => builtin_fn!(lt, |def_ix, arg_refs, args, kwargs, _ctx| {
    eval_numeric_bool_op(def_ix, arg_refs, args, kwargs, BoolOp::Lt)
  }),
  "eq" => builtin_fn!(eq, |def_ix, arg_refs: &[ArgRef], args, kwargs, _ctx| {
    let lhs = arg_refs[0].resolve(args, kwargs);
    let rhs = arg_refs[1].resolve(args, kwargs);
    eq_impl(def_ix, lhs, rhs)
  }),
  "neq" => builtin_fn!(neq, |def_ix, arg_refs: &[ArgRef], args, kwargs, _ctx| {
    let lhs = arg_refs[0].resolve(args, kwargs);
    let rhs = arg_refs[1].resolve(args, kwargs);
    neq_impl(def_ix, lhs, rhs)
  }),
  "and" => builtin_fn!(and, |def_ix, arg_refs: &[ArgRef], args, kwargs, ctx| {
    let lhs = arg_refs[0].resolve(args, kwargs);
    let rhs = arg_refs[1].resolve(args, kwargs);
    and_impl(ctx, def_ix, lhs, rhs)
  }),
  "or" => builtin_fn!(or, |def_ix, arg_refs: &[ArgRef], args, kwargs, ctx| {
    let lhs = arg_refs[0].resolve(args, kwargs);
    let rhs = arg_refs[1].resolve(args, kwargs);
    or_impl(ctx, def_ix, lhs, rhs)
  }),
  "not" => builtin_fn!(not, |def_ix, arg_refs: &[ArgRef], args, kwargs, _ctx| {
    let val = arg_refs[0].resolve(args, kwargs);
    not_impl(def_ix, val)
  }),
  "bit_and" => builtin_fn!(bit_and, |def_ix, arg_refs: &[ArgRef], args, kwargs, ctx| {
    let lhs = arg_refs[0].resolve(args, kwargs);
    let rhs = arg_refs[1].resolve(args, kwargs);
    bit_and_impl(ctx, def_ix, lhs, rhs)
  }),
  "bit_or" => builtin_fn!(bit_or, |def_ix, arg_refs: &[ArgRef], args, kwargs, ctx| {
    let lhs = arg_refs[0].resolve(args, kwargs);
    let rhs = arg_refs[1].resolve(args, kwargs);
    bit_or_impl(ctx, def_ix, lhs, rhs)
  }),
  "sin" => builtin_fn!(sin, |def_ix, arg_refs, args, kwargs, _ctx| {
    sin_impl(def_ix, arg_refs, args, kwargs)
  }),
  "cos" => builtin_fn!(cos, |def_ix, arg_refs, args, kwargs, _ctx| {
    cos_impl(def_ix, arg_refs, args, kwargs)
  }),
  "tan" => builtin_fn!(tan, |def_ix, arg_refs, args, kwargs, _ctx| {
    tan_impl(def_ix, arg_refs, args, kwargs)
  }),
  "sinh" => builtin_fn!(sinh, |def_ix, arg_refs, args, kwargs, _ctx| {
    sinh_impl(def_ix, arg_refs, args, kwargs)
  }),
  "cosh" => builtin_fn!(cosh, |def_ix, arg_refs, args, kwargs, _ctx| {
    cosh_impl(def_ix, arg_refs, args, kwargs)
  }),
  "tanh" => builtin_fn!(tanh, |def_ix, arg_refs, args, kwargs, _ctx| {
    tanh_impl(def_ix, arg_refs, args, kwargs)
  }),
  "pow" => builtin_fn!(pow, |def_ix, arg_refs: &[ArgRef], args, kwargs, _ctx| {
    pow_impl(def_ix, arg_refs, args, kwargs)
  }),
  "exp" => builtin_fn!(exp, |def_ix, arg_refs, args, kwargs, _ctx| {
    exp_impl(def_ix, arg_refs, args, kwargs)
  }),
  "log10" => builtin_fn!(log, |def_ix, arg_refs, args, kwargs, _ctx| {
    log10_impl(def_ix, arg_refs, args, kwargs)
  }),
  "log2" => builtin_fn!(log2, |def_ix, arg_refs, args, kwargs, _ctx| {
    log2_impl(def_ix, arg_refs, args, kwargs)
  }),
  "ln" => builtin_fn!(ln, |def_ix, arg_refs, args, kwargs, _ctx| {
    ln_impl(def_ix, arg_refs, args, kwargs)
  }),
  "trunc" => builtin_fn!(trunc, |def_ix, arg_refs, args, kwargs, _ctx| {
    trunc_impl(def_ix, arg_refs, args, kwargs)
  }),
  "fract" => builtin_fn!(fract, |def_ix, arg_refs, args, kwargs, _ctx| {
    fract_impl(def_ix, arg_refs, args, kwargs)
  }),
  "round" => builtin_fn!(round, |def_ix, arg_refs, args, kwargs, _ctx| {
    round_impl(def_ix, arg_refs, args, kwargs)
  }),
  "ceil" => builtin_fn!(ceil, |def_ix, arg_refs, args, kwargs, _ctx| {
    ceil_impl(def_ix, arg_refs, args, kwargs)
  }),
  "floor" => builtin_fn!(floor, |def_ix, arg_refs, args, kwargs, _ctx| {
    floor_impl(def_ix, arg_refs, args, kwargs)
  }),
  "fix_float" => builtin_fn!(fix_float, |def_ix, arg_refs, args, kwargs, _ctx| {
    fix_float_impl(def_ix, arg_refs, args, kwargs)
  }),
  "rad2deg" => builtin_fn!(rad2deg, |def_ix, arg_refs, args, kwargs, _ctx| {
    rad2deg_impl(def_ix, arg_refs, args, kwargs)
  }),
  "deg2rad" => builtin_fn!(deg2rad, |def_ix, arg_refs, args, kwargs, _ctx| {
    deg2rad_impl(def_ix, arg_refs, args, kwargs)
  }),
  "lerp" => builtin_fn!(lerp, |def_ix, arg_refs, args, kwargs, _ctx| {
    lerp_impl(def_ix, arg_refs, args, kwargs)
  }),
  "smoothstep" => builtin_fn!(smoothstep, |def_ix, arg_refs, args, kwargs, _ctx| {
    smoothstep_impl(def_ix, arg_refs, args, kwargs)
  }),
  "linearstep" => builtin_fn!(linearstep, |def_ix, arg_refs, args, kwargs, _ctx| {
    linearstep_impl(def_ix, arg_refs, args, kwargs)
  }),
  "print" => builtin_fn!(print, |_def_ix, _arg_refs, args, kwargs, ctx| {
    print_impl(ctx, args, kwargs)
  }),
  "render" => builtin_fn!(render, |def_ix, arg_refs, args, kwargs, ctx| {
    render_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "point_distribute" => builtin_fn!(point_distribute, |def_ix, arg_refs, args, kwargs, _ctx| {
    point_distribute_impl(def_ix, arg_refs, args, kwargs)
  }),
  "compose" => builtin_fn!(compose, |def_ix, _arg_refs, args, kwargs, ctx| {
    compose_impl(ctx, def_ix, args, kwargs)
  }),
  "warp" => builtin_fn!(warp, |def_ix, arg_refs, args, kwargs, ctx| {
    warp_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "tessellate" => builtin_fn!(tessellate, |def_ix, arg_refs, args, kwargs, _ctx| {
    tessellate_impl(def_ix, arg_refs, args, kwargs)
  }),
  "subdivide_by_plane" => builtin_fn!(subdivide_by_plane, |def_ix, arg_refs, args, kwargs, _ctx| {
    subdivide_by_plane_impl(def_ix, arg_refs, args, kwargs)
  }),
  "split_by_plane" => builtin_fn!(split_by_plane, |def_ix, arg_refs, args, kwargs, _ctx| {
    split_by_plane_impl(def_ix, arg_refs, args, kwargs)
  }),
  "connected_components" => builtin_fn!(connected_components, |def_ix, arg_refs, args, kwargs, _ctx| {
    connected_components_impl(def_ix, arg_refs, args, kwargs)
  }),
  "intersects" => builtin_fn!(intersects, |def_ix, arg_refs, args, kwargs, _ctx| {
    intersects_impl(def_ix, arg_refs, args, kwargs)
  }),
  "intersects_ray" => builtin_fn!(intersects_ray, |def_ix, arg_refs, args, kwargs, _ctx| {
    intersects_ray_impl(def_ix, arg_refs, args, kwargs)
  }),
  "len" => builtin_fn!(len, |def_ix, arg_refs, args, kwargs, _ctx| {
    len_impl(def_ix, arg_refs, args, kwargs)
  }),
  "distance" => builtin_fn!(distance, |def_ix, arg_refs, args, kwargs, _ctx| {
    distance_impl(def_ix, arg_refs, args, kwargs)
  }),
  "normalize" => builtin_fn!(normalize, |def_ix, arg_refs, args, kwargs, _ctx| {
    normalize_impl(def_ix, arg_refs, args, kwargs)
  }),
  "bezier3d" => builtin_fn!(bezier3d, |def_ix, arg_refs, args, kwargs, _ctx| {
    bezier3d_impl(def_ix, arg_refs, args, kwargs)
  }),
  "superellipse_path" => builtin_fn!(superellipse_path, |def_ix, arg_refs, args, kwargs, _ctx| {
    superellipse_path_impl(def_ix, arg_refs, args, kwargs)
  }),
  "extrude_pipe" => builtin_fn!(extrude_pipe, |def_ix, arg_refs, args, kwargs, ctx| {
    extrude_pipe_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "torus_knot_path" => builtin_fn!(torus_knot_path, |def_ix, arg_refs, args, kwargs, _ctx| {
    torus_knot_path_impl(def_ix, arg_refs, args, kwargs)
  }),
  "extrude" => builtin_fn!(extrude, |def_ix, arg_refs, args, kwargs, _ctx| {
    extrude_impl(def_ix, arg_refs, args, kwargs)
  }),
  "stitch_contours" => builtin_fn!(stitch_contours, |def_ix, arg_refs, args, kwargs, ctx| {
    stitch_contours_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "trace_geodesic_path" => builtin_fn!(trace_geodesic_path, |def_ix, arg_refs, args, kwargs, ctx| {
    trace_geodesic_path_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "fan_fill" => builtin_fn!(fan_fill, |def_ix, arg_refs, args, kwargs, ctx| {
    fan_fill_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "simplify" => builtin_fn!(simplify, |def_ix, arg_refs, args, kwargs, _ctx| {
    simplify_impl(def_ix, arg_refs, args, kwargs)
  }),
  "convex_hull" => builtin_fn!(convex_hull, |def_ix, arg_refs, args, kwargs, ctx| {
    convex_hull_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "verts" => builtin_fn!(verts, |def_ix, arg_refs, args, kwargs, _ctx| {
    verts_impl(def_ix, arg_refs, args, kwargs)
  }),
  "randf" => builtin_fn!(randf, |def_ix, arg_refs, args, kwargs, ctx| {
    randf_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "randv" => builtin_fn!(randv, |def_ix, arg_refs, args, kwargs, ctx| {
    randv_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "randi" => builtin_fn!(randi, |def_ix, arg_refs, args, kwargs, ctx| {
    randi_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "fbm" => builtin_fn!(fbm, |def_ix, arg_refs, args, kwargs, _ctx| {
    fbm_impl(def_ix, arg_refs, args, kwargs)
  }),
  "call" => builtin_fn!(call, |def_ix, arg_refs, args, kwargs, ctx| {
    call_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "mesh" => builtin_fn!(mesh, |def_ix, arg_refs, args, kwargs, ctx| {
    mesh_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "dir_light" => builtin_fn!(dir_light, |def_ix, arg_refs, args, kwargs, _ctx| {
    dir_light_impl(def_ix, arg_refs, args, kwargs)
  }),
  "ambient_light" => builtin_fn!(ambient_light, |def_ix, arg_refs, args, kwargs, _ctx| {
    ambient_light_impl(def_ix, arg_refs, args, kwargs)
  }),
  "point_light" => builtin_fn!(point_light, |def_ix, arg_refs, args, kwargs, _ctx| {
    point_light_impl(def_ix, arg_refs, args, kwargs)
  }),
  "spot_light" => builtin_fn!(spot_light, |def_ix, arg_refs, args, kwargs, _ctx| {
    spot_light_impl(def_ix, arg_refs, args, kwargs)
  }),
  "set_material" => builtin_fn!(set_material, |def_ix, arg_refs, args, kwargs, ctx| {
    set_material_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "set_default_material" => builtin_fn!(set_default_material, |def_ix, arg_refs, args, kwargs, ctx| {
    set_default_material_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
};

pub(crate) fn resolve_builtin_impl(
  name: &str,
) -> fn(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
  ctx: &EvalCtx,
) -> Result<Value, ErrorStack> {
  match BUILTIN_FN_IMPLS.get(name) {
    Some(f) => *f,
    None => match FUNCTION_ALIASES.get(name) {
      Some(&real_name) => *BUILTIN_FN_IMPLS.get(real_name).unwrap_or_else(|| {
        panic!("Alias `{name}` maps to builtin function `{real_name}`, but no such function exists")
      }),
      None => panic!("No builtin function named `{name}` found"),
    },
  }
}
