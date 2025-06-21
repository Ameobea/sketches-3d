use paste::paste;
use std::cell::RefCell;
use std::cmp::Reverse;
use std::marker::ConstParamTy;
use std::rc::Rc;

use fxhash::FxHashMap;
use mesh::{
  linked_mesh::{DisplacementNormalMethod, FaceKey, Vec3, Vertex, VertexKey},
  LinkedMesh,
};
use nalgebra::{Matrix3, Matrix4, Point3, Rotation3, UnitQuaternion};
use parry3d::bounding_volume::Aabb;
use parry3d::math::{Isometry, Point};
use parry3d::query::Ray;
use rand::Rng;

use crate::{
  lights::{AmbientLight, DirectionalLight, Light},
  mesh_ops::{
    extrude_pipe,
    mesh_boolean::{eval_mesh_boolean, MeshBooleanOp},
    mesh_ops::{convex_hull_from_verts, simplify_mesh},
  },
  noise::fbm,
  path_building::{build_torus_knot_path, cubic_bezier_3d_path},
  seq::{
    ChainSeq, FilterSeq, IteratorSeq, MeshVertsSeq, PointDistributeSeq, SkipSeq, SkipWhileSeq,
    TakeSeq, TakeWhileSeq,
  },
  ArgRef, Callable, ComposedFn, ErrorStack, EvalCtx, MapSeq, Value,
};
use crate::{ManifoldHandle, MeshHandle};

pub(crate) mod fn_defs;

pub(crate) static FUNCTION_ALIASES: phf::Map<&'static str, &'static str> = phf::phf_map! {
  "trans" => "translate",
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
      })))
    }
    5 => translate_impl(
      0,
      &[ArgRef::Positional(1), ArgRef::Positional(0)],
      &[lhs.clone(), rhs.clone()],
      &Default::default(),
    ),
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
        &[ArgRef::Positional(1), ArgRef::Positional(1)],
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

pub(crate) fn map_impl(def_ix: usize, fn_value: &Value, seq: &Value) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let fn_value = fn_value.as_callable().unwrap();
      let seq = seq.as_sequence().unwrap();

      Ok(Value::Sequence(Box::new(MapSeq {
        cb: fn_value.clone(),
        inner: seq.clone_box(),
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

#[inline(always)]
pub(crate) fn eval_builtin_fn(
  name: &str,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
  ctx: &EvalCtx,
) -> Result<Value, ErrorStack> {
  match name {
    "box" => {
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
    "icosphere" => match def_ix {
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
    },
    "cylinder" => match def_ix {
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
    },
    "translate" => translate_impl(def_ix, arg_refs, args, kwargs),
    "scale" => scale_impl(def_ix, arg_refs, args, kwargs),
    "rot" => {
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
      rotated_mesh.transform =
        back * rotation.to_homogeneous() * to_origin * rotated_mesh.transform;

      Ok(Value::Mesh(Rc::new(rotated_mesh)))
    }

    "look_at" => match def_ix {
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
    },
    "apply_transforms" => match def_ix {
      0 => {
        let mesh = arg_refs[0].resolve(args, &kwargs).as_mesh().unwrap();
        let mut new_mesh = (*mesh.mesh).clone();
        for vtx in new_mesh.vertices.values_mut() {
          vtx.position = (mesh.transform * vtx.position.push(1.)).xyz();
        }
        Ok(Value::Mesh(Rc::new(MeshHandle::new(Rc::new(new_mesh)))))
      }
      _ => unimplemented!(),
    },
    "vec3" => match def_ix {
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
    },
    "join" => match def_ix {
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
        })))
      }
      _ => unimplemented!(),
    },
    "union" => eval_mesh_boolean(def_ix, arg_refs, args, kwargs, ctx, MeshBooleanOp::Union),
    "difference" => eval_mesh_boolean(
      def_ix,
      arg_refs,
      args,
      kwargs,
      ctx,
      MeshBooleanOp::Difference,
    ),
    "intersect" => eval_mesh_boolean(
      def_ix,
      arg_refs,
      args,
      kwargs,
      ctx,
      MeshBooleanOp::Intersection,
    ),
    "fold" => {
      let initial_val = arg_refs[0].resolve(args, &kwargs).clone();
      let fn_value = arg_refs[1].resolve(args, &kwargs).as_callable().unwrap();
      let sequence = arg_refs[2].resolve(args, &kwargs).as_sequence().unwrap();

      ctx.fold(initial_val, fn_value, sequence.clone_box())
    }
    "reduce" => {
      let fn_value = arg_refs[0].resolve(args, &kwargs).as_callable().unwrap();
      let seq = arg_refs[1].resolve(args, &kwargs).as_sequence().unwrap();

      ctx.reduce(fn_value, seq.clone_box())
    }
    "map" => {
      let fn_value = arg_refs[0].resolve(args, &kwargs);
      let seq = arg_refs[1].resolve(args, &kwargs);

      map_impl(def_ix, fn_value, seq)
    }
    "filter" => match def_ix {
      0 => {
        let fn_value = arg_refs[0].resolve(args, &kwargs).as_callable().unwrap();
        let sequence = arg_refs[1].resolve(args, &kwargs).as_sequence().unwrap();

        Ok(Value::Sequence(Box::new(FilterSeq {
          cb: fn_value.clone(),
          inner: sequence.clone_box(),
        })))
      }
      _ => unimplemented!(),
    },
    "take" => match def_ix {
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
    },
    "skip" => match def_ix {
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
    },
    "take_while" => match def_ix {
      0 => {
        let fn_value = arg_refs[0].resolve(args, &kwargs).as_callable().unwrap();
        let sequence = arg_refs[1].resolve(args, &kwargs).as_sequence().unwrap();
        Ok(Value::Sequence(Box::new(TakeWhileSeq {
          cb: fn_value.clone(),
          inner: sequence.clone_box(),
        })))
      }
      _ => unimplemented!(),
    },
    "skip_while" => match def_ix {
      0 => {
        let fn_value = arg_refs[0].resolve(args, &kwargs).as_callable().unwrap();
        let sequence = arg_refs[1].resolve(args, &kwargs).as_sequence().unwrap();
        Ok(Value::Sequence(Box::new(SkipWhileSeq {
          cb: fn_value.clone(),
          inner: sequence.clone_box(),
        })))
      }
      _ => unimplemented!(),
    },
    "chain" => match def_ix {
      0 => {
        let seqs = arg_refs[0]
          .resolve(args, &kwargs)
          .as_sequence()
          .unwrap()
          .clone_box();
        Ok(Value::Sequence(Box::new(ChainSeq::new(ctx, seqs)?)))
      }
      _ => unimplemented!(),
    },
    "first" => match def_ix {
      0 => {
        let sequence = arg_refs[0].resolve(args, &kwargs).as_sequence().unwrap();
        let mut iter = sequence.clone_box().consume(ctx);
        match iter.next() {
          Some(res) => res,
          None => Ok(Value::Nil),
        }
      }
      _ => unimplemented!(),
    },
    "any" => match def_ix {
      0 => {
        let sequence = arg_refs[0].resolve(args, &kwargs).as_sequence().unwrap();
        let cb = arg_refs[1].resolve(args, &kwargs).as_callable().unwrap();
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
    },
    "all" => match def_ix {
      0 => {
        let sequence = arg_refs[0].resolve(args, &kwargs).as_sequence().unwrap();
        let cb = arg_refs[1].resolve(args, &kwargs).as_callable().unwrap();
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
    },
    "neg" => {
      let val = arg_refs[0].resolve(args, &kwargs);
      neg_impl(def_ix, val)
    }
    "pos" => {
      let val = arg_refs[0].resolve(args, &kwargs);
      pos_impl(def_ix, val)
    }
    "abs" => match def_ix {
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
    },
    "sqrt" => match def_ix {
      0 => {
        let value = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        Ok(Value::Float(value.sqrt()))
      }
      _ => unimplemented!(),
    },
    "add" => {
      let lhs = arg_refs[0].resolve(args, &kwargs);
      let rhs = arg_refs[1].resolve(args, &kwargs);
      add_impl(def_ix, lhs, rhs)
    }
    "sub" => {
      let lhs = arg_refs[0].resolve(args, &kwargs);
      let rhs = arg_refs[1].resolve(args, &kwargs);
      sub_impl(ctx, def_ix, lhs, rhs)
    }
    "mul" => {
      let lhs = arg_refs[0].resolve(args, &kwargs);
      let rhs = arg_refs[1].resolve(args, &kwargs);
      mul_impl(def_ix, lhs, rhs)
    }
    "div" => {
      let lhs = arg_refs[0].resolve(args, &kwargs);
      let rhs = arg_refs[1].resolve(args, &kwargs);
      div_impl(def_ix, lhs, rhs)
    }
    "mod" => {
      let lhs = arg_refs[0].resolve(args, &kwargs);
      let rhs = arg_refs[1].resolve(args, &kwargs);
      mod_impl(def_ix, lhs, rhs)
    }
    "max" => match def_ix {
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
    },
    "min" => match def_ix {
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
    },
    "float" => match def_ix {
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
    },
    "int" => match def_ix {
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
    },
    "gte" => eval_numeric_bool_op(def_ix, arg_refs, args, kwargs, BoolOp::Gte),
    "lte" => eval_numeric_bool_op(def_ix, arg_refs, args, kwargs, BoolOp::Lte),
    "gt" => eval_numeric_bool_op(def_ix, arg_refs, args, kwargs, BoolOp::Gt),
    "lt" => eval_numeric_bool_op(def_ix, arg_refs, args, kwargs, BoolOp::Lt),
    "eq" => {
      let lhs = arg_refs[0].resolve(args, &kwargs);
      let rhs = arg_refs[1].resolve(args, &kwargs);
      eq_impl(def_ix, lhs, rhs)
    }
    "neq" => {
      let lhs = arg_refs[0].resolve(args, &kwargs);
      let rhs = arg_refs[1].resolve(args, &kwargs);
      neq_impl(def_ix, lhs, rhs)
    }
    "and" => {
      let lhs = arg_refs[0].resolve(args, &kwargs);
      let rhs = arg_refs[1].resolve(args, &kwargs);
      and_impl(ctx, def_ix, lhs, rhs)
    }
    "or" => {
      let lhs = arg_refs[0].resolve(args, &kwargs);
      let rhs = arg_refs[1].resolve(args, &kwargs);
      or_impl(ctx, def_ix, lhs, rhs)
    }
    "not" => {
      let val = arg_refs[0].resolve(args, &kwargs);
      not_impl(def_ix, val)
    }
    "bit_and" => {
      let lhs = arg_refs[0].resolve(args, &kwargs);
      let rhs = arg_refs[1].resolve(args, &kwargs);
      bit_and_impl(ctx, def_ix, lhs, rhs)
    }
    "bit_or" => {
      let lhs = arg_refs[0].resolve(args, &kwargs);
      let rhs = arg_refs[1].resolve(args, &kwargs);
      bit_or_impl(ctx, def_ix, lhs, rhs)
    }
    "sin" => match def_ix {
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
    },
    "cos" => match def_ix {
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
    },
    "tan" => match def_ix {
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
    },
    "pow" => match def_ix {
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
    },
    "trunc" => match def_ix {
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
    },
    "fract" => match def_ix {
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
    },
    "round" => match def_ix {
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
    },
    "ceil" => match def_ix {
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
    },
    "floor" => match def_ix {
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
    },
    "fix_float" => match def_ix {
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
    },
    "rad2deg" => match def_ix {
      0 => {
        let value = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        Ok(Value::Float(value.to_degrees()))
      }
      _ => unimplemented!(),
    },
    "deg2rad" => match def_ix {
      0 => {
        let value = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        Ok(Value::Float(value.to_radians()))
      }
      _ => unimplemented!(),
    },
    "lerp" => match def_ix {
      0 => {
        let a = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_vec3().unwrap();
        let t = arg_refs[2].resolve(args, &kwargs).as_float().unwrap();
        Ok(Value::Vec3(a.lerp(b, t)))
      }
      1 => {
        let a = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
        let t = arg_refs[2].resolve(args, &kwargs).as_float().unwrap();
        Ok(Value::Float(a + (b - a) * t))
      }
      _ => unimplemented!(),
    },
    "print" => {
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
    "render" => match def_ix {
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
    },
    "point_distribute" => {
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
    "compose" => {
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
    "warp" => match def_ix {
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
              "warp function must return Vec3, got: {:?}",
              warped_pos
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
        })))
      }
      _ => unimplemented!(),
    },
    "tessellate" => match def_ix {
      0 => {
        let target_edge_length = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        if target_edge_length <= 0. {
          return Err(ErrorStack::new("`target_edge_length` must be > 0"));
        }
        let mesh = arg_refs[1].resolve(args, &kwargs).as_mesh().unwrap();
        let transform = mesh.transform.clone();

        let mut mesh = (*mesh.mesh).clone();
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
        })))
      }
      _ => unimplemented!(),
    },
    "intersects" => match def_ix {
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
    },
    "intersects_ray" => match def_ix {
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
    },
    "connected_components" => match def_ix {
      0 => {
        let mesh = arg_refs[0].resolve(args, &kwargs).as_mesh().unwrap();
        let transform = mesh.transform.clone();
        let mesh = Rc::clone(&mesh.mesh);
        let mut components: Vec<Vec<FaceKey>> = mesh.connected_components();
        components.sort_unstable_by_key(|c| Reverse(c.len()));
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
            })))
          }),
        })))
      }
      _ => unimplemented!(),
    },
    "len" => match def_ix {
      0 => {
        let v = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
        Ok(Value::Float(v.magnitude()))
      }
      _ => unimplemented!(),
    },
    "distance" => match def_ix {
      0 => {
        let a = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_vec3().unwrap();
        Ok(Value::Float((*a - *b).magnitude()))
      }
      _ => unimplemented!(),
    },
    "normalize" => match def_ix {
      0 => {
        let v = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
        Ok(Value::Vec3(v.normalize()))
      }
      _ => unimplemented!(),
    },
    "bezier3d" => match def_ix {
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
    },
    "extrude_pipe" => match def_ix {
      0 => {
        let radius = arg_refs[0].resolve(args, &kwargs);
        let resolution = arg_refs[1].resolve(args, &kwargs).as_int().unwrap() as usize;
        let path = arg_refs[2].resolve(args, &kwargs).as_sequence().unwrap();
        let close_ends = arg_refs[3].resolve(args, &kwargs).as_bool().unwrap();

        enum Twist<'a> {
          Const(f32),
          Dyn(&'a Callable),
        }

        let twist = match arg_refs[4].resolve(args, &kwargs) {
          Value::Float(f) => Twist::Const(*f),
          Value::Int(i) => Twist::Const(*i as f32),
          Value::Callable(cb) => Twist::Dyn(cb),
          _ => {
            return Err(ErrorStack::new(format!(
              "Invalid twist argument for `extrude_pipe`; expected Numeric or Callable, found: \
               {:?}",
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

        let mesh = match radius {
          _ if let Some(radius) = radius.as_float() => {
            let get_radius = |_, _| Ok(radius);
            match twist {
              Twist::Const(twist) => {
                extrude_pipe(get_radius, resolution, path, close_ends, |_, _| Ok(twist))?
              }
              Twist::Dyn(get_twist) => extrude_pipe(
                get_radius,
                resolution,
                path,
                close_ends,
                build_twist_or_radius_callable(ctx, get_twist, "twist"),
              )?,
            }
          }
          _ if let Some(get_radius) = radius.as_callable() => {
            let get_radius = build_twist_or_radius_callable(ctx, get_radius, "radius");
            match twist {
              Twist::Const(twist) => {
                extrude_pipe(get_radius, resolution, path, close_ends, |_, _| Ok(twist))?
              }
              Twist::Dyn(get_twist) => extrude_pipe(
                get_radius,
                resolution,
                path,
                close_ends,
                build_twist_or_radius_callable(ctx, get_twist, "twist"),
              )?,
            }
          }
          _ => {
            return Err(ErrorStack::new(format!(
              "Invalid radius argument for `extrude_pipe`; expected Float, Int, or Callable, \
               found: {radius:?}",
            )))
          }
        };
        Ok(Value::Mesh(Rc::new(MeshHandle {
          mesh: Rc::new(mesh),
          transform: Matrix4::identity(),
          manifold_handle: Rc::new(ManifoldHandle::new(0)),
          aabb: RefCell::new(None),
          trimesh: RefCell::new(None),
        })))
      }
      _ => unimplemented!(),
    },
    "torus_knot_path" => match def_ix {
      0 => {
        let radius = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        let tube_radius = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
        let p = arg_refs[2].resolve(args, &kwargs).as_int().unwrap() as usize;
        let q = arg_refs[3].resolve(args, &kwargs).as_int().unwrap() as usize;
        let count = arg_refs[4].resolve(args, &kwargs).as_int().unwrap() as usize;

        Ok(Value::Sequence(Box::new(IteratorSeq {
          inner: build_torus_knot_path(radius, tube_radius, p, q, count)
            .map(|v| Ok(Value::Vec3(v))),
        })))
      }
      _ => unreachable!(),
    },
    "simplify" => match def_ix {
      0 => {
        let tolerance = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        let mesh = arg_refs[1].resolve(args, &kwargs).as_mesh().unwrap();

        let out_mesh_handle = simplify_mesh(&mesh, tolerance)
          .map_err(|err| err.wrap("Error in `simplify` function"))?;
        Ok(Value::Mesh(Rc::new(out_mesh_handle)))
      }
      _ => unimplemented!(),
    },
    "convex_hull" => match def_ix {
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
    },
    "verts" => match def_ix {
      0 => {
        let mesh = arg_refs[0]
          .resolve(args, &kwargs)
          .as_mesh()
          .unwrap()
          .clone(false, false, true);
        Ok(Value::Sequence(Box::new(MeshVertsSeq { mesh })))
      }
      _ => unimplemented!(),
    },
    "randf" => match def_ix {
      0 => {
        let min = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        let max = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
        Ok(Value::Float(ctx.rng().gen_range(min..max)))
      }
      1 => Ok(Value::Float(ctx.rng().gen())),
      _ => unimplemented!(),
    },
    "randv" => match def_ix {
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
    },
    "randi" => match def_ix {
      0 => {
        let min = arg_refs[0].resolve(args, &kwargs).as_int().unwrap();
        let max = arg_refs[1].resolve(args, &kwargs).as_int().unwrap();
        Ok(Value::Int(ctx.rng().gen_range(min..max)))
      }
      1 => Ok(Value::Int(ctx.rng().gen())),
      _ => unimplemented!(),
    },
    "fbm" => match def_ix {
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
    },
    "call" => match def_ix {
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
    },
    "mesh" => match def_ix {
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
                "`verts` sequence produced invalid value in call to `mesh`.  Expected Vec3, \
                 found: {v:?}",
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
                "Found vtx ix {i} in element {ix} of `indices` sequence passed to `mesh`, but \
                 there are only {} vertices in the `verts` sequence",
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
        })))
      }
      _ => unimplemented!(),
    },
    "dir_light" => match def_ix {
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
    },
    "ambient_light" => match def_ix {
      0 => {
        let color = arg_refs[0].resolve(args, &kwargs); // vec3 or int
        let intensity = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();

        let light = AmbientLight::new(color, intensity)
          .map_err(|err| ErrorStack::new(format!("Error creating ambient light: {err}")))?;
        Ok(Value::Light(Box::new(Light::Ambient(light))))
      }
      _ => unimplemented!(),
    },
    "point_light" => match def_ix {
      0 => {
        todo!()
      }
      _ => unimplemented!(),
    },
    "spot_light" => match def_ix {
      0 => {
        todo!()
      }
      _ => unimplemented!(),
    },
    _ => unimplemented!("Builtin function `{name}` not yet implemented"),
  }
}

// macro that creates a closure that calls `eval_builtin_fn` with a hard-coded name.  This is
// kind of a way of emulating const generics for `&'static str`, which isn't currenlty supported in
// Rust.
macro_rules! define_builtin_fn {
  ($name:ident) => {
    paste! {{
      fn [<builtin_ $name _impl>] (
        def_ix: usize,
        arg_refs: &[ArgRef],
        args: &[Value],
        kwargs: &FxHashMap<String, Value>,
        ctx: &EvalCtx,
      ) -> Result<Value, ErrorStack> {
        eval_builtin_fn(stringify!($name), def_ix, arg_refs, args, kwargs, ctx)
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
  "box" => define_builtin_fn!(box),
  "icosphere" => define_builtin_fn!(icosphere),
  "cylinder" => define_builtin_fn!(cylinder),
  "sphere" => define_builtin_fn!(sphere),
  "translate" => define_builtin_fn!(translate),
  "scale" => define_builtin_fn!(scale),
  "rot" => define_builtin_fn!(rot),
  "look_at" => define_builtin_fn!(look_at),
  "apply_transforms" => define_builtin_fn!(apply_transforms),
  "vec3" => define_builtin_fn!(vec3),
  "join" => define_builtin_fn!(join),
  "union" => define_builtin_fn!(union),
  "difference" => define_builtin_fn!(difference),
  "intersect" => define_builtin_fn!(intersect),
  "fold" => define_builtin_fn!(fold),
  "reduce" => define_builtin_fn!(reduce),
  "map" => define_builtin_fn!(map),
  "filter" => define_builtin_fn!(filter),
  "take" => define_builtin_fn!(take),
  "skip" => define_builtin_fn!(skip),
  "take_while" => define_builtin_fn!(take_while),
  "skip_while" => define_builtin_fn!(skip_while),
  "chain" => define_builtin_fn!(chain),
  "first" => define_builtin_fn!(first),
  "any" => define_builtin_fn!(any),
  "all" => define_builtin_fn!(all),
  "neg" => define_builtin_fn!(neg),
  "pos" => define_builtin_fn!(pos),
  "abs" => define_builtin_fn!(abs),
  "sqrt" => define_builtin_fn!(sqrt),
  "add" => define_builtin_fn!(add),
  "sub" => define_builtin_fn!(sub),
  "mul" => define_builtin_fn!(mul),
  "div" => define_builtin_fn!(div),
  "mod" => define_builtin_fn!(mod),
  "max" => define_builtin_fn!(max),
  "min" => define_builtin_fn!(min),
  "float" => define_builtin_fn!(float),
  "int" => define_builtin_fn!(int),
  "gte" => define_builtin_fn!(gte),
  "lte" => define_builtin_fn!(lte),
  "gt" => define_builtin_fn!(gt),
  "lt" => define_builtin_fn!(lt),
  "eq" => define_builtin_fn!(eq),
  "neq" => define_builtin_fn!(neq),
  "and" => define_builtin_fn!(and),
  "or" => define_builtin_fn!(or),
  "not" => define_builtin_fn!(not),
  "bit_and" => define_builtin_fn!(bit_and),
  "bit_or" => define_builtin_fn!(bit_or),
  "sin" => define_builtin_fn!(sin),
  "cos" => define_builtin_fn!(cos),
  "tan" => define_builtin_fn!(tan),
  "pow" => define_builtin_fn!(pow),
  "trunc" => define_builtin_fn!(trunc),
  "fract" => define_builtin_fn!(fract),
  "round" => define_builtin_fn!(round),
  "ceil" => define_builtin_fn!(ceil),
  "floor" => define_builtin_fn!(floor),
  "fix_float" => define_builtin_fn!(fix_float),
  "rad2deg" => define_builtin_fn!(rad2deg),
  "deg2rad" => define_builtin_fn!(deg2rad),
  "lerp" => define_builtin_fn!(lerp),
  "print" => define_builtin_fn!(print),
  "render" => define_builtin_fn!(render),
  "point_distribute" => define_builtin_fn!(point_distribute),
  "compose" => define_builtin_fn!(compose),
  "warp" => define_builtin_fn!(warp),
  "tessellate" => define_builtin_fn!(tessellate),
  "connected_components" => define_builtin_fn!(connected_components),
  "intersects" => define_builtin_fn!(intersects),
  "intersects_ray" => define_builtin_fn!(intersects_ray),
  "len" => define_builtin_fn!(len),
  "distance" => define_builtin_fn!(distance),
  "normalize" => define_builtin_fn!(normalize),
  "bezier3d" => define_builtin_fn!(bezier3d),
  "extrude_pipe" => define_builtin_fn!(extrude_pipe),
  "torus_knot_path" => define_builtin_fn!(torus_knot_path),
  "simplify" => define_builtin_fn!(simplify),
  "convex_hull" => define_builtin_fn!(convex_hull),
  "verts" => define_builtin_fn!(verts),
  "randf" => define_builtin_fn!(randf),
  "randv" => define_builtin_fn!(randv),
  "randi" => define_builtin_fn!(randi),
  "fbm" => define_builtin_fn!(fbm),
  "call" => define_builtin_fn!(call),
  "mesh" => define_builtin_fn!(mesh),
  "dir_light" => define_builtin_fn!(dir_light),
  "ambient_light" => define_builtin_fn!(ambient_light),
  "point_light" => define_builtin_fn!(point_light),
  "spot_light" => define_builtin_fn!(spot_light),
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
