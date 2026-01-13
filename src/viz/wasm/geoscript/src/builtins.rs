use noise::RangeFunction;
use paste::paste;
#[cfg(target_arch = "wasm32")]
use rand_pcg::Pcg32;
use smallvec::SmallVec;
use std::cmp::Reverse;
use std::marker::ConstParamTy;
use std::rc::Rc;
use std::str::FromStr;
use std::{cell::RefCell, fmt::Display};

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
#[cfg(target_arch = "wasm32")]
use rand::{RngCore, SeedableRng};

use crate::materials::Material;
use crate::mesh_ops::extrude_pipe::PipeRadius;
use crate::mesh_ops::mesh_ops::{
  alpha_wrap_mesh, alpha_wrap_points, delaunay_remesh, get_geodesics_loaded,
  get_text_to_path_cached_mesh, isotropic_remesh, remesh_planar_patches, smooth_mesh, SmoothType,
};
use crate::mesh_ops::voxels::sample_voxels;
use crate::noise::{
  curl_noise_2d, curl_noise_3d, fbm_1d, fbm_2d, ridged_2d, ridged_3d, worley_noise_2d,
  worley_noise_3d, WorleyReturnType,
};
use crate::path_building::build_lissajous_knot_path;
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
  noise::fbm_3d,
  path_building::{build_torus_knot_path, cubic_bezier_3d_path, superellipse_path},
  seq::{
    ChainSeq, EagerSeq, FilterSeq, FlattenSeq, IteratorSeq, MeshVertsSeq, PointDistributeSeq,
    ScanSeq, SkipSeq, SkipWhileSeq, TakeSeq, TakeWhileSeq,
  },
  seq_as_eager, ArgRef, Callable, ComposedFn, ErrorStack, EvalCtx, MapSeq, Value, Vec2,
};
use crate::{ManifoldHandle, MeshHandle, Sequence, Sym, EMPTY_KWARGS};

pub(crate) mod fn_defs;
pub(crate) mod trace_path;

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
  "teapot" => "utah_teapot",
  "bunny" => "stanford_bunny",
  // "monkey" => "suzanne",
  "push" => "append",
  "randv3" => "randv",
  "string" => "str",
  "sign" => "signum",
  "worley" => "worley_noise",
  "quad_bezier" => "quadratic_bezier",
  "smooth_quad_bezier" => "smooth_quadratic_bezier",
  "smooth_bezier" => "smooth_cubic_bezier",
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
  kwargs: &FxHashMap<Sym, Value>,
  op: BoolOp,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let a = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
      let b = arg_refs[1].resolve(args, kwargs).as_int().unwrap();
      let result = match op {
        BoolOp::Gte => a >= b,
        BoolOp::Lte => a <= b,
        BoolOp::Gt => a > b,
        BoolOp::Lt => a < b,
      };
      Ok(Value::Bool(result))
    }
    1 => {
      let a = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let b = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
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

            combined.vertices.insert(Vertex {
              position: transformed_pos.xyz(),
              displacement_normal: old_vtx.displacement_normal,
              shading_normal: old_vtx.shading_normal,
              edges: SmallVec::new(),
              _padding: Default::default(),
            })
          })
        });
        combined.add_face::<false>(new_vtx_keys, ());
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
        manifold_handle: Rc::new(ManifoldHandle::new_empty()),
        aabb: RefCell::new(maybe_combined_aabb),
        trimesh: RefCell::new(None),
        material: lhs.material.clone(),
      })))
    }
    5 => translate_impl(
      0,
      &[ArgRef::Positional(1), ArgRef::Positional(0)],
      &[lhs.clone(), rhs.clone()],
      EMPTY_KWARGS,
    ),
    // vec3 + float
    6 => {
      let a = lhs.as_vec3().unwrap();
      let b = rhs.as_float().unwrap();
      Ok(Value::Vec3(a + Vec3::new(b, b, b)))
    }
    // vec2 + vec2
    7 => {
      let a = lhs.as_vec2().unwrap();
      let b = rhs.as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(a.x + b.x, a.y + b.y)))
    }
    // vec2 + num
    8 => {
      let a = lhs.as_vec2().unwrap();
      let b = rhs.as_float().unwrap();
      Ok(Value::Vec2(a + Vec2::new(b, b)))
    }
    // string + string
    9 => {
      let a = lhs.as_str().unwrap();
      let b = rhs.as_str().unwrap();
      Ok(Value::String(format!("{a}{b}")))
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
        EMPTY_KWARGS,
        ctx,
        MeshBooleanOp::Difference,
      )
    }
    5 => translate_impl(
      0,
      &[ArgRef::Positional(1), ArgRef::Positional(0)],
      &[lhs.clone(), Value::Vec3(-rhs.as_vec3().unwrap())],
      EMPTY_KWARGS,
    ),
    // vec3 - float
    6 => {
      let a = lhs.as_vec3().unwrap();
      let b = rhs.as_float().unwrap();
      Ok(Value::Vec3(a - Vec3::new(b, b, b)))
    }
    // vec2 - vec2
    7 => {
      let a = lhs.as_vec2().unwrap();
      let b = rhs.as_vec2().unwrap();
      Ok(Value::Vec2(a - b))
    }
    // vec2 - num
    8 => {
      let a = lhs.as_vec2().unwrap();
      let b = rhs.as_float().unwrap();
      Ok(Value::Vec2(a - Vec2::new(b, b)))
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
      EMPTY_KWARGS,
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
    // float * vec2
    9 => {
      let a = lhs.as_float().unwrap();
      let b = rhs.as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(a * b.x, a * b.y)))
    }
    // float * vec3
    10 => {
      let a = lhs.as_float().unwrap();
      let b = rhs.as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(a * b.x, a * b.y, a * b.z)))
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
    5 => {
      // vec2 / vec2
      let a = lhs.as_vec2().unwrap();
      let b = rhs.as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(a.x / b.x, a.y / b.y)))
    }
    6 => {
      // vec2 / float
      let a = lhs.as_vec2().unwrap();
      let b = rhs.as_float().unwrap();
      Ok(Value::Vec2(a / b))
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

pub(crate) fn and_impl(def_ix: usize, lhs: &Value, rhs: &Value) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let a = lhs.as_bool().unwrap();
      let b = rhs.as_bool().unwrap();
      Ok(Value::Bool(a && b))
    }
    _ => unimplemented!(),
  }
}

pub(crate) fn or_impl(def_ix: usize, lhs: &Value, rhs: &Value) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let a = lhs.as_bool().unwrap();
      let b = rhs.as_bool().unwrap();
      Ok(Value::Bool(a || b))
    }
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
      EMPTY_KWARGS,
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
      EMPTY_KWARGS,
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

      Ok(Value::Sequence(Rc::new(MapSeq {
        cb: fn_value.clone(),
        inner: seq,
      })))
    }
    // map(fn, mesh), alias for warp
    1 => warp_impl(
      ctx,
      0,
      &[ArgRef::Positional(0), ArgRef::Positional(1)],
      &[fn_value.clone(), seq.clone()],
      EMPTY_KWARGS,
    ),
    _ => unimplemented!(),
  }
}

fn fold_while_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let init = arg_refs[0].resolve(args, kwargs);
      let cb = arg_refs[1].resolve(args, kwargs).as_callable().unwrap();
      let seq = arg_refs[2].resolve(args, kwargs).as_sequence().unwrap();

      let mut acc = init.clone();
      for (i, next) in seq.consume(ctx).enumerate() {
        let next = next.map_err(|err| {
          err.wrap("Error produced while consuming sequence passed to `fold_while`")
        })?;
        let out = ctx
          .invoke_callable(cb, &[acc.clone(), next, Value::Int(i as i64)], EMPTY_KWARGS)
          .map_err(|err| err.wrap("Error in user-provided callback to `fold_while`"))?;
        if out.is_nil() {
          return Ok(acc);
        } else {
          acc = out;
        }
      }

      Ok(acc)
    }
    _ => unimplemented!(),
  }
}

pub(crate) fn warp_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let warp_fn = arg_refs[0].resolve(args, kwargs).as_callable().unwrap();
      let mesh = arg_refs[1].resolve(args, kwargs).as_mesh().unwrap();

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
            EMPTY_KWARGS,
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
        transform: mesh.transform,
        manifold_handle: Rc::new(ManifoldHandle::new_empty()),
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
    4 => {
      // negate vec2
      let value = val.as_vec2().unwrap();
      Ok(Value::Vec2(-*value))
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
    2 => {
      // pass through vec2
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let (translation, obj) = match def_ix {
    0 => {
      let translation = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      let obj = arg_refs[1].resolve(args, kwargs);
      (*translation, obj)
    }
    1 => {
      let x = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let y = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      let z = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
      let translation = Vec3::new(x, y, z);
      let obj = arg_refs[3].resolve(args, kwargs);
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let (scale, mesh) = match def_ix {
    0 => {
      let x = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let y = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      let z = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
      (Vec3::new(x, y, z), arg_refs[3].resolve(args, kwargs))
    }
    1 => {
      let val = arg_refs[0].resolve(args, kwargs);
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

      let mesh = arg_refs[1].resolve(args, kwargs);
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
  _kwargs: &FxHashMap<Sym, Value>,
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
  _kwargs: &FxHashMap<Sym, Value>,
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let color = arg_refs[0].resolve(args, kwargs); // vec3 or int
      let intensity = arg_refs[1].resolve(args, kwargs).as_float().unwrap();

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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let target = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      let color = arg_refs[1].resolve(args, kwargs); // vec3 or int
      let intensity = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
      let cast_shadow = arg_refs[3].resolve(args, kwargs).as_bool().unwrap();
      let shadow_map_size = arg_refs[4].resolve(args, kwargs); // map or int
      let shadow_map_radius = arg_refs[5].resolve(args, kwargs).as_float().unwrap();
      let shadow_map_blur_samples = arg_refs[6].resolve(args, kwargs).as_int().unwrap() as usize;
      let shadow_map_type = arg_refs[7].resolve(args, kwargs).as_str().unwrap();
      let shadow_map_bias = arg_refs[8].resolve(args, kwargs).as_float().unwrap();
      let shadow_camera = arg_refs[9].resolve(args, kwargs).as_map().unwrap();
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let material = arg_refs[0]
        .resolve(args, kwargs)
        .as_material(ctx)
        .unwrap()?;
      let mesh = arg_refs[1].resolve(args, kwargs).as_mesh().unwrap();

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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let material = arg_refs[0]
        .resolve(args, kwargs)
        .as_material(ctx)
        .unwrap()?;
      ctx.default_material.replace(Some(material.clone()));
      Ok(Value::Nil)
    }
    _ => unimplemented!(),
  }
}

#[cfg(target_arch = "wasm32")]
fn set_rng_seed_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let seed = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
      unsafe {
        ctx
          .rng
          .replace(Pcg32::from_seed(std::mem::transmute((seed, seed))));
      };

      // pump the rng a few times because sometimes that seems to be necessary to get good results
      let _ = ctx.rng().next_u64();
      let _ = ctx.rng().next_u64();
      let _ = ctx.rng().next_u64();

      Ok(Value::Nil)
    }
    _ => unimplemented!(),
  }
}

#[cfg(not(target_arch = "wasm32"))]
fn set_rng_seed_impl(
  _ctx: &EvalCtx,
  _def_ix: usize,
  _arg_refs: &[ArgRef],
  _args: &[Value],
  _kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  Ok(Value::Nil)
}

fn set_sharp_angle_threshold_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let angle_degrees = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      ctx.sharp_angle_threshold_degrees.replace(angle_degrees);
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let verts = arg_refs[0].resolve(args, kwargs).as_sequence().unwrap();
      let indices = arg_refs[1].resolve(args, kwargs).as_sequence().unwrap();

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
        manifold_handle: Rc::new(ManifoldHandle::new_empty()),
        aabb: RefCell::new(None),
        trimesh: RefCell::new(None),
        material: None,
      })))
    }
    1 => Ok(Value::Mesh(Rc::new(MeshHandle {
      mesh: Rc::new(LinkedMesh::default()),
      transform: Matrix4::identity(),
      manifold_handle: Rc::new(ManifoldHandle::new_empty()),
      aabb: RefCell::new(None),
      trimesh: RefCell::new(None),
      material: None,
    }))),
    _ => unimplemented!(),
  }
}

fn call_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let callable = arg_refs[0].resolve(args, kwargs).as_callable().unwrap();
      ctx.invoke_callable(callable, &[], EMPTY_KWARGS)
    }
    1 => {
      let callable = arg_refs[0].resolve(args, kwargs).as_callable().unwrap();
      let call_args = arg_refs[1].resolve(args, kwargs).as_sequence().unwrap();
      let args = call_args.consume(ctx).collect::<Result<Vec<_>, _>>()?;
      ctx.invoke_callable(callable, &args, EMPTY_KWARGS)
    }
    _ => unimplemented!(),
  }
}

fn fbm_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let pos = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Float(fbm_3d(0, 4, 1., 0.5, 2., *pos)))
    }
    1 => {
      let seed = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
      if seed < 0 || seed > u32::MAX as i64 {
        return Err(ErrorStack::new(format!(
          "Seed for fbm must be in range [0, {}], found: {seed}",
          u32::MAX
        )));
      }
      let seed = seed as u32;
      let octaves = arg_refs[1].resolve(args, kwargs).as_int().unwrap() as usize;
      let frequency = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
      let lacunarity = arg_refs[3].resolve(args, kwargs).as_float().unwrap();
      let persistence = arg_refs[4].resolve(args, kwargs).as_float().unwrap();
      let pos = arg_refs[5].resolve(args, kwargs).as_vec3().unwrap();

      Ok(Value::Float(fbm_3d(
        seed,
        octaves,
        frequency,
        persistence,
        lacunarity,
        *pos,
      )))
    }
    2 => {
      let pos = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Float(fbm_2d(0, 4, 1., 0.5, 2., *pos)))
    }
    3 => {
      let seed = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
      if seed < 0 || seed > u32::MAX as i64 {
        return Err(ErrorStack::new(format!(
          "Seed for fbm must be in range [0, {}], found: {seed}",
          u32::MAX
        )));
      }
      let seed = seed as u32;
      let octaves = arg_refs[1].resolve(args, kwargs).as_int().unwrap() as usize;
      let frequency = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
      let lacunarity = arg_refs[3].resolve(args, kwargs).as_float().unwrap();
      let persistence = arg_refs[4].resolve(args, kwargs).as_float().unwrap();
      let pos = arg_refs[5].resolve(args, kwargs).as_vec2().unwrap();

      Ok(Value::Float(fbm_2d(
        seed,
        octaves,
        frequency,
        persistence,
        lacunarity,
        *pos,
      )))
    }
    4 => {
      let pos = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(fbm_1d(0, 4, 1., 0.5, 2., pos)))
    }
    5 => {
      let seed = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
      if seed < 0 || seed > u32::MAX as i64 {
        return Err(ErrorStack::new(format!(
          "Seed for fbm must be in range [0, {}], found: {seed}",
          u32::MAX
        )));
      }
      let seed = seed as u32;
      let octaves = arg_refs[1].resolve(args, kwargs).as_int().unwrap() as usize;
      let frequency = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
      let lacunarity = arg_refs[3].resolve(args, kwargs).as_float().unwrap();
      let persistence = arg_refs[4].resolve(args, kwargs).as_float().unwrap();
      let pos = arg_refs[5].resolve(args, kwargs).as_float().unwrap();

      Ok(Value::Float(fbm_1d(
        seed,
        octaves,
        frequency,
        persistence,
        lacunarity,
        pos,
      )))
    }
    _ => unimplemented!(),
  }
}

fn curl_noise_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let pos = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(curl_noise_3d(0, 4, 1., 0.5, 2., *pos)))
    }
    1 => {
      let seed = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
      if seed < 0 || seed > u32::MAX as i64 {
        return Err(ErrorStack::new(format!(
          "Seed for fbm must be in range [0, {}], found: {seed}",
          u32::MAX
        )));
      }
      let seed = seed as u32;
      let octaves = arg_refs[1].resolve(args, kwargs).as_int().unwrap() as usize;
      let frequency = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
      let lacunarity = arg_refs[3].resolve(args, kwargs).as_float().unwrap();
      let persistence = arg_refs[4].resolve(args, kwargs).as_float().unwrap();
      let pos = arg_refs[5].resolve(args, kwargs).as_vec3().unwrap();

      Ok(Value::Vec3(curl_noise_3d(
        seed,
        octaves,
        frequency,
        persistence,
        lacunarity,
        *pos,
      )))
    }
    2 => {
      let pos = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(curl_noise_2d(0, 4, 1., 0.5, 2., *pos)))
    }
    3 => {
      let seed = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
      if seed < 0 || seed > u32::MAX as i64 {
        return Err(ErrorStack::new(format!(
          "Seed for fbm must be in range [0, {}], found: {seed}",
          u32::MAX
        )));
      }
      let seed = seed as u32;
      let octaves = arg_refs[1].resolve(args, kwargs).as_int().unwrap() as usize;
      let frequency = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
      let lacunarity = arg_refs[3].resolve(args, kwargs).as_float().unwrap();
      let persistence = arg_refs[4].resolve(args, kwargs).as_float().unwrap();
      let pos = arg_refs[5].resolve(args, kwargs).as_vec2().unwrap();

      Ok(Value::Vec2(curl_noise_2d(
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

fn ridged_multifractal_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let pos = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Float(ridged_3d(0, 4, 1., 0.5, 2., 2., *pos)))
    }
    1 => {
      let seed = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
      if seed < 0 || seed > u32::MAX as i64 {
        return Err(ErrorStack::new(format!(
          "Seed for fbm must be in range [0, {}], found: {seed}",
          u32::MAX
        )));
      }
      let seed = seed as u32;
      let octaves = arg_refs[1].resolve(args, kwargs).as_int().unwrap() as usize;
      let frequency = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
      let lacunarity = arg_refs[3].resolve(args, kwargs).as_float().unwrap();
      let persistence = arg_refs[4].resolve(args, kwargs).as_float().unwrap();
      let gain = arg_refs[5].resolve(args, kwargs).as_float().unwrap();
      let pos = arg_refs[6].resolve(args, kwargs).as_vec3().unwrap();

      Ok(Value::Float(ridged_3d(
        seed,
        octaves,
        frequency,
        persistence,
        lacunarity,
        gain,
        *pos,
      )))
    }
    2 => {
      let pos = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Float(ridged_2d(0, 4, 1., 0.5, 2., 2., *pos)))
    }
    3 => {
      let seed = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
      if seed < 0 || seed > u32::MAX as i64 {
        return Err(ErrorStack::new(format!(
          "Seed for fbm must be in range [0, {}], found: {seed}",
          u32::MAX
        )));
      }
      let seed = seed as u32;
      let octaves = arg_refs[1].resolve(args, kwargs).as_int().unwrap() as usize;
      let frequency = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
      let lacunarity = arg_refs[3].resolve(args, kwargs).as_float().unwrap();
      let persistence = arg_refs[4].resolve(args, kwargs).as_float().unwrap();
      let gain = arg_refs[5].resolve(args, kwargs).as_float().unwrap();
      let pos = arg_refs[6].resolve(args, kwargs).as_vec2().unwrap();

      Ok(Value::Float(ridged_2d(
        seed,
        octaves,
        frequency,
        persistence,
        lacunarity,
        gain,
        *pos,
      )))
    }
    _ => unimplemented!(),
  }
}

fn worley_noise_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let seed = arg_refs[1].resolve(args, kwargs).as_int().unwrap();
  if seed < 0 || seed > u32::MAX as i64 {
    return Err(ErrorStack::new(format!(
      "Seed for worley_noise must be in range [0, {}], found: {seed}",
      u32::MAX
    )));
  }
  let seed = seed as u32;
  let range_fn = arg_refs[2].resolve(args, kwargs).as_str().unwrap();
  let range_fn = match range_fn {
    "euclidean" => RangeFunction::Euclidean,
    "euclidean_squared" => RangeFunction::EuclideanSquared,
    "manhattan" => RangeFunction::Manhattan,
    "chebyshev" => RangeFunction::Chebyshev,
    "quadratic" => RangeFunction::Quadratic,
    _ => {
      return Err(ErrorStack::new(format!(
        "Invalid range function for worley_noise: {range_fn:?}",
      )))
    }
  };
  let return_type = arg_refs[3].resolve(args, kwargs).as_str().unwrap();
  let return_type = WorleyReturnType::from_str(return_type).ok_or_else(|| {
    ErrorStack::new(format!(
      "Invalid return type for worley_noise: {return_type:?}",
    ))
  })?;

  match def_ix {
    0 => {
      let pos = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();

      Ok(Value::Float(worley_noise_3d(
        seed,
        *pos,
        range_fn,
        return_type,
      )))
    }
    1 => {
      let pos = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();

      Ok(Value::Float(worley_noise_2d(
        seed,
        *pos,
        range_fn,
        return_type,
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let min = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
      let max = arg_refs[1].resolve(args, kwargs).as_int().unwrap();

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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  fn invalid_bounds_err(min: impl Display, max: impl Display) -> ErrorStack {
    ErrorStack::new(format!(
      "Invalid range for rand_vec3: min ({min}) must be less than or equal to max ({max})"
    ))
  }

  match def_ix {
    0 => {
      let min = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      let max = arg_refs[1].resolve(args, kwargs).as_vec3().unwrap();
      if min == max {
        return Ok(Value::Vec3(*min));
      } else if min.x > max.x || min.y > max.y || min.z > max.z {
        return Err(invalid_bounds_err(min, max));
      }
      Ok(Value::Vec3(Vec3::new(
        ctx.rng().gen_range(min.x..max.x),
        ctx.rng().gen_range(min.y..max.y),
        ctx.rng().gen_range(min.z..max.z),
      )))
    }
    1 => {
      let min = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let max = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      if min > max {
        return Err(invalid_bounds_err(min, max));
      } else if min == max {
        return Ok(Value::Vec3(Vec3::new(min, min, min)));
      }
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let min = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let max = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let mesh = arg_refs[0]
        .resolve(args, kwargs)
        .as_mesh()
        .unwrap()
        .clone(false, false, true);
      let world_space = arg_refs[1].resolve(args, kwargs).as_bool().unwrap();

      let seq: Rc<dyn Sequence> = if world_space {
        Rc::new(MeshVertsSeq::<true> { mesh })
      } else {
        Rc::new(MeshVertsSeq::<false> { mesh })
      };
      Ok(Value::Sequence(seq))
    }
    _ => unimplemented!(),
  }
}

fn convex_hull_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let verts_seq = arg_refs[0].resolve(args, kwargs).as_sequence().unwrap();
      let verts = verts_seq
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
        .map_err(|err| err.wrap("Error in `convex_hull` function"))?;
      Ok(Value::Mesh(Rc::new(out_mesh)))
    }
    1 => {
      let mesh = arg_refs[0].resolve(args, kwargs).as_mesh().unwrap();
      let verts = mesh
        .mesh
        .vertices
        .values()
        .map(|v| (mesh.transform * v.position.push(1.)).xyz())
        .collect::<Vec<_>>();
      let out_mesh = convex_hull_from_verts(&verts)
        .map_err(|err| err.wrap("Error in `convex_hull` function"))?;
      Ok(Value::Mesh(Rc::new(out_mesh)))
    }
    _ => unimplemented!(),
  }
}

fn simplify_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let tolerance = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let mesh = arg_refs[1].resolve(args, kwargs).as_mesh().unwrap();

      let out_mesh_handle =
        simplify_mesh(mesh, tolerance).map_err(|err| err.wrap("Error in `simplify` function"))?;
      Ok(Value::Mesh(Rc::new(out_mesh_handle)))
    }
    _ => unimplemented!(),
  }
}

fn sample_voxels_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let dims = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      if dims.x <= 0. || dims.y <= 0. || dims.z <= 0. {
        return Err(ErrorStack::new(format!(
          "All dimensions passed to `voxelize` must be positive, found: {dims:?}",
        )));
      }
      let cb = arg_refs[1].resolve(args, kwargs).as_callable().unwrap();
      let materials: Option<Vec<Rc<Material>>> = match arg_refs[2].resolve(args, kwargs) {
        Value::Sequence(seq) => Some(
          seq
            .consume(ctx)
            .map(|res| match res {
              Ok(val) => val.as_material(ctx).unwrap(),
              Err(err) => Err(err),
            })
            .collect::<Result<Vec<_>, _>>()?,
        ),
        Value::Nil => None,
        _ => unreachable!(),
      };

      let cb = move |x: usize, y: usize, z: usize| -> Result<u8, ErrorStack> {
        let val = ctx.invoke_callable(
          &cb,
          &[
            Value::Int(x as i64),
            Value::Int(y as i64),
            Value::Int(z as i64),
          ],
          EMPTY_KWARGS,
        )?;
        match val {
          Value::Int(ix) => {
            if ix < 0 || ix > u8::MAX as i64 {
              Err(ErrorStack::new(format!(
                "cb passed to `voxelize` returned invalid material index: {ix}.  Must be in range \
                 [0, {}]",
                u8::MAX
              )))
            } else {
              Ok(ix as u8)
            }
          }
          Value::Nil => Ok(0),
          Value::Bool(b) => {
            if b {
              Ok(1)
            } else {
              Ok(0)
            }
          }
          _ => Err(ErrorStack::new(format!(
            "Voxel callback passed to `voxelize` must return an integer, boolean, or nil, found: \
             {val:?}",
          ))),
        }
      };

      let use_cgal_remeshing = match arg_refs[3].resolve(args, kwargs) {
        Value::Bool(b) => Some(*b),
        Value::Nil => None,
        _ => unreachable!(),
      };

      let fill_internal_voids = arg_refs[4].resolve(args, kwargs).as_bool().unwrap();

      let out_meshes = sample_voxels(
        [dims.x as usize, dims.y as usize, dims.z as usize],
        cb,
        fill_internal_voids,
        materials.unwrap_or_default(),
        use_cgal_remeshing,
      )?;

      if out_meshes.is_empty() {
        Ok(Value::Mesh(Rc::new(MeshHandle {
          mesh: Rc::new(LinkedMesh::default()),
          transform: Matrix4::identity(),
          manifold_handle: Rc::new(ManifoldHandle::new_empty()),
          aabb: RefCell::new(None),
          trimesh: RefCell::new(None),
          material: None,
        })))
      } else if out_meshes.len() == 1 {
        Ok(Value::Mesh(Rc::new(out_meshes.into_iter().next().unwrap())))
      } else {
        let seq: Rc<dyn Sequence> = Rc::new(EagerSeq {
          inner: out_meshes
            .into_iter()
            .map(|m| Value::Mesh(Rc::new(m)))
            .collect(),
        });
        Ok(Value::Sequence(seq))
      }
    }
    _ => unimplemented!(),
  }
}

fn fan_fill_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let path = arg_refs[0]
        .resolve(args, kwargs)
        .as_sequence()
        .unwrap()
        .consume(ctx)
        .map(|res| match res {
          Ok(Value::Vec3(v)) => Ok(v),
          Ok(val) => Err(ErrorStack::new(format!(
            "Expected Vec3 in sequence passed to `fan_fill`, found: {val:?}"
          ))),
          Err(err) => Err(err),
        })
        .collect::<Result<Vec<_>, _>>()?;
      let closed = arg_refs[1].resolve(args, kwargs).as_bool().unwrap();
      let flipped = arg_refs[2].resolve(args, kwargs).as_bool().unwrap();
      let center = match arg_refs[3].resolve(args, kwargs) {
        Value::Vec3(v) => Some(*v),
        Value::Nil => None,
        _ => None,
      };

      let mesh =
        fan_fill(&path, closed, flipped, center).map_err(|err| err.wrap("Error in `fan_fill`"))?;
      Ok(Value::Mesh(Rc::new(MeshHandle {
        mesh: Rc::new(mesh),
        transform: Matrix4::identity(),
        manifold_handle: Rc::new(ManifoldHandle::new_empty()),
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let mut contours = arg_refs[0]
        .resolve(args, kwargs)
        .as_sequence()
        .unwrap()
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
      let flipped = arg_refs[1].resolve(args, kwargs).as_bool().unwrap();
      let closed = arg_refs[2].resolve(args, kwargs).as_bool().unwrap();
      let cap_start = arg_refs[3].resolve(args, kwargs).as_bool().unwrap();
      let cap_end = arg_refs[4].resolve(args, kwargs).as_bool().unwrap();
      let cap_ends = arg_refs[5].resolve(args, kwargs).as_bool().unwrap();

      let cap_start = cap_ends || cap_start;
      let cap_end = cap_ends || cap_end;

      let mesh = stitch_contours(&mut contours, flipped, closed, cap_start, cap_end)
        .map_err(|err| err.wrap("Error in `stitch_contours`"))?;
      Ok(Value::Mesh(Rc::new(MeshHandle {
        mesh: Rc::new(mesh),
        transform: Matrix4::identity(),
        manifold_handle: Rc::new(ManifoldHandle::new_empty()),
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  if !get_geodesics_loaded() {
    return Err(ErrorStack::new_uninitialized_module("geodesics"));
  }

  match def_ix {
    0 => {
      let path = arg_refs[0]
        .resolve(args, kwargs)
        .as_sequence()
        .unwrap()
        .consume(ctx);
      let mesh = arg_refs[1].resolve(args, kwargs).as_mesh().unwrap();
      let world_space = arg_refs[2].resolve(args, kwargs).as_bool().unwrap();
      let full_path = arg_refs[3].resolve(args, kwargs).as_bool().unwrap();
      let start_pos_local_space = arg_refs[4].resolve(args, kwargs).as_vec3();
      let start_pos_local_space = start_pos_local_space
        .map(|v| v.as_slice())
        .unwrap_or_default();
      let up_dir_world_space = arg_refs[5].resolve(args, kwargs).as_vec3();
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

      Ok(Value::Sequence(Rc::new(IteratorSeq {
        inner: out_points_v3.into_iter().map(|v| Ok(Value::Vec3(v))),
      })))
    }
    _ => unimplemented!(),
  }
}

fn text_to_mesh_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let text = arg_refs[0].resolve(args, kwargs).as_str().unwrap();
      let font_family = arg_refs[1].resolve(args, kwargs).as_str().unwrap();
      let font_size = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
      let font_weight_val = arg_refs[3].resolve(args, kwargs);
      let font_weight = match font_weight_val {
        Value::Int(i) => {
          if *i < 100 || *i > 900 {
            return Err(ErrorStack::new(format!(
              "Invalid font_weight argument for `text_to_mesh`; expected value in range [100, \
               900], found: {i}"
            )));
          }
          Some(i.to_string())
        }
        Value::String(s) => Some(s.clone()),
        Value::Nil => None,
        _ => {
          return Err(ErrorStack::new(format!(
            "Invalid font_weight argument for `text_to_mesh`; expected Int, String, or Nil, \
             found: {font_weight_val:?}"
          )));
        }
      };
      let font_style_val = arg_refs[4].resolve(args, kwargs);
      let font_style = match font_style_val {
        Value::String(s) => Some(s.as_str().to_string()),
        Value::Nil => None,
        _ => {
          return Err(ErrorStack::new(format!(
            "Invalid font_style argument for `text_to_mesh`; expected String or Nil, found: \
             {font_style_val:?}"
          )));
        }
      };
      let letter_spacing = match arg_refs[5].resolve(args, kwargs) {
        Value::Float(f) => *f,
        Value::Int(i) => *i as f32,
        Value::Nil => 0.,
        other => {
          return Err(ErrorStack::new(format!(
            "Invalid letter_spacing argument for `text_to_mesh`; expected Float, Int, or Nil, \
             found: {other:?}"
          )));
        }
      };
      let width_val = arg_refs[6].resolve(args, kwargs);
      let width = match width_val {
        Value::Float(f) => Some(*f),
        Value::Int(i) => Some(*i as f32),
        Value::Nil => None,
        _ => {
          return Err(ErrorStack::new(format!(
            "Invalid width argument for `text_to_mesh`; expected Float, Int, or Nil, found: \
             {width_val:?}"
          )));
        }
      };
      let height_val = arg_refs[7].resolve(args, kwargs);
      let height = match height_val {
        Value::Float(f) => Some(*f),
        Value::Int(i) => Some(*i as f32),
        Value::Nil => None,
        _ => {
          return Err(ErrorStack::new(format!(
            "Invalid height argument for `text_to_mesh`; expected Float, Int, or Nil, found: \
             {height_val:?}"
          )));
        }
      };
      let depth_val = arg_refs[8].resolve(args, kwargs);
      let depth = match depth_val {
        Value::Float(f) => Some(*f),
        Value::Int(i) => Some(*i as f32),
        Value::Nil => None,
        _ => {
          return Err(ErrorStack::new(format!(
            "Invalid depth argument for `text_to_mesh`; expected Float, Int, or Nil, found: \
             {depth_val:?}"
          )));
        }
      };

      let mesh = get_text_to_path_cached_mesh(
        &text,
        &font_family,
        font_size,
        font_weight.as_ref().map(String::as_str).unwrap_or_default(),
        font_style.as_ref().map(String::as_str).unwrap_or_default(),
        letter_spacing,
        width.unwrap_or(0.),
        height.unwrap_or(0.),
        depth,
      )?;
      let Some(mesh) = mesh else {
        let args = [
          text.to_owned(),
          font_family.to_owned(),
          font_size.to_string(),
          font_weight.unwrap_or_default(),
          font_style.unwrap_or_default(),
          letter_spacing.to_string(),
          width.unwrap_or(0.).to_string(),
          height.unwrap_or(0.).to_string(),
          depth.unwrap_or(0.).to_string(),
        ];
        return Err(ErrorStack::new_uninitialized_module_with_args(
          "text_to_path",
          args.into_iter(),
        ));
      };
      Ok(Value::Mesh(Rc::new(MeshHandle {
        mesh: Rc::new(mesh),
        transform: Matrix4::identity(),
        manifold_handle: Rc::new(ManifoldHandle::new_empty()),
        aabb: RefCell::new(None),
        trimesh: RefCell::new(None),
        material: None,
      })))
    }
    _ => unimplemented!(),
  }
}

fn alpha_wrap_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let mesh = arg_refs[0].resolve(args, kwargs).as_mesh().unwrap();
      let alpha = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      let offset = arg_refs[2].resolve(args, kwargs).as_float().unwrap();

      let out = alpha_wrap_mesh(mesh, alpha, offset)
        .map_err(|err| err.wrap("Error in `alpha_wrap` function"))?;
      Ok(Value::Mesh(Rc::new(out)))
    }
    1 => {
      // TODO: would be good to create a helper function for this
      let points = arg_refs[0]
        .resolve(args, kwargs)
        .as_sequence()
        .unwrap()
        .consume(ctx)
        .map(|res| match res {
          Ok(Value::Vec3(v)) => Ok(v),
          Ok(val) => Err(ErrorStack::new(format!(
            "Expected Vec3 in sequence passed to `alpha_wrap`, found: {val:?}"
          ))),
          Err(err) => Err(err),
        })
        .collect::<Result<Vec<_>, _>>()?;
      let alpha = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      let offset = arg_refs[2].resolve(args, kwargs).as_float().unwrap();

      let out = alpha_wrap_points(&points, alpha, offset)
        .map_err(|err| err.wrap("Error in `alpha_wrap` function"))?;
      Ok(Value::Mesh(Rc::new(out)))
    }
    _ => unimplemented!(),
  }
}

fn smooth_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let mesh = arg_refs[0].resolve(args, kwargs).as_mesh().unwrap();
      let smooth_type = arg_refs[1].resolve(args, kwargs).as_str().unwrap();
      let iterations = arg_refs[2].resolve(args, kwargs).as_int().unwrap();
      let iterations = if iterations <= 0 {
        return Err(ErrorStack::new(format!(
          "Invalid iterations argument for `smooth`: {iterations}; must be > 0"
        )));
      } else {
        iterations as u32
      };

      let smooth_type = SmoothType::from_str(smooth_type)?;

      let out = smooth_mesh(mesh, smooth_type, iterations)
        .map_err(|err| err.wrap("Error in `smooth` function"))?;
      Ok(Value::Mesh(Rc::new(out)))
    }
    _ => unimplemented!(),
  }
}

fn remesh_planar_patches_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let mesh = arg_refs[0].resolve(args, kwargs).as_mesh().unwrap();
      let max_angle_deg = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      let max_offset = arg_refs[2].resolve(args, kwargs);
      let max_offset = match max_offset {
        _ if let Some(n) = max_offset.as_float() => n,
        Value::Nil => {
          let bbox = mesh.get_or_compute_aabb();
          let diag = (bbox.maxs - bbox.mins).magnitude();
          diag * 0.01
        }
        _ => {
          return Err(ErrorStack::new(format!(
            "Invalid max_offset argument for `remesh_planar_patches`; expected Float or Nil, \
             found: {max_offset:?}"
          )))
        }
      };

      let out = remesh_planar_patches(mesh, max_angle_deg, max_offset)
        .map_err(|err| err.wrap("Error in `remesh_planar_patches` function"))?;
      Ok(Value::Mesh(Rc::new(out)))
    }
    _ => unimplemented!(),
  }
}

fn isotropic_remesh_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let target_edge_length = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let mesh = arg_refs[1].resolve(args, kwargs).as_mesh().unwrap();
      let iterations = arg_refs[2].resolve(args, kwargs).as_int().unwrap();
      let iterations = if iterations <= 0 {
        return Err(ErrorStack::new(format!(
          "Invalid iterations argument for `isotropic_remesh`: {iterations}; must be > 0"
        )));
      } else {
        iterations as u32
      };
      let protect_borders = arg_refs[3].resolve(args, kwargs).as_bool().unwrap();
      let protect_sharp_edges = arg_refs[4].resolve(args, kwargs).as_bool().unwrap();
      let sharp_angle_threshold_degrees = arg_refs[5].resolve(args, kwargs).as_float().unwrap();

      let out = isotropic_remesh(
        mesh,
        target_edge_length,
        iterations,
        protect_borders,
        protect_sharp_edges,
        sharp_angle_threshold_degrees,
      )
      .map_err(|err| err.wrap("Error in `isotropic_remesh` function"))?;
      Ok(Value::Mesh(Rc::new(out)))
    }
    _ => unimplemented!(),
  }
}

fn delaunay_remesh_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let mesh = arg_refs[0].resolve(args, kwargs).as_mesh().unwrap();
      let facet_distance = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      let target_edge_length = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
      let protect_sharp_edges = arg_refs[3].resolve(args, kwargs).as_bool().unwrap();
      let sharp_angle_threshold_degrees = arg_refs[4].resolve(args, kwargs).as_float().unwrap();

      let out = delaunay_remesh(
        mesh,
        target_edge_length,
        facet_distance,
        protect_sharp_edges,
        sharp_angle_threshold_degrees,
      )
      .map_err(|err| err.wrap("Error in `delaunay_remesh` function"))?;
      Ok(Value::Mesh(Rc::new(out)))
    }
    _ => unimplemented!(),
  }
}

fn extrude_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let up = arg_refs[0].resolve(args, kwargs);
      let mesh = arg_refs[1].resolve(args, kwargs).as_mesh().unwrap();
      let mut out_mesh = (*mesh.mesh).clone();

      match up {
        Value::Vec3(up) => extrude(&mut out_mesh, |_| Ok(*up))?,
        Value::Callable(cb) => extrude(&mut out_mesh, |vtx| {
          let out = ctx
            .invoke_callable(cb, &[Value::Vec3(vtx)], EMPTY_KWARGS)
            .map_err(|err| {
              err.wrap("Error calling user-provided cb passed to `up` arg in `extrude`")
            })?;
          out.as_vec3().copied().ok_or_else(|| {
            ErrorStack::new(format!(
              "Expected Vec3 from user-provided cb passed to `up` arg in `extrude`, found: {out:?}"
            ))
          })
        })?,
        _ => {
          return Err(ErrorStack::new(format!(
            "Invalid up argument for `extrude`; expected Vec3 or Callable, found: {up:?}"
          )))
        }
      }

      Ok(Value::Mesh(Rc::new(MeshHandle {
        mesh: Rc::new(out_mesh),
        transform: mesh.transform,
        manifold_handle: Rc::new(ManifoldHandle::new_empty()),
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let radius = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let tube_radius = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      let p = arg_refs[2].resolve(args, kwargs).as_int().unwrap() as usize;
      let q = arg_refs[3].resolve(args, kwargs).as_int().unwrap() as usize;
      let count = arg_refs[4].resolve(args, kwargs).as_int().unwrap() as usize;

      Ok(Value::Sequence(Rc::new(IteratorSeq {
        inner: build_torus_knot_path(radius, tube_radius, p, q, count).map(|v| Ok(Value::Vec3(v))),
      })))
    }
    _ => unreachable!(),
  }
}

fn lissajous_knot_path_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let amp = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      let freq = arg_refs[1].resolve(args, kwargs).as_vec3().unwrap();
      let phase = arg_refs[2].resolve(args, kwargs).as_vec3().unwrap();
      let count = arg_refs[3].resolve(args, kwargs).as_int().unwrap();
      if count < 1 {
        return Err(ErrorStack::new(format!(
          "Invalid count for lissajous knot path: {count}; must be >= 1"
        )));
      }

      Ok(Value::Sequence(Rc::new(IteratorSeq {
        inner: build_lissajous_knot_path(*amp, *freq, *phase, count as usize)
          .map(|v| Ok(Value::Vec3(v))),
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let radius = arg_refs[0].resolve(args, kwargs);
      let resolution = arg_refs[1].resolve(args, kwargs).as_int().unwrap() as usize;
      let path = arg_refs[2].resolve(args, kwargs).as_sequence().unwrap();
      let close_ends = arg_refs[3].resolve(args, kwargs).as_bool().unwrap();
      let connect_ends = arg_refs[4].resolve(args, kwargs).as_bool().unwrap();

      enum Twist<'a> {
        Const(f32),
        Dyn(&'a Rc<Callable>),
      }

      let twist = match arg_refs[5].resolve(args, kwargs) {
        Value::Float(f) => Twist::Const(*f),
        Value::Int(i) => Twist::Const(*i as f32),
        Value::Callable(cb) => Twist::Dyn(cb),
        _ => {
          return Err(ErrorStack::new(format!(
            "Invalid twist argument for `extrude_pipe`; expected Numeric or Callable, found: {:?}",
            arg_refs[4].resolve(args, kwargs)
          )))
        }
      };

      fn build_twist_callable<'a>(
        ctx: &'a EvalCtx,
        get_twist: &'a Rc<Callable>,
      ) -> impl Fn(usize, Vec3) -> Result<f32, ErrorStack> + 'a {
        move |i, pos| {
          let out = ctx
            .invoke_callable(
              get_twist,
              &[Value::Int(i as i64), Value::Vec3(pos)],
              EMPTY_KWARGS,
            )
            .map_err(|err| {
              err.wrap("Error calling user-provided cb passed to `twist` arg in `extrude_pipe`")
            })?;
          out.as_float().ok_or_else(|| {
            ErrorStack::new(format!(
              "Expected Float from user-provided cb passed to `twist` arg in `extrude_pipe`, \
               found: {out:?}"
            ))
          })
        }
      }

      fn build_radius_callable<'a>(
        ctx: &'a EvalCtx,
        get_radius: &'a Rc<Callable>,
      ) -> impl Fn(usize, Vec3) -> Result<PipeRadius, ErrorStack> + 'a {
        move |i, pos| {
          let radius_for_ring = ctx
            .invoke_callable(
              get_radius,
              &[Value::Int(i as i64), Value::Vec3(pos)],
              EMPTY_KWARGS,
            )
            .map_err(|err| {
              err.wrap("Error calling user-provided cb passed to `radius` arg in `extrude_pipe`")
            })?;

          if let Some(radius) = radius_for_ring.as_float() {
            Ok(PipeRadius::constant(radius))
          } else if let Some(seq) = radius_for_ring.as_sequence() {
            let radii: Vec<f32> = seq
              .consume(ctx)
              .map(|res| match res {
                Ok(val) if let Some(f) = val.as_float() => Ok(f),
                Ok(val) => Err(ErrorStack::new(format!(
                  "Expected Int/Float in sequence returned from user-provided cb passed to \
                   `radius` arg in `extrude_pipe`, found: {val:?}"
                ))),
                Err(err) => Err(err),
              })
              .collect::<Result<_, _>>()?;
            Ok(PipeRadius::Explicit(radii))
          } else {
            Err(ErrorStack::new(format!(
              "Expected Num or Sequence from user-provided cb passed to `radius` arg in \
               `extrude_pipe`, found: {radius_for_ring:?}"
            )))
          }
        }
      }

      let path = path.consume(ctx).map(|res| match res {
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
          let get_radius = |_, _| Ok(PipeRadius::Constant(radius));
          match twist {
            Twist::Const(twist) => {
              extrude_pipe(get_radius, resolution, path, end_mode, |_, _| Ok(twist))?
            }
            Twist::Dyn(get_twist) => extrude_pipe(
              get_radius,
              resolution,
              path,
              end_mode,
              build_twist_callable(ctx, get_twist),
            )?,
          }
        }
        _ if let Some(get_radius) = radius.as_callable() => {
          let get_radius = build_radius_callable(ctx, get_radius);
          match twist {
            Twist::Const(twist) => {
              extrude_pipe(get_radius, resolution, path, end_mode, |_, _| Ok(twist))?
            }
            Twist::Dyn(get_twist) => extrude_pipe(
              get_radius,
              resolution,
              path,
              end_mode,
              build_twist_callable(ctx, get_twist),
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
        manifold_handle: Rc::new(ManifoldHandle::new_empty()),
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let p0 = *arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      let p1 = *arg_refs[1].resolve(args, kwargs).as_vec3().unwrap();
      let p2 = *arg_refs[2].resolve(args, kwargs).as_vec3().unwrap();
      let p3 = *arg_refs[3].resolve(args, kwargs).as_vec3().unwrap();
      let count = arg_refs[4].resolve(args, kwargs).as_int().unwrap() as usize;

      let curve = cubic_bezier_3d_path(p0, p1, p2, p3, count).map(|v| Ok(Value::Vec3(v)));
      Ok(Value::Sequence(Rc::new(IteratorSeq { inner: curve })))
    }
    _ => unimplemented!(),
  }
}

fn superellipse_path_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let width = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let height = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      let n = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
      let point_count = arg_refs[3].resolve(args, kwargs).as_int().unwrap() as usize;

      let curve = superellipse_path(width, height, n, point_count).map(|v| Ok(Value::Vec2(v)));
      Ok(Value::Sequence(Rc::new(IteratorSeq { inner: curve })))
    }
    _ => unimplemented!(),
  }
}

fn normalize_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let v = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(v.normalize()))
    }
    1 => {
      let v = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(v.normalize()))
    }
    _ => unimplemented!(),
  }
}

fn distance_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let a = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      let b = arg_refs[1].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Float((*a - *b).magnitude()))
    }
    1 => {
      let a = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      let b = arg_refs[1].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Float((*a - *b).magnitude()))
    }
    _ => unimplemented!(),
  }
}

fn len_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let v = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Float(v.magnitude()))
    }
    1 => {
      let v = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Float(v.magnitude()))
    }
    2 => {
      let v = arg_refs[0].resolve(args, kwargs).as_str().unwrap();
      Ok(Value::Int(v.chars().count() as i64))
    }
    3 => {
      let v = arg_refs[0].resolve(args, kwargs).as_sequence().unwrap();
      if let Some(eager) = seq_as_eager(&*v) {
        Ok(Value::Int(eager.inner.len() as i64))
      } else {
        let iter = v.consume(ctx);
        let mut len = 0;
        for res in iter {
          match res {
            Ok(_) => len += 1,
            Err(err) => return Err(err.wrap("Error evaluating sequence in `len` function")),
          }
        }
        Ok(Value::Int(len as i64))
      }
    }
    4 => {
      let m = arg_refs[0].resolve(args, kwargs).as_mesh().unwrap();
      Ok(Value::Int(m.mesh.vertices.len() as i64))
    }
    _ => unimplemented!(),
  }
}

fn chars_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let s = arg_refs[0]
        .resolve(args, kwargs)
        .as_str()
        .unwrap()
        .to_owned();

      #[derive(Clone)]
      struct OwnedChars {
        // struct fields are dropped in the order they're declared
        iter: std::str::Chars<'static>,
        #[allow(dead_code)]
        s: Rc<String>,
      }

      impl OwnedChars {
        fn new(s: String) -> Self {
          let s = Rc::new(s);
          // this is safe because the static reference is guaranteed to live as long as the `Rc`
          let iter = unsafe { std::mem::transmute::<&str, &'static str>(&*s) }.chars();
          OwnedChars { s, iter }
        }
      }

      impl Iterator for OwnedChars {
        type Item = char;

        fn next(&mut self) -> Option<Self::Item> {
          self.iter.next()
        }
      }

      let iter = OwnedChars::new(s);

      Ok(Value::Sequence(Rc::new(IteratorSeq {
        inner: iter.map(|c| Ok(Value::String(c.to_string()))),
      })))
    }
    _ => unimplemented!(),
  }
}

fn assert_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let condition = arg_refs[0].resolve(args, kwargs).as_bool().unwrap();
      let msg = arg_refs[1].resolve(args, kwargs).as_str().unwrap();

      if !condition {
        return Err(ErrorStack::new(msg.to_owned()));
      }

      Ok(Value::Nil)
    }
    _ => unimplemented!(),
  }
}

fn intersects_ray_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let ray_origin = *arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      let ray_direction = *arg_refs[1].resolve(args, kwargs).as_vec3().unwrap();
      let mesh = arg_refs[2].resolve(args, kwargs).as_mesh().unwrap();
      let max_distance = arg_refs[3].resolve(args, kwargs).as_float();

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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let a = arg_refs[0].resolve(args, kwargs).as_mesh().unwrap();
      let b = arg_refs[1].resolve(args, kwargs).as_mesh().unwrap();

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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let mesh_handle = arg_refs[0].resolve(args, kwargs).as_mesh().unwrap();
      let transform = mesh_handle.transform;
      let mesh = Rc::clone(&mesh_handle.mesh);
      let mut components: Vec<Vec<FaceKey>> = mesh.connected_components();
      components.sort_unstable_by_key(|c| Reverse(c.len()));
      let material = mesh_handle.material.clone();
      Ok(Value::Sequence(Rc::new(IteratorSeq {
        inner: components.into_iter().map(move |c| {
          let mut sub_vkey_by_old_vkey: FxHashMap<VertexKey, VertexKey> = FxHashMap::default();
          let mut sub_mesh = LinkedMesh::new(0, c.len(), None);

          let mut map_vtx = |sub_mesh: &mut LinkedMesh<()>, vkey: VertexKey| {
            *sub_vkey_by_old_vkey.entry(vkey).or_insert_with(|| {
              sub_mesh.vertices.insert(Vertex {
                position: mesh.vertices[vkey].position,
                shading_normal: None,
                displacement_normal: None,
                edges: SmallVec::new(),
                _padding: Default::default(),
              })
            })
          };

          for face_key in c {
            let face = &mesh.faces[face_key];
            let vtx0 = map_vtx(&mut sub_mesh, face.vertices[0]);
            let vtx1 = map_vtx(&mut sub_mesh, face.vertices[1]);
            let vtx2 = map_vtx(&mut sub_mesh, face.vertices[2]);
            sub_mesh.add_face::<true>([vtx0, vtx1, vtx2], ());
          }
          Ok(Value::Mesh(Rc::new(MeshHandle {
            mesh: Rc::new(sub_mesh),
            transform,
            manifold_handle: Rc::new(ManifoldHandle::new_empty()),
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let target_edge_length = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      if target_edge_length <= 0. {
        return Err(ErrorStack::new("`target_edge_length` must be > 0"));
      }
      let mesh_handle = arg_refs[1].resolve(args, kwargs).as_mesh().unwrap();
      let transform = mesh_handle.transform;

      let mut mesh = (*mesh_handle.mesh).clone();
      tessellation::tessellate_mesh(
        &mut mesh,
        target_edge_length,
        DisplacementNormalMethod::Interpolate,
      );
      Ok(Value::Mesh(Rc::new(MeshHandle {
        mesh: Rc::new(mesh),
        transform: transform,
        manifold_handle: Rc::new(ManifoldHandle::new_empty()),
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  fn unhandled_transform_error() -> ErrorStack {
    ErrorStack::new(
      "subdivide_by_plane does not currently support meshes with transforms.  Either call this \
       function before transforming or use `apply_transforms` to bake the transforms into the \
       mesh vertex positions.",
    )
  }

  match def_ix {
    0 => {
      let plane_normal = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      let plane_offset = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      let mesh_handle = arg_refs[2].resolve(args, kwargs).as_mesh().unwrap();

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
        transform: mesh_handle.transform,
        manifold_handle: Rc::new(ManifoldHandle::new_empty()),
        aabb: RefCell::new(None),
        trimesh: RefCell::new(None),
        material: mesh_handle.material.clone(),
      })))
    }
    1 => {
      let mesh_handle = arg_refs[2].resolve(args, kwargs).as_mesh().unwrap();
      let plane_normals = arg_refs[0]
        .resolve(args, kwargs)
        .as_sequence()
        .unwrap()
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
        .resolve(args, kwargs)
        .as_sequence()
        .unwrap()
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
        transform: mesh_handle.transform,
        manifold_handle: Rc::new(ManifoldHandle::new_empty()),
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let plane_normal = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      let plane_offset = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      let mesh_handle = arg_refs[2].resolve(args, kwargs).as_mesh().unwrap();

      let (a, b) = split_mesh_by_plane(mesh_handle, *plane_normal, plane_offset)
        .map_err(|err| ErrorStack::new(format!("Error in `split_by_plane`: {err}")))?;

      Ok(Value::Sequence(Rc::new(EagerSeq {
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
  kwargs: &FxHashMap<Sym, Value>,
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
          seq.consume(ctx).collect::<Result<Vec<_>, _>>()?
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let count = arg_refs[0].resolve(args, kwargs);
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
      let mesh = arg_refs[1].resolve(args, kwargs).as_mesh().unwrap();
      let seed = arg_refs[2]
        .resolve(args, kwargs)
        .as_int()
        .unwrap()
        .unsigned_abs();
      let cb = arg_refs[3].resolve(args, kwargs).as_callable().cloned();
      let world_space = arg_refs[4].resolve(args, kwargs).as_bool().unwrap();

      let sampler_seq = PointDistributeSeq {
        mesh: mesh.clone(false, false, true),
        point_count,
        seed,
        cb,
        world_space,
      };
      Ok(Value::Sequence(Rc::new(sampler_seq)))
    }
    _ => unimplemented!(),
  }
}

fn render_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let Value::Mesh(mesh) = arg_refs[0].resolve(args, kwargs) else {
        unreachable!()
      };
      ctx.rendered_meshes.push(Rc::clone(mesh));
      Ok(Value::Nil)
    }
    1 => {
      let light = arg_refs[0].resolve(args, kwargs).as_light().unwrap();
      ctx.rendered_lights.push(light.clone());
      Ok(Value::Nil)
    }
    2 => {
      // This is expected to be a `seq<Vec3> | seq<Mesh | seq<Vec3>>`
      let sequence = arg_refs[0].resolve(args, kwargs).as_sequence().unwrap();

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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let formatted_pos_ags = args
    .iter()
    .map(|v| format!("{v:?}"))
    .collect::<Vec<_>>()
    .join(", ");
  let formatted_kwargs = kwargs
    .iter()
    .map(|(k, v)| ctx.with_resolved_sym(*k, |k| format!("{k}={v:?}")))
    .collect::<Vec<_>>()
    .join(", ");

  (ctx.log_fn)(&format!("{formatted_pos_ags}, {formatted_kwargs}"));
  Ok(Value::Nil)
}

fn lerp_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let t = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let a = arg_refs[1].resolve(args, kwargs).as_vec3().unwrap();
      let b = arg_refs[2].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(a.lerp(b, t)))
    }
    1 => {
      let t = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let a = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      let b = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(a + (b - a) * t))
    }
    _ => unimplemented!(),
  }
}

fn smoothstep_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let edge0 = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let edge1 = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      let x = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let edge0 = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let edge1 = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      let x = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(value.to_radians()))
    }
    _ => unimplemented!(),
  }
}

fn rad2deg_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(value.to_degrees()))
    }
    _ => unimplemented!(),
  }
}

fn fix_float_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      if value.is_normal() {
        Ok(Value::Float(value))
      } else {
        Ok(Value::Float(0.))
      }
    }
    1 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        if value.x.is_normal() { value.x } else { 0. },
        if value.y.is_normal() { value.y } else { 0. },
        if value.z.is_normal() { value.z } else { 0. },
      )))
    }
    2 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(
        if value.x.is_normal() { value.x } else { 0. },
        if value.y.is_normal() { value.y } else { 0. },
      )))
    }
    _ => unimplemented!(),
  }
}

fn round_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(value.round()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.round(),
        value.y.round(),
        value.z.round(),
      )))
    }
    2 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(value.x.round(), value.y.round())))
    }
    _ => unimplemented!(),
  }
}

fn floor_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(value.floor()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.floor(),
        value.y.floor(),
        value.z.floor(),
      )))
    }
    2 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(value.x.floor(), value.y.floor())))
    }
    _ => unimplemented!(),
  }
}

fn ceil_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(value.ceil()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.ceil(),
        value.y.ceil(),
        value.z.ceil(),
      )))
    }
    2 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(value.x.ceil(), value.y.ceil())))
    }
    _ => unimplemented!(),
  }
}

fn fract_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(value.fract()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.fract(),
        value.y.fract(),
        value.z.fract(),
      )))
    }
    2 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(value.x.fract(), value.y.fract())))
    }
    _ => unimplemented!(),
  }
}

fn trunc_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(value.trunc()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.trunc(),
        value.y.trunc(),
        value.z.trunc(),
      )))
    }
    2 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(value.x.trunc(), value.y.trunc())))
    }
    _ => unimplemented!(),
  }
}

fn pow_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let base = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let exponent = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(base.powf(exponent)))
    }
    1 => {
      let base = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      let exponent = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Vec3(Vec3::new(
        base.x.powf(exponent),
        base.y.powf(exponent),
        base.z.powf(exponent),
      )))
    }
    2 => {
      let base = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      let exponent = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Vec2(Vec2::new(
        base.x.powf(exponent),
        base.y.powf(exponent),
      )))
    }
    _ => unimplemented!(),
  }
}

fn exp_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(value.exp()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.exp(),
        value.y.exp(),
        value.z.exp(),
      )))
    }
    2 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(value.x.exp(), value.y.exp())))
    }
    _ => unimplemented!(),
  }
}

fn log10_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(value.log10()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.log10(),
        value.y.log10(),
        value.z.log10(),
      )))
    }
    2 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(value.x.log10(), value.y.log10())))
    }
    _ => unimplemented!(),
  }
}

fn log2_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(value.log2()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.log2(),
        value.y.log2(),
        value.z.log2(),
      )))
    }
    2 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(value.x.log2(), value.y.log2())))
    }
    _ => unimplemented!(),
  }
}

fn ln_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(value.ln()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.ln(),
        value.y.ln(),
        value.z.ln(),
      )))
    }
    2 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(value.x.ln(), value.y.ln())))
    }
    _ => unimplemented!(),
  }
}

fn tan_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(value.tan()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.tan(),
        value.y.tan(),
        value.z.tan(),
      )))
    }
    2 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(value.x.tan(), value.y.tan())))
    }
    _ => unimplemented!(),
  }
}

fn cos_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(value.cos()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.cos(),
        value.y.cos(),
        value.z.cos(),
      )))
    }
    2 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(value.x.cos(), value.y.cos())))
    }
    _ => unimplemented!(),
  }
}

fn sin_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(value.sin()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(value.sinh()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.sinh(),
        value.y.sinh(),
        value.z.sinh(),
      )))
    }
    2 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(value.x.sinh(), value.y.sinh())))
    }
    _ => unimplemented!(),
  }
}

fn cosh_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(value.cosh()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.cosh(),
        value.y.cosh(),
        value.z.cosh(),
      )))
    }
    2 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(value.x.cosh(), value.y.cosh())))
    }
    _ => unimplemented!(),
  }
}

fn tanh_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(value.tanh()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.tanh(),
        value.y.tanh(),
        value.z.tanh(),
      )))
    }
    2 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(value.x.tanh(), value.y.tanh())))
    }
    _ => unimplemented!(),
  }
}

fn acos_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(value.acos()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.acos(),
        value.y.acos(),
        value.z.acos(),
      )))
    }
    2 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(value.x.acos(), value.y.acos())))
    }
    _ => unimplemented!(),
  }
}

fn asin_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(value.asin()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.asin(),
        value.y.asin(),
        value.z.asin(),
      )))
    }
    2 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(value.x.asin(), value.y.asin())))
    }
    _ => unimplemented!(),
  }
}

fn atan_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(value.atan()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.atan(),
        value.y.atan(),
        value.z.atan(),
      )))
    }
    2 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(value.x.atan(), value.y.atan())))
    }
    _ => unimplemented!(),
  }
}

fn atan2_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let y = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let x = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(y.atan2(x)))
    }
    1 => {
      let v = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Float(v.y.atan2(v.x)))
    }
    _ => unimplemented!(),
  }
}

fn box_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  {
    let (width, height, depth) = match def_ix {
      0 => {
        let w = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
        let h = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
        let d = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
        (w, h, d)
      }
      1 => {
        let val = arg_refs[0].resolve(args, kwargs);
        match val {
          Value::Vec3(v3) => (v3.x, v3.y, v3.z),
          Value::Float(size) => (*size, *size, *size),
          Value::Int(size) => {
            let size = *size as f32;
            (size, size, size)
          }
          _ => {
            return Err(ErrorStack::new(format!(
              "Invalid argument for box size: expected Vec3, Int, or Float; found: {val:?}",
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let radius = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let resolution = arg_refs[1].resolve(args, kwargs).as_int().unwrap();
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let radius = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let height = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      let radial_segments = arg_refs[2].resolve(args, kwargs).as_int().unwrap();
      let height_segments = arg_refs[3].resolve(args, kwargs).as_int().unwrap();

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

fn cone_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let radius = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let height = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      let radial_segments = arg_refs[2].resolve(args, kwargs).as_int().unwrap();
      let height_segments = arg_refs[3].resolve(args, kwargs).as_int().unwrap();

      if radial_segments < 3 {
        return Err(ErrorStack::new("`radial_segments` must be >= 3"));
      } else if height_segments < 1 {
        return Err(ErrorStack::new("`height_segments` must be >= 1"));
      }

      Ok(Value::Mesh(Rc::new(MeshHandle::new(Rc::new(
        LinkedMesh::new_cone(
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let size = match arg_refs[0].resolve(args, kwargs) {
        _ if let Some(v) = arg_refs[0].resolve(args, kwargs).as_vec2() => *v,
        _ if let Some(f) = arg_refs[0].resolve(args, kwargs).as_float() => Vec2::new(f, f),
        other => {
          return Err(ErrorStack::new(format!(
            "Invalid type for grid size: expected Vec2 or Float, found {other:?}",
          )))
        }
      };
      let divisions = match arg_refs[1].resolve(args, kwargs) {
        _ if let Some(v) = arg_refs[1].resolve(args, kwargs).as_vec2() => {
          if v.x < 1. || v.y < 1. {
            return Err(ErrorStack::new("Grid divisions must be >= 1"));
          }
          (v.x as usize, v.y as usize)
        }
        _ if let Some(i) = arg_refs[1].resolve(args, kwargs).as_int() => {
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
      let flipped = arg_refs[2].resolve(args, kwargs).as_bool().unwrap();

      let mesh = LinkedMesh::new_grid(size.x, size.y, divisions.0, divisions.1, flipped);
      Ok(Value::Mesh(Rc::new(MeshHandle::new(Rc::new(mesh)))))
    }
    _ => unimplemented!(),
  }
}

fn utah_teapot_impl(def_ix: usize) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => Ok(Value::Mesh(Rc::new(MeshHandle::new(Rc::new(
      LinkedMesh::new_utah_teapot(),
    ))))),
    _ => unimplemented!(),
  }
}

fn stanford_bunny_impl(def_ix: usize) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => Ok(Value::Mesh(Rc::new(MeshHandle::new(Rc::new(
      LinkedMesh::new_stanford_bunny(),
    ))))),
    _ => unimplemented!(),
  }
}

// fn suzanne_impl(def_ix: usize) -> Result<Value, ErrorStack> {
//   match def_ix {
//     0 => Ok(Value::Mesh(Rc::new(MeshHandle::new(Rc::new(
//       LinkedMesh::new_suzanne(),
//     ))))),
//     _ => unimplemented!(),
//   }
// }

fn rot_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  enum ObjType {
    Mesh,
    Light,
  }

  let (rotation, obj_arg, obj_type) = match def_ix {
    0 | 2 => {
      let rotation = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      (
        UnitQuaternion::from_euler_angles(rotation.x, rotation.y, rotation.z),
        &arg_refs[1],
        if def_ix == 0 {
          ObjType::Mesh
        } else {
          ObjType::Light
        },
      )
    }
    1 | 3 => {
      let x = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let y = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      let z = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
      (
        UnitQuaternion::from_euler_angles(x, y, z),
        &arg_refs[3],
        if def_ix == 1 {
          ObjType::Mesh
        } else {
          ObjType::Light
        },
      )
    }
    _ => unimplemented!(),
  };
  let obj_arg = obj_arg.resolve(args, kwargs);

  let apply_rotation = |transform: &Matrix4<f32>| {
    let back: Matrix4<f32> = Matrix4::new_translation(&transform.column(3).xyz());
    let to_origin: Matrix4<f32> = Matrix4::new_translation(&-transform.column(3).xyz());
    back * rotation.to_homogeneous() * to_origin * *transform
  };

  match obj_type {
    ObjType::Mesh => {
      let mesh = obj_arg.as_mesh().unwrap();

      let mut rotated_mesh = mesh.clone(true, false, false);
      rotated_mesh.transform = apply_rotation(&rotated_mesh.transform);

      Ok(Value::Mesh(Rc::new(rotated_mesh)))
    }
    ObjType::Light => {
      let light = obj_arg.as_light().unwrap();

      let mut rotated_light = (*light).clone();
      let transform = rotated_light.transform_mut();
      *transform = apply_rotation(transform);

      Ok(Value::Light(Box::new(rotated_light)))
    }
  }
}

fn look_at_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    // TODO: I'm pretty sure this isn't working like I was expecting it to
    0 => {
      let pos = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      let target = arg_refs[1].resolve(args, kwargs).as_vec3().unwrap();

      let dir = target - *pos;
      let up = Vec3::new(0., 1., 0.);
      let rot = UnitQuaternion::look_at_rh(&dir, &up);
      let (x, y, z) = rot.euler_angles();
      Ok(Value::Vec3(Vec3::new(x, y, z)))
    }
    1 => {
      let mesh = arg_refs[0].resolve(args, kwargs).as_mesh().unwrap();
      let target = arg_refs[1].resolve(args, kwargs).as_vec3().unwrap();
      let up = arg_refs[2].resolve(args, kwargs).as_vec3().unwrap();

      let mut mesh = mesh.clone(true, false, false);

      // extract translation
      let translation = mesh.transform.column(3).xyz();

      // extract current scale
      let basis3 = mesh.transform.fixed_view::<3, 3>(0, 0).clone_owned();
      let scale_x = basis3.column(0).norm();
      let scale_y = basis3.column(1).norm();
      let scale_z = basis3.column(2).norm();

      let dir = (target - translation).normalize();

      let rotation = Rotation3::rotation_between(up, &dir).ok_or_else(|| {
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let mesh = arg_refs[0].resolve(args, kwargs).as_mesh().unwrap();
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let mesh = arg_refs[0].resolve(args, kwargs).as_mesh().unwrap();
      let mut new_mesh = (*mesh.mesh).clone();
      for vtx in new_mesh.vertices.values_mut() {
        vtx.position = (mesh.transform * vtx.position.push(1.)).xyz();
      }
      Ok(Value::Mesh(Rc::new(MeshHandle {
        mesh: Rc::new(new_mesh),
        transform: Matrix4::identity(),
        manifold_handle: Rc::new(ManifoldHandle::new_empty()),
        aabb: RefCell::new(None),
        trimesh: RefCell::new(None),
        material: mesh.material.clone(),
      })))
    }
    _ => unimplemented!(),
  }
}

fn flip_normals_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let mesh = arg_refs[0].resolve(args, kwargs).as_mesh().unwrap();
      let mut new_mesh = (*mesh.mesh).clone();
      new_mesh.flip_normals();
      Ok(Value::Mesh(Rc::new(MeshHandle {
        aabb: mesh.aabb.clone(),
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

fn vec2_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let x = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let y = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Vec2(Vec2::new(x, y)))
    }
    1 => {
      let x = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Vec2(Vec2::new(x, x)))
    }
    _ => unimplemented!(),
  }
}

fn vec3_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let x = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let y = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      let z = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Vec3(Vec3::new(x, y, z)))
    }
    1 => {
      let xy = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      let z = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Vec3(Vec3::new(xy.x, xy.y, z)))
    }
    2 => {
      let x = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let yz = arg_refs[1].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec3(Vec3::new(x, yz.x, yz.y)))
    }
    3 => {
      let x = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Vec3(Vec3::new(x, x, x)))
    }
    _ => unimplemented!(),
  }
}

fn join_meshes(
  iter: &mut impl Iterator<Item = Result<Value, ErrorStack>>,
) -> Result<Value, ErrorStack> {
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

          combined.vertices.insert(Vertex {
            position: transformed_pos.xyz(),
            displacement_normal: old_vtx.displacement_normal,
            shading_normal: old_vtx.shading_normal,
            edges: SmallVec::new(),
            _padding: Default::default(),
          })
        })
      });
      combined.add_face::<false>(new_vtx_keys, ());
    }
  }

  Ok(Value::Mesh(Rc::new(MeshHandle {
    mesh: Rc::new(combined),
    transform: out_transform,
    manifold_handle: Rc::new(ManifoldHandle::new_empty()),
    aabb: RefCell::new(None),
    trimesh: RefCell::new(None),
    material: base.material.clone(),
  })))
}

fn join_strings(
  iter: &mut impl Iterator<Item = Result<Value, ErrorStack>>,
  separator: &str,
) -> Result<Value, ErrorStack> {
  let mut out = String::new();
  let mut is_first = true;
  for res in iter.by_ref() {
    match res {
      Ok(Value::String(s)) => {
        if !is_first {
          out.push_str(separator);
        }
        out.push_str(&s);
        is_first = false;
      }
      Ok(other) => {
        return Err(ErrorStack::new(format!(
          "Non-string value produced in sequence passed to join: {other:?}"
        )))
      }
      Err(e) => return Err(e.wrap("Error in sequence passed to join")),
    }
  }
  Ok(Value::String(out))
}

fn join_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let mut iter = arg_refs[0]
        .resolve(args, kwargs)
        .as_sequence()
        .unwrap()
        .consume(ctx)
        .peekable();

      join_meshes(&mut iter)
    }
    1 => {
      let separator = arg_refs[0].resolve(args, kwargs).as_str().unwrap();
      let sequence = arg_refs[1].resolve(args, kwargs).as_sequence().unwrap();
      let mut iter = sequence.consume(ctx).peekable();

      join_strings(&mut iter, separator)
    }
    _ => unimplemented!(),
  }
}

fn filter_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let fn_value = arg_refs[0].resolve(args, kwargs).as_callable().unwrap();
      let sequence = arg_refs[1].resolve(args, kwargs).as_sequence().unwrap();

      Ok(Value::Sequence(Rc::new(FilterSeq {
        cb: Rc::clone(fn_value),
        inner: sequence,
      })))
    }
    _ => unimplemented!(),
  }
}

fn scan_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let init = arg_refs[0].resolve(args, kwargs);
      let fn_value = arg_refs[1].resolve(args, kwargs).as_callable().unwrap();
      let sequence = arg_refs[2].resolve(args, kwargs).as_sequence().unwrap();
      Ok(Value::Sequence(Rc::new(ScanSeq {
        acc: init.clone(),
        cb: fn_value.clone(),
        inner: sequence,
      })))
    }
    _ => unimplemented!(),
  }
}

fn take_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let count = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
      let count = if count < 0 { 0 } else { count as usize };
      let sequence = arg_refs[1].resolve(args, kwargs).as_sequence().unwrap();
      Ok(Value::Sequence(Rc::new(TakeSeq {
        count,
        inner: sequence,
      })))
    }
    _ => unimplemented!(),
  }
}

fn skip_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let count = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
      let count = if count < 0 { 0 } else { count as usize };
      let sequence = arg_refs[1].resolve(args, kwargs).as_sequence().unwrap();
      Ok(Value::Sequence(Rc::new(SkipSeq {
        count,
        inner: sequence,
      })))
    }
    _ => unimplemented!(),
  }
}

fn take_while_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let fn_value = arg_refs[0].resolve(args, kwargs).as_callable().unwrap();
      let sequence = arg_refs[1].resolve(args, kwargs).as_sequence().unwrap();
      Ok(Value::Sequence(Rc::new(TakeWhileSeq {
        cb: fn_value.clone(),
        inner: sequence,
      })))
    }
    _ => unimplemented!(),
  }
}

fn skip_while_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let fn_value = arg_refs[0].resolve(args, kwargs).as_callable().unwrap();
      let sequence = arg_refs[1].resolve(args, kwargs).as_sequence().unwrap();
      Ok(Value::Sequence(Rc::new(SkipWhileSeq {
        cb: fn_value.clone(),
        inner: sequence,
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let seqs = arg_refs[0].resolve(args, kwargs).as_sequence().unwrap();
      Ok(Value::Sequence(Rc::new(ChainSeq::new(ctx, seqs)?)))
    }
    _ => unimplemented!(),
  }
}

fn first_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let sequence = arg_refs[0].resolve(args, kwargs).as_sequence().unwrap();
      let mut iter = sequence.consume(ctx);
      match iter.next() {
        Some(res) => res,
        None => Ok(Value::Nil),
      }
    }
    _ => unimplemented!(),
  }
}

fn last_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let seq = arg_refs[0].resolve(args, kwargs).as_sequence().unwrap();
      if let Some(eager) = seq_as_eager(&*seq) {
        return Ok(eager.inner.last().cloned().unwrap_or(Value::Nil));
      }

      let iter = seq.consume(ctx);
      match iter.last() {
        Some(res) => res,
        None => Ok(Value::Nil),
      }
    }
    _ => unimplemented!(),
  }
}

fn append_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let val = arg_refs[0].resolve(args, kwargs);
      let seq = arg_refs[1].resolve(args, kwargs).as_sequence().unwrap();

      let mut eager_seq = match seq_as_eager(&*seq) {
        Some(eager) => eager.clone(),
        None => {
          let iter = seq.consume(ctx);
          let collected = iter
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| err.wrap("error produced during `collect`"))?;
          EagerSeq { inner: collected }
        }
      };
      eager_seq.inner.push(val.clone());
      Ok(Value::Sequence(Rc::new(eager_seq)))
    }
    _ => unimplemented!(),
  }
}

fn reverse_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let sequence = arg_refs[0].resolve(args, kwargs).as_sequence().unwrap();
      let mut vals: Vec<Value> = sequence.consume(ctx).collect::<Result<Vec<_>, _>>()?;
      vals.reverse();
      Ok(Value::Sequence(Rc::new(EagerSeq { inner: vals })))
    }
    _ => unimplemented!(),
  }
}

fn collect_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let val = arg_refs[0].resolve(args, kwargs);
      let seq = val.as_sequence().unwrap();
      match seq_as_eager(&*seq) {
        Some(_) => Ok(val.clone()),
        None => {
          let iter = seq.consume(ctx);
          let collected = iter
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| err.wrap("error produced during `collect`"))?;
          Ok(Value::Sequence(Rc::new(EagerSeq { inner: collected })))
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let cb = arg_refs[0].resolve(args, kwargs).as_callable().unwrap();
      let sequence = arg_refs[1].resolve(args, kwargs).as_sequence().unwrap();
      let iter = sequence.consume(ctx);
      for (i, res) in iter.enumerate() {
        let val = res?;
        let val = ctx
          .invoke_callable(cb, &[val], EMPTY_KWARGS)
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let cb = arg_refs[0].resolve(args, kwargs).as_callable().unwrap();
      let sequence = arg_refs[1].resolve(args, kwargs).as_sequence().unwrap();
      let iter = sequence.consume(ctx);
      for (i, res) in iter.enumerate() {
        let val = res?;
        let val = ctx
          .invoke_callable(cb, &[val], EMPTY_KWARGS)
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let cb = arg_refs[0].resolve(args, kwargs).as_callable().unwrap();
      let sequence = arg_refs[1].resolve(args, kwargs).as_sequence().unwrap();
      let iter = sequence.consume(ctx);
      for res in iter {
        let val = res?;
        ctx
          .invoke_callable(cb, &[val], EMPTY_KWARGS)
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let seq = arg_refs[0].resolve(args, kwargs).as_sequence().unwrap();
      Ok(Value::Sequence(Rc::new(FlattenSeq { inner: seq })))
    }
    _ => unimplemented!(),
  }
}

fn abs_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
      Ok(Value::Int(value.abs()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(value.abs()))
    }
    2 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.abs(),
        value.y.abs(),
        value.z.abs(),
      )))
    }
    3 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(value.x.abs(), value.y.abs())))
    }
    _ => unimplemented!(),
  }
}

fn signum_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(value.signum()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
      Ok(Value::Int(value.signum()))
    }
    2 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(value.x.signum(), value.y.signum())))
    }
    3 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.signum(),
        value.y.signum(),
        value.z.signum(),
      )))
    }
    _ => unimplemented!(),
  }
}

fn sqrt_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let value = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(value.sqrt()))
    }
    1 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.sqrt(),
        value.y.sqrt(),
        value.z.sqrt(),
      )))
    }
    2 => {
      let value = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(value.x.sqrt(), value.y.sqrt())))
    }
    _ => unimplemented!(),
  }
}

fn max_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      // int, int
      let a = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
      let b = arg_refs[1].resolve(args, kwargs).as_int().unwrap();
      Ok(Value::Int(a.max(b)))
    }
    1 => {
      // float, float
      let a = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let b = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(a.max(b)))
    }
    2 => {
      // vec3, vec3
      let a = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      let b = arg_refs[1].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        a.x.max(b.x),
        a.y.max(b.y),
        a.z.max(b.z),
      )))
    }
    3 => {
      // vec2, vec2
      let a = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      let b = arg_refs[1].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(a.x.max(b.x), a.y.max(b.y))))
    }
    _ => unimplemented!(),
  }
}

fn min_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      // int, int
      let a = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
      let b = arg_refs[1].resolve(args, kwargs).as_int().unwrap();
      Ok(Value::Int(a.min(b)))
    }
    1 => {
      // float, float
      let a = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let b = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(a.min(b)))
    }
    2 => {
      // vec3, vec3
      let a = arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
      let b = arg_refs[1].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        a.x.min(b.x),
        a.y.min(b.y),
        a.z.min(b.z),
      )))
    }
    3 => {
      // vec2, vec2
      let a = arg_refs[0].resolve(args, kwargs).as_vec2().unwrap();
      let b = arg_refs[1].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(a.x.min(b.x), a.y.min(b.y))))
    }
    _ => unimplemented!(),
  }
}

fn clamp_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      // int, int, int
      let min = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
      let max = arg_refs[1].resolve(args, kwargs).as_int().unwrap();
      let value = arg_refs[2].resolve(args, kwargs).as_int().unwrap();
      Ok(Value::Int(value.clamp(min, max)))
    }
    1 => {
      // float, float, float
      let min = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let max = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      let value = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
      Ok(Value::Float(value.clamp(min, max)))
    }
    2 => {
      // float, float, vec3
      let min = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let max = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      let value = arg_refs[2].resolve(args, kwargs).as_vec3().unwrap();
      Ok(Value::Vec3(Vec3::new(
        value.x.clamp(min, max),
        value.y.clamp(min, max),
        value.z.clamp(min, max),
      )))
    }
    3 => {
      // float, float, vec2
      let min = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      let max = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
      let value = arg_refs[2].resolve(args, kwargs).as_vec2().unwrap();
      Ok(Value::Vec2(Vec2::new(
        value.x.clamp(min, max),
        value.y.clamp(min, max),
      )))
    }
    _ => unimplemented!(),
  }
}

fn float_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let val = arg_refs[0].resolve(args, kwargs);
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
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let val = arg_refs[0].resolve(args, kwargs);
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

fn str_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let val = arg_refs[0].resolve(args, kwargs);
      let s = match val {
        Value::Int(i) => format!("{i}"),
        Value::Float(f) => format!("{f}"),
        Value::Vec2(v) => format!("vec2({}, {})", v.x, v.y),
        Value::Vec3(v) => format!("vec3({}, {}, {})", v.x, v.y, v.z),
        Value::Mesh(mesh_handle) => format!("{:?}", mesh_handle.mesh),
        Value::Light(light) => format!("{light:?}"),
        Value::Callable(callable) => format!("{callable:?}"),
        Value::Sequence(sequence) => format!("{sequence:?}"),
        Value::Map(hash_map) => format!("{hash_map:?}"),
        Value::Bool(b) => format!("{b}"),
        Value::String(s) => s.clone(),
        Value::Material(material) => format!("{material:?}"),
        Value::Nil => String::from("nil"),
      };
      Ok(Value::String(s))
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
        kwargs: &FxHashMap<Sym, Value>,
        ctx: &EvalCtx,
      ) -> Result<Value, ErrorStack> {
        $impl(def_ix, arg_refs, args, kwargs, ctx)
      }

      [<builtin_ $name _impl>]
    }}
  };
}

fn builtin_move_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
  ctx: &EvalCtx,
) -> Result<Value, ErrorStack> {
  trace_path::draw_command_stub_impl("move", def_ix, arg_refs, args, kwargs, ctx)
}

pub(crate) static BUILTIN_FN_IMPLS: phf::Map<
  &'static str,
  fn(
    def_ix: usize,
    arg_refs: &[ArgRef],
    args: &[Value],
    kwargs: &FxHashMap<Sym, Value>,
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
  "cone" => builtin_fn!(cone, |def_ix, arg_refs, args, kwargs, _ctx| {
    cone_impl(def_ix, arg_refs, args, kwargs)
  }),
  "grid" => builtin_fn!(grid, |def_ix, arg_refs, args, kwargs, _ctx| {
    grid_impl(def_ix, arg_refs, args, kwargs)
  }),
  "utah_teapot" => builtin_fn!(utah_teapot, |def_ix, _arg_refs, _args, _kwargs, _ctx| {
    utah_teapot_impl(def_ix)
  }),
  "stanford_bunny" => builtin_fn!(stanford_bunny, |def_ix, _arg_refs, _args, _kwargs, _ctx| {
    stanford_bunny_impl(def_ix)
  }),
  // "suzanne" => builtin_fn!(suzanne, |def_ix, _arg_refs, _args, _kwargs, _ctx| {
  //   suzanne_impl(def_ix)
  // }),
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
  "flip_normals" => builtin_fn!(flip_normals, |def_ix, arg_refs, args, kwargs, _ctx| {
    flip_normals_impl(def_ix, arg_refs, args, kwargs)
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
    ctx.fold(initial_val, fn_value, sequence)
  }),
  "fold_while" => builtin_fn!(fold, |def_ix, arg_refs, args, kwargs, ctx| {
    fold_while_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "reduce" => builtin_fn!(reduce, |_def_ix, arg_refs: &[ArgRef], args, kwargs, ctx: &EvalCtx| {
    let fn_value = arg_refs[0].resolve(args, kwargs).as_callable().unwrap();
    let seq = arg_refs[1].resolve(args, kwargs).as_sequence().unwrap();
    ctx.reduce(fn_value, seq)
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
  "last" => builtin_fn!(first, |def_ix, arg_refs, args, kwargs, ctx| {
    last_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "append" => builtin_fn!(first, |def_ix, arg_refs, args, kwargs, ctx| {
    append_impl(ctx, def_ix, arg_refs, args, kwargs)
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
  "signum" => builtin_fn!(signum, |def_ix, arg_refs: &[ArgRef], args, kwargs, _ctx| {
    signum_impl(def_ix, arg_refs, args, kwargs)
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
  "str" => builtin_fn!(str, |def_ix, arg_refs, args, kwargs, _ctx| {
    str_impl(def_ix, arg_refs, args, kwargs)
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
  "and" => builtin_fn!(and, |def_ix, arg_refs: &[ArgRef], args, kwargs, _ctx| {
    let lhs = arg_refs[0].resolve(args, kwargs);
    let rhs = arg_refs[1].resolve(args, kwargs);
    and_impl(def_ix, lhs, rhs)
  }),
  "or" => builtin_fn!(or, |def_ix, arg_refs: &[ArgRef], args, kwargs, _ctx| {
    let lhs = arg_refs[0].resolve(args, kwargs);
    let rhs = arg_refs[1].resolve(args, kwargs);
    or_impl(def_ix, lhs, rhs)
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
  "acos" => builtin_fn!(acos, |def_ix, arg_refs, args, kwargs, _ctx| {
    acos_impl(def_ix, arg_refs, args, kwargs)
  }),
  "asin" => builtin_fn!(asin, |def_ix, arg_refs, args, kwargs, _ctx| {
    asin_impl(def_ix, arg_refs, args, kwargs)
  }),
  "atan" => builtin_fn!(atan, |def_ix, arg_refs, args, kwargs, _ctx| {
    atan_impl(def_ix, arg_refs, args, kwargs)
  }),
  "atan2" => builtin_fn!(atan2, |def_ix, arg_refs, args, kwargs, _ctx| {
    atan2_impl(def_ix, arg_refs, args, kwargs)
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
  "len" => builtin_fn!(len, |def_ix, arg_refs, args, kwargs, ctx| {
    len_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "chars" => builtin_fn!(chars, |def_ix, arg_refs, args, kwargs, _ctx| {
    chars_impl(def_ix, arg_refs, args, kwargs)
  }),
  "assert" => builtin_fn!(assert, |def_ix, arg_refs, args, kwargs, _ctx| {
    assert_impl(def_ix, arg_refs, args, kwargs)
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
  "lissajous_knot_path" => builtin_fn!(lissajous_knot_path, |def_ix, arg_refs, args, kwargs, _ctx| {
    lissajous_knot_path_impl(def_ix, arg_refs, args, kwargs)
  }),
  "move" => builtin_move_impl,
  "line" => builtin_fn!(line, |def_ix, arg_refs, args, kwargs, ctx| {
    trace_path::draw_command_stub_impl("line", def_ix, arg_refs, args, kwargs, ctx)
  }),
  "quadratic_bezier" => builtin_fn!(quadratic_bezier, |def_ix, arg_refs, args, kwargs, ctx| {
    trace_path::draw_command_stub_impl("quadratic_bezier", def_ix, arg_refs, args, kwargs, ctx)
  }),
  "smooth_quadratic_bezier" => builtin_fn!(smooth_quadratic_bezier, |def_ix, arg_refs, args, kwargs, ctx| {
    trace_path::draw_command_stub_impl("smooth_quadratic_bezier", def_ix, arg_refs, args, kwargs, ctx)
  }),
  "cubic_bezier" => builtin_fn!(cubic_bezier, |def_ix, arg_refs, args, kwargs, ctx| {
    trace_path::draw_command_stub_impl("cubic_bezier", def_ix, arg_refs, args, kwargs, ctx)
  }),
  "smooth_cubic_bezier" => builtin_fn!(smooth_cubic_bezier, |def_ix, arg_refs, args, kwargs, ctx| {
    trace_path::draw_command_stub_impl("smooth_cubic_bezier", def_ix, arg_refs, args, kwargs, ctx)
  }),
  "arc" => builtin_fn!(arc, |def_ix, arg_refs, args, kwargs, ctx| {
    trace_path::draw_command_stub_impl("arc", def_ix, arg_refs, args, kwargs, ctx)
  }),
  "close" => builtin_fn!(close, |def_ix, arg_refs, args, kwargs, ctx| {
    trace_path::draw_command_stub_impl("close", def_ix, arg_refs, args, kwargs, ctx)
  }),
  "trace_path" => builtin_fn!(trace_path, |def_ix, arg_refs, args, kwargs, ctx| {
    trace_path::trace_path_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "trace_svg_path" => builtin_fn!(trace_svg_path, |def_ix, arg_refs, args, kwargs, ctx| {
    trace_path::trace_svg_path_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "extrude" => builtin_fn!(extrude, |def_ix, arg_refs, args, kwargs, ctx| {
    extrude_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "stitch_contours" => builtin_fn!(stitch_contours, |def_ix, arg_refs, args, kwargs, ctx| {
    stitch_contours_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "trace_geodesic_path" => builtin_fn!(trace_geodesic_path, |def_ix, arg_refs, args, kwargs, ctx| {
    trace_geodesic_path_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "text_to_mesh" => builtin_fn!(text_to_mesh, |def_ix, arg_refs, args, kwargs, _ctx| {
    text_to_mesh_impl(def_ix, arg_refs, args, kwargs)
  }),
  "alpha_wrap" => builtin_fn!(alpha_wrap, |def_ix, arg_refs, args, kwargs, ctx| {
    alpha_wrap_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "smooth" => builtin_fn!(smooth, |def_ix, arg_refs, args, kwargs, _ctx| {
    smooth_impl(def_ix, arg_refs, args, kwargs)
  }),
  "remesh_planar_patches" => builtin_fn!(remesh_planar_patches, |def_ix, arg_refs, args, kwargs, _ctx| {
    remesh_planar_patches_impl(def_ix, arg_refs, args, kwargs)
  }),
  "isotropic_remesh" => builtin_fn!(isotropic_remesh, |def_ix, arg_refs, args, kwargs, _ctx| {
    isotropic_remesh_impl(def_ix, arg_refs, args, kwargs)
  }),
  "delaunay_remesh" => builtin_fn!(delaunay_remesh, |def_ix, arg_refs, args, kwargs, _ctx| {
    delaunay_remesh_impl(def_ix, arg_refs, args, kwargs)
  }),
  "sample_voxels" => builtin_fn!(sample_voxels, |def_ix, arg_refs, args, kwargs, ctx| {
    sample_voxels_impl(ctx, def_ix, arg_refs, args, kwargs)
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
  "curl_noise" => builtin_fn!(curl_noise, |def_ix, arg_refs, args, kwargs, _ctx| {
    curl_noise_impl(def_ix, arg_refs, args, kwargs)
  }),
  "ridged_multifractal" => builtin_fn!(ridged_multifractal, |def_ix, arg_refs, args, kwargs, _ctx| {
    ridged_multifractal_impl(def_ix, arg_refs, args, kwargs)
  }),
  "worley_noise" => builtin_fn!(worley_noise, |def_ix, arg_refs, args, kwargs, _ctx| {
    worley_noise_impl(def_ix, arg_refs, args, kwargs)
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
  "set_rng_seed" => builtin_fn!(set_rng_seed, |def_ix, arg_refs, args, kwargs, ctx| {
    set_rng_seed_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
  "set_sharp_angle_threshold" => builtin_fn!(set_sharp_angle_threshold, |def_ix, arg_refs, args, kwargs, ctx| {
    set_sharp_angle_threshold_impl(ctx, def_ix, arg_refs, args, kwargs)
  }),
};

pub(crate) fn resolve_builtin_impl(
  name: &str,
) -> fn(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
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
