use std::sync::Arc;

use fxhash::FxHashMap;
use mesh::{
  linked_mesh::{DisplacementNormalMethod, Vec3},
  LinkedMesh,
};
use rand::Rng;

use crate::{
  mesh_ops::{
    extrude_pipe,
    mesh_boolean::{eval_mesh_boolean, MeshBooleanOp},
    mesh_ops::{convex_hull_from_verts, simplify_mesh},
  },
  noise::fbm,
  path_building::{build_torus_knot_path, cubic_bezier_3d_path},
  seq::{FilterSeq, IteratorSeq, MeshVertsSeq, PointDistributeSeq},
  ArgRef, ArgType, Callable, ComposedFn, ErrorStack, EvalCtx, MapSeq, Value,
};

// TODO: support optional arguments and default values
pub(crate) static FN_SIGNATURE_DEFS: phf::Map<&'static str, &[&[(&'static str, &[ArgType])]]> = phf::phf_map! {
  "sphere" => &[
    &[
      ("radius", &[ArgType::Numeric])
    ]
    // TODO: optional args for origin and for resolution
  ],
  "box" => &[
    &[
      ("width", &[ArgType::Numeric]),
      ("height", &[ArgType::Numeric]),
      ("depth", &[ArgType::Numeric]),
    ],
    &[
      ("size", &[ArgType::Vec3, ArgType::Numeric]),
    ]
    // TODO: optional args for origin and for resolution
  ],
  "translate" => &[
    &[
      ("translation", &[ArgType::Vec3]),
      ("mesh", &[ArgType::Mesh])
    ],
    &[
      ("x", &[ArgType::Numeric]),
      ("y", &[ArgType::Numeric]),
      ("z", &[ArgType::Numeric]),
      ("mesh", &[ArgType::Mesh]),
    ],
  ],
  "rot" => &[
    &[
      ("x", &[ArgType::Numeric]),
      ("y", &[ArgType::Numeric]),
      ("z", &[ArgType::Numeric]),
      ("mesh", &[ArgType::Mesh]),
    ]
  ],
  "scale" => &[
    &[
      ("x", &[ArgType::Numeric]),
      ("y", &[ArgType::Numeric]),
      ("z", &[ArgType::Numeric]),
      ("mesh", &[ArgType::Mesh]),
    ],
    &[
      ("scale", &[ArgType::Vec3, ArgType::Numeric]),
      ("mesh", &[ArgType::Mesh])
    ]
  ],
  "vec3" => &[
    &[
      ("x", &[ArgType::Numeric]),
      ("y", &[ArgType::Numeric]),
      ("z", &[ArgType::Numeric]),
    ],
    &[
      ("x", &[ArgType::Numeric])
    ],
  ],
  "union" => &[
    &[
      ("a", &[ArgType::Mesh]),
      ("b", &[ArgType::Mesh]),
    ],
    &[
      ("meshes", &[ArgType::Sequence]),
    ]
  ],
  "difference" => &[
    &[
      ("a", &[ArgType::Mesh]),
      ("b", &[ArgType::Mesh]),
    ],
    &[
      ("meshes", &[ArgType::Sequence]),
    ]
  ],
  "intersect" => &[
    &[
      ("a", &[ArgType::Mesh]),
      ("b", &[ArgType::Mesh]),
    ],
    &[
      ("meshes", &[ArgType::Sequence]),
    ]
  ],
  "fold" => &[
    &[
      ("initial_val", &[ArgType::Any]),
      ("fn", &[ArgType::Callable]),
      ("sequence", &[ArgType::Sequence]),
    ],
  ],
  "reduce" => &[
    &[
      ("fn", &[ArgType::Callable]),
      ("sequence", &[ArgType::Sequence]),
    ],
  ],
  "neg" => &[
    &[
      ("value", &[ArgType::Numeric]),
    ],
    &[
      ("value", &[ArgType::Vec3]),
    ],
    &[
      ("value", &[ArgType::Bool]),
    ],
  ],
  "pos" => &[
    &[
      ("value", &[ArgType::Numeric]),
    ],
    &[
      ("value", &[ArgType::Vec3]),
    ],
  ],
  "abs" => &[
    &[
      ("value", &[ArgType::Int]),
    ],
    &[
      ("value", &[ArgType::Float]),
    ],
    &[
      ("value", &[ArgType::Vec3]),
    ],
  ],
  "sqrt" => &[
    &[
      ("value", &[ArgType::Numeric]),
    ],
  ],
  "add" => &[
    &[
      ("a", &[ArgType::Vec3]),
      ("b", &[ArgType::Vec3]),
    ],
    &[
      ("a", &[ArgType::Numeric]),
      ("b", &[ArgType::Float]),
    ],
    &[
      ("a", &[ArgType::Float]),
      ("b", &[ArgType::Int]),
    ],
    &[
      ("a", &[ArgType::Int]),
      ("b", &[ArgType::Int]),
    ],
    &[
      ("a", &[ArgType::Mesh]),
      ("b", &[ArgType::Mesh]),
    ],
    // shorthand for translate
    &[
      ("mesh", &[ArgType::Mesh]),
      ("offset", &[ArgType::Vec3]),
    ]
  ],
  "sub" => &[
    &[
      ("a", &[ArgType::Vec3]),
      ("b", &[ArgType::Vec3]),
    ],
    &[
      ("a", &[ArgType::Numeric]),
      ("b", &[ArgType::Float]),
    ],
    &[
      ("a", &[ArgType::Float]),
      ("b", &[ArgType::Int]),
    ],
    &[
      ("a", &[ArgType::Int]),
      ("b", &[ArgType::Int]),
    ],
    &[
      ("a", &[ArgType::Mesh]),
      ("b", &[ArgType::Mesh]),
    ],
    // shorthands for translate
    &[
      ("mesh", &[ArgType::Mesh]),
      ("offset", &[ArgType::Vec3]),
    ]
  ],
  "mul" => &[
    &[
      ("a", &[ArgType::Vec3]),
      ("b", &[ArgType::Vec3]),
    ],
    &[
      ("a", &[ArgType::Vec3]),
      ("b", &[ArgType::Numeric]),
    ],
    &[
      ("a", &[ArgType::Numeric]),
      ("b", &[ArgType::Float]),
    ],
    &[
      ("a", &[ArgType::Float]),
      ("b", &[ArgType::Int]),
    ],
    &[
      ("a", &[ArgType::Int]),
      ("b", &[ArgType::Int]),
    ],
    // shorthands for scale
    &[
      ("mesh", &[ArgType::Mesh]),
      ("factor", &[ArgType::Numeric]),
    ],
    &[
      ("mesh", &[ArgType::Mesh]),
      ("factor", &[ArgType::Vec3]),
    ]
  ],
  "div" => &[
    &[
      ("a", &[ArgType::Vec3]),
      ("b", &[ArgType::Vec3]),
    ],
    &[
      ("a", &[ArgType::Vec3]),
      ("b", &[ArgType::Numeric]),
    ],
    &[
      ("a", &[ArgType::Numeric]),
      ("b", &[ArgType::Float]),
    ],
    &[
      ("a", &[ArgType::Float]),
      ("b", &[ArgType::Int]),
    ],
    &[
      ("a", &[ArgType::Int]),
      ("b", &[ArgType::Int]),
    ],
  ],
  "mod" => &[
    &[
      ("a", &[ArgType::Int]),
      ("b", &[ArgType::Int]),
    ],
    &[
      ("a", &[ArgType::Float]),
      ("b", &[ArgType::Float]),
    ],
  ],
  "and" => &[
    &[
      ("a", &[ArgType::Bool]),
      ("b", &[ArgType::Bool]),
    ],
    &[
      ("a", &[ArgType::Mesh]),
      ("b", &[ArgType::Mesh]),
    ],
  ],
  "or" => &[
    &[
      ("a", &[ArgType::Bool]),
      ("b", &[ArgType::Bool]),
    ],
    &[
      ("a", &[ArgType::Mesh]),
      ("b", &[ArgType::Mesh]),
    ],
  ],
  "xor" => &[
    &[
      ("a", &[ArgType::Bool]),
      ("b", &[ArgType::Bool]),
    ],
  ],
  "not" => &[
    &[
      ("value", &[ArgType::Bool]),
    ],
  ],
  "bit_and" => &[
    &[
      ("a", &[ArgType::Int]),
      ("b", &[ArgType::Int]),
    ],
    &[
      ("a", &[ArgType::Mesh]),
      ("b", &[ArgType::Mesh]),
    ],
  ],
  "bit_or" => &[
    &[
      ("a", &[ArgType::Int]),
      ("b", &[ArgType::Int]),
    ],
    &[
      ("a", &[ArgType::Mesh]),
      ("b", &[ArgType::Mesh]),
    ],
  ],
  "map" => &[
    &[
      ("fn", &[ArgType::Callable]),
      ("sequence", &[ArgType::Sequence]),
    ],
  ],
  "filter" => &[
    &[
      ("fn", &[ArgType::Callable]),
      ("sequence", &[ArgType::Sequence]),
    ],
  ],
  "print" => &[],
  "render" => &[
    &[
      ("mesh", &[ArgType::Mesh]),
    ],
    &[
      ("meshes", &[ArgType::Sequence]),
    ]
  ],
  "sin" => &[
    &[
      ("value", &[ArgType::Numeric]),
    ],
    &[
      ("value", &[ArgType::Vec3]),
    ]
  ],
  "cos" => &[
    &[
      ("value", &[ArgType::Numeric]),
    ],
    &[
      ("value", &[ArgType::Vec3]),
    ]
  ],
  "tan" => &[
    &[
      ("value", &[ArgType::Numeric]),
    ],
    &[
      ("value", &[ArgType::Vec3]),
    ]
  ],
  "rad2deg" => &[
    &[
      ("value", &[ArgType::Numeric]),
    ],
  ],
  "deg2rad" => &[
    &[
      ("value", &[ArgType::Numeric]),
    ],
  ],
  "gte" => &[
    &[
      ("a", &[ArgType::Int]),
      ("b", &[ArgType::Int]),
    ],
    &[
      ("a", &[ArgType::Numeric]),
      ("b", &[ArgType::Numeric]),
    ],
  ],
  "lte" => &[
    &[
      ("a", &[ArgType::Int]),
      ("b", &[ArgType::Int]),
    ],
    &[
      ("a", &[ArgType::Numeric]),
      ("b", &[ArgType::Numeric]),
    ],
  ],
  "gt" => &[
    &[
      ("a", &[ArgType::Int]),
      ("b", &[ArgType::Int]),
    ],
    &[
      ("a", &[ArgType::Numeric]),
      ("b", &[ArgType::Numeric]),
    ],
  ],
  "lt" => &[
    &[
      ("a", &[ArgType::Int]),
      ("b", &[ArgType::Int]),
    ],
    &[
      ("a", &[ArgType::Numeric]),
      ("b", &[ArgType::Numeric]),
    ],
  ],
  "eq" => &[
    &[
      ("a", &[ArgType::Int]),
      ("b", &[ArgType::Int]),
    ],
    &[
      ("a", &[ArgType::Numeric]),
      ("b", &[ArgType::Numeric]),
    ],
  ],
  "neq" => &[
    &[
      ("a", &[ArgType::Int]),
      ("b", &[ArgType::Int]),
    ],
    &[
      ("a", &[ArgType::Numeric]),
      ("b", &[ArgType::Numeric]),
    ],
  ],
  "point_distribute" => &[
    &[
      ("count", &[ArgType::Int]),
      ("mesh", &[ArgType::Mesh]),
    ],
  ],
  "lerp" => &[
    &[
      ("a", &[ArgType::Vec3]),
      ("b", &[ArgType::Vec3]),
      ("t", &[ArgType::Float]),
    ],
    &[
      ("a", &[ArgType::Numeric]),
      ("b", &[ArgType::Numeric]),
      ("t", &[ArgType::Float]),
    ],
  ],
  "compose" => &[],
  "join" => &[
    &[
      ("strings", &[ArgType::Sequence]),
    ],
  ],
  "convex_hull" => &[
    &[
      ("points", &[ArgType::Sequence]),
    ],
    &[
      ("elems", &[ArgType::Sequence]),
    ],
  ],
  "warp" => &[
    &[
      ("fn", &[ArgType::Callable]),
      ("mesh", &[ArgType::Mesh]),
    ],
  ],
  "tessellate" => &[
    &[
      ("target_edge_length", &[ArgType::Numeric]),
      ("mesh", &[ArgType::Mesh]),
    ],
  ],
  "len" => &[
    &[
      ("v", &[ArgType::Vec3]),
    ],
  ],
  "distance" => &[
    &[
      ("a", &[ArgType::Vec3]),
      ("b", &[ArgType::Vec3]),
    ],
  ],
  // cubic bezier curve
  "bezier3d" => &[
    &[
      ("p0", &[ArgType::Vec3]),
      ("p1", &[ArgType::Vec3]),
      ("p2", &[ArgType::Vec3]),
      ("p3", &[ArgType::Vec3]),
      ("count", &[ArgType::Int]),
    ],
  ],
  "extrude_pipe" => &[
    &[
      ("radius", &[ArgType::Numeric, ArgType::Callable]),
      ("resolution", &[ArgType::Int]),
      ("path", &[ArgType::Sequence]),
      ("close_ends", &[ArgType::Bool]),
    ]
  ],
  "torus_knot_path" => &[
    &[
      ("radius", &[ArgType::Numeric]),
      ("tube_radius", &[ArgType::Numeric]),
      ("p", &[ArgType::Int]),
      ("q", &[ArgType::Int]),
      ("point_count", &[ArgType::Int]),
    ]
  ],
  "torus_knot" => &[
    &[
      ("radius", &[ArgType::Numeric]),
      ("tube_radius", &[ArgType::Numeric]),
      ("p", &[ArgType::Int]),
      ("q", &[ArgType::Int]),
      ("point_count", &[ArgType::Int]),
      ("tube_resolution", &[ArgType::Int]),
    ]
  ],
  "simplify" => &[
    &[
      ("tolerance", &[ArgType::Numeric]),
      ("mesh", &[ArgType::Mesh]),
    ]
  ],
  "verts" => &[
    &[
      ("mesh", &[ArgType::Mesh]),
    ]
  ],
  "randf" => &[
    &[],
    &[
      ("min", &[ArgType::Float]),
      ("max", &[ArgType::Float]),
    ],
  ],
  "randi" => &[
    &[],
    &[
      ("min", &[ArgType::Int]),
      ("max", &[ArgType::Int]),
    ],
  ],
  "randv" => &[
    &[],
    &[
      ("mins", &[ArgType::Vec3]),
      ("maxs", &[ArgType::Vec3]),
    ],
  ],
  "fbm" => &[
    &[
      ("seed", &[ArgType::Int]),
      ("octaves", &[ArgType::Int]),
      ("frequency", &[ArgType::Float]),
      ("lacunarity", &[ArgType::Float]),
      ("gain", &[ArgType::Float]),
      ("pos", &[ArgType::Vec3]),
    ],
    &[
      ("pos", &[ArgType::Vec3]),
    ]
  ],
};

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
};

enum BoolOp {
  Gte,
  Lte,
  Gt,
  Lt,
  Eq,
  Neq,
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
        BoolOp::Eq => a == b,
        BoolOp::Neq => a != b,
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
        BoolOp::Eq => a == b,
        BoolOp::Neq => a != b,
      };
      Ok(Value::Bool(result))
    }
    _ => unimplemented!(),
  }
}

pub(crate) fn eval_builtin_fn(
  name: &str,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<String, Value>,
  ctx: &EvalCtx,
) -> Result<Value, ErrorStack> {
  match name {
    "sphere" => todo!(),
    // TODO: these should be merged in with the function signature defs to avoid double hashmap
    // lookups
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
      Ok(Value::Mesh(Arc::new(LinkedMesh::new_box(
        width, height, depth,
      ))))
    }
    "translate" => {
      let (translation, mesh) = match def_ix {
        0 => {
          let translation = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
          let mesh = arg_refs[1].resolve(args, &kwargs);
          (*translation, mesh)
        }
        1 => {
          let x = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
          let y = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
          let z = arg_refs[2].resolve(args, &kwargs).as_float().unwrap();
          let translation = Vec3::new(x, y, z);
          let mesh = arg_refs[3].resolve(args, &kwargs);
          (translation, mesh)
        }
        _ => unimplemented!(),
      };

      let mesh = mesh.as_mesh().unwrap();
      let mut translated_mesh = (*mesh).clone();

      // TODO: use built-in transform instead of modifying vertices directly?
      for vtx in translated_mesh.vertices.values_mut() {
        vtx.position.x += translation.x;
        vtx.position.y += translation.y;
        vtx.position.z += translation.z;
      }

      Ok(Value::Mesh(Arc::new(translated_mesh)))
    }
    "scale" => {
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

      // TODO: make use of the mesh's transform rather than modifying vertices directly?
      let mesh = mesh
        .as_mesh()
        .ok_or(ErrorStack::new("Scale function requires a mesh argument"))?;
      let mut mesh = (*mesh).clone();
      for vtx in mesh.vertices.values_mut() {
        vtx.position.x *= scale.x;
        vtx.position.y *= scale.y;
        vtx.position.z *= scale.z;
      }

      Ok(Value::Mesh(Arc::new(mesh)))
    }
    "rot" => match def_ix {
      0 => {
        let x = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        let y = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
        let z = arg_refs[2].resolve(args, &kwargs).as_float().unwrap();
        let mesh = arg_refs[3].resolve(args, &kwargs).as_mesh().unwrap();

        // interpret as a euler angle in radians
        let mut rotated_mesh = (*mesh).clone();
        let rotation = nalgebra::UnitQuaternion::from_euler_angles(x, y, z);
        for vtx in rotated_mesh.vertices.values_mut() {
          vtx.position = rotation * vtx.position;
        }

        Ok(Value::Mesh(Arc::new(rotated_mesh)))
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

      ctx.fold(initial_val, fn_value, sequence.clone_box().consume(ctx))
    }
    "reduce" => {
      let fn_value = arg_refs[0].resolve(args, &kwargs).as_callable().unwrap();
      let sequence = arg_refs[1].resolve(args, &kwargs).as_sequence().unwrap();

      ctx.reduce(fn_value, sequence.clone_box())
    }
    "map" => {
      let fn_value = arg_refs[0].resolve(args, &kwargs).as_callable().unwrap();
      let sequence = arg_refs[1].resolve(args, &kwargs).as_sequence().unwrap();

      Ok(Value::Sequence(Box::new(MapSeq {
        cb: fn_value.clone(),
        inner: sequence.clone_box(),
      })))
    }
    "filter" => {
      let fn_value = arg_refs[0].resolve(args, &kwargs).as_callable().unwrap();
      let sequence = arg_refs[1].resolve(args, &kwargs).as_sequence().unwrap();

      Ok(Value::Sequence(Box::new(FilterSeq {
        cb: fn_value.clone(),
        inner: sequence.clone_box(),
      })))
    }
    "neg" => match def_ix {
      0 => {
        // negate numeric value
        let value = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        Ok(Value::Float(-value))
      }
      1 => {
        // negate vec3
        let value = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
        Ok(Value::Vec3(-*value))
      }
      2 => {
        // negate bool
        let value = arg_refs[0].resolve(args, &kwargs).as_bool().unwrap();
        Ok(Value::Bool(!value))
      }
      _ => unimplemented!(),
    },
    "pos" => match def_ix {
      0 => {
        // pass through numeric value
        Ok(arg_refs[0].resolve(args, &kwargs).clone())
      }
      1 => {
        // pass through vec3
        Ok(arg_refs[0].resolve(args, &kwargs).clone())
      }
      _ => unimplemented!(),
    },
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
    "add" => match def_ix {
      0 => {
        // vec3 + vec3
        let a = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_vec3().unwrap();
        Ok(Value::Vec3(*a + *b))
      }
      1 => {
        // float + float
        let a = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
        Ok(Value::Float(a + b))
      }
      2 => {
        // float + int
        let a = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_int().unwrap();
        Ok(Value::Float(a + b as f32))
      }
      3 => {
        // int + int
        let a = arg_refs[0].resolve(args, &kwargs).as_int().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_int().unwrap();
        Ok(Value::Int(a + b))
      }
      4 => {
        // mesh + mesh
        eval_mesh_boolean(0, arg_refs, args, kwargs, ctx, MeshBooleanOp::Union)
      }
      5 => ctx.eval_fn_call(
        "translate",
        &[
          arg_refs[1].resolve(args, &kwargs).clone(),
          arg_refs[0].resolve(args, &kwargs).clone(),
        ],
        Default::default(),
        &ctx.globals,
        true,
      ),
      _ => unimplemented!(),
    },
    "sub" => match def_ix {
      0 => {
        // vec3 - vec3
        let a = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_vec3().unwrap();
        Ok(Value::Vec3(*a - *b))
      }
      1 => {
        // float - float
        let a = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
        Ok(Value::Float(a - b))
      }
      2 => {
        // float - int
        let a = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_int().unwrap();
        Ok(Value::Float(a - b as f32))
      }
      3 => {
        // int - int
        let a = arg_refs[0].resolve(args, &kwargs).as_int().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_int().unwrap();
        Ok(Value::Int(a - b))
      }
      4 => {
        // mesh - mesh
        eval_mesh_boolean(0, arg_refs, args, kwargs, ctx, MeshBooleanOp::Difference)
      }
      5 => ctx.eval_fn_call(
        "translate",
        &[
          arg_refs[1].resolve(args, &kwargs).clone(),
          arg_refs[0].resolve(args, &kwargs).clone(),
        ],
        Default::default(),
        &ctx.globals,
        true,
      ),
      _ => unimplemented!(),
    },
    "mul" => match def_ix {
      0 => {
        let a = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_vec3().unwrap();
        Ok(Value::Vec3(Vec3::new(a.x * b.x, a.y * b.y, a.z * b.z)))
      }
      1 => {
        let a = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
        Ok(Value::Vec3(a * b))
      }
      2 => {
        // float * float
        let a = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
        Ok(Value::Float(a * b))
      }
      3 => {
        // float * int
        let a = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_int().unwrap();
        Ok(Value::Float(a * b as f32))
      }
      4 => {
        // int * int
        let a = arg_refs[0].resolve(args, &kwargs).as_int().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_int().unwrap();
        Ok(Value::Int(a * b))
      }
      5 | 6 => {
        // scale mesh by float
        ctx.eval_fn_call(
          "scale",
          &[
            arg_refs[1].resolve(args, &kwargs).clone(),
            arg_refs[0].resolve(args, &kwargs).clone(),
          ],
          Default::default(),
          &ctx.globals,
          true,
        )
      }
      _ => unimplemented!(),
    },
    "div" => match def_ix {
      0 => {
        let a = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_vec3().unwrap();
        Ok(Value::Vec3(Vec3::new(a.x / b.x, a.y / b.y, a.z / b.z)))
      }
      1 => {
        let a = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
        Ok(Value::Vec3(a / b))
      }
      2 => {
        // float / float
        let a = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
        Ok(Value::Float(a / b))
      }
      3 => {
        // float / int
        let a = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_int().unwrap();
        Ok(Value::Float(a / b as f32))
      }
      4 => {
        // int / int
        let a = arg_refs[0].resolve(args, &kwargs).as_int().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_int().unwrap();
        // there's basically no reason to do real integer division, so just treating things as
        // floats in this case makes so much more sense
        Ok(Value::Float((a as f32) / (b as f32)))
      }
      _ => unimplemented!(),
    },
    "mod" => match def_ix {
      0 => {
        // int % int
        let a = arg_refs[0].resolve(args, &kwargs).as_int().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_int().unwrap();
        Ok(Value::Int(a % b))
      }
      1 => {
        // float % float
        let a = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
        Ok(Value::Float(a % b))
      }
      _ => unimplemented!(),
    },
    "gte" => eval_numeric_bool_op(def_ix, arg_refs, args, kwargs, BoolOp::Gte),
    "lte" => eval_numeric_bool_op(def_ix, arg_refs, args, kwargs, BoolOp::Lte),
    "gt" => eval_numeric_bool_op(def_ix, arg_refs, args, kwargs, BoolOp::Gt),
    "lt" => eval_numeric_bool_op(def_ix, arg_refs, args, kwargs, BoolOp::Lt),
    "eq" => eval_numeric_bool_op(def_ix, arg_refs, args, kwargs, BoolOp::Eq),
    "neq" => eval_numeric_bool_op(def_ix, arg_refs, args, kwargs, BoolOp::Neq),
    "and" => match def_ix {
      0 => {
        let a = arg_refs[0].resolve(args, &kwargs).as_bool().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_bool().unwrap();
        Ok(Value::Bool(a && b))
      }
      1 => eval_mesh_boolean(0, arg_refs, args, kwargs, ctx, MeshBooleanOp::Intersection),
      _ => unimplemented!(),
    },
    "or" => match def_ix {
      0 => {
        let a = arg_refs[0].resolve(args, &kwargs).as_bool().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_bool().unwrap();
        Ok(Value::Bool(a || b))
      }
      1 => eval_mesh_boolean(0, arg_refs, args, kwargs, ctx, MeshBooleanOp::Union),
      _ => unimplemented!(),
    },
    "not" => match def_ix {
      0 => {
        let value = arg_refs[0].resolve(args, &kwargs).as_bool().unwrap();
        Ok(Value::Bool(!value))
      }
      _ => unimplemented!(),
    },
    "bit_and" => match def_ix {
      0 => {
        let a = arg_refs[0].resolve(args, &kwargs).as_int().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_int().unwrap();
        Ok(Value::Int(a & b))
      }
      1 => eval_mesh_boolean(0, arg_refs, args, kwargs, ctx, MeshBooleanOp::Intersection),
      _ => unimplemented!(),
    },
    "bit_or" => match def_ix {
      0 => {
        let a = arg_refs[0].resolve(args, &kwargs).as_int().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_int().unwrap();
        Ok(Value::Int(a | b))
      }
      1 => eval_mesh_boolean(0, arg_refs, args, kwargs, ctx, MeshBooleanOp::Union),
      _ => unimplemented!(),
    },
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
        let mesh = arg_refs[0].resolve(args, &kwargs).as_mesh().unwrap();
        ctx.rendered_meshes.push(mesh.clone());
        Ok(Value::Nil)
      }
      1 => {
        let sequence = arg_refs[0]
          .resolve(args, &kwargs)
          .as_sequence()
          .unwrap()
          .clone_box();
        for res in sequence.consume(ctx) {
          let mesh = res.map_err(|err| err.wrap("Error evaluating mesh in render"))?;
          if let Value::Mesh(mesh) = mesh {
            ctx.rendered_meshes.push(mesh);
          } else {
            return Err(ErrorStack::new(
              "Render function expects a sequence of meshes",
            ));
          }
        }
        Ok(Value::Nil)
      }
      _ => unimplemented!(),
    },
    "point_distribute" => {
      let count = arg_refs[0].resolve(args, &kwargs).as_int().unwrap();
      let mesh = arg_refs[1].resolve(args, &kwargs).as_mesh().unwrap();

      if count < 0 {
        return Err(ErrorStack::new(
          "negative point count is not valid for point_distribute",
        ));
      }

      let sampler_seq = PointDistributeSeq {
        mesh: mesh.clone(),
        point_count: count as usize,
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

      Ok(Value::Callable(Callable::ComposedFn(ComposedFn { inner })))
    }
    "join" => match def_ix {
      0 => {
        let seq = arg_refs[0].resolve(args, &kwargs).as_sequence().unwrap();
        ctx.reduce(&Callable::Builtin("union".to_owned()), seq.clone_box())
      }
      _ => unimplemented!(),
    },
    "warp" => match def_ix {
      0 => {
        let warp_fn = arg_refs[0].resolve(args, &kwargs).as_callable().unwrap();
        let mesh = arg_refs[1].resolve(args, &kwargs).as_mesh().unwrap();

        let mut needs_displacement_normals_computed = false;
        let mut new_mesh = (*mesh).clone();
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
              Default::default(),
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

        Ok(Value::Mesh(Arc::new(new_mesh)))
      }
      _ => unimplemented!(),
    },
    "tessellate" => match def_ix {
      0 => {
        let target_edge_length = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        let mesh = arg_refs[1].resolve(args, &kwargs).as_mesh().unwrap();

        let mut mesh = (*mesh).clone();
        tessellation::tessellate_mesh(
          &mut mesh,
          target_edge_length,
          DisplacementNormalMethod::Interpolate,
        );
        Ok(Value::Mesh(Arc::new(mesh)))
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

        let path = path.clone_box().consume(ctx).map(|res| match res {
          Ok(Value::Vec3(v)) => Ok(v),
          Ok(val) => Err(ErrorStack::new(format!(
            "Expected Vec3 in path seq passed to `extrude_pipe`, found: {val:?}"
          ))),
          Err(err) => Err(err),
        });

        let mesh = match radius {
          _ if let Some(radius) = radius.as_float() => {
            extrude_pipe(|_, _| Ok(radius), resolution, path, close_ends)?
          }
          _ if let Some(get_radius) = radius.as_callable() => {
            let get_radius = |i: usize, pos: Vec3| -> Result<_, ErrorStack> {
              let val = ctx
                .invoke_callable(
                  get_radius,
                  &[Value::Int(i as i64), Value::Vec3(pos)],
                  Default::default(),
                  &ctx.globals,
                )
                .map_err(|err| err.wrap("Error calling radius cb in `extrude_pipe`"))?;
              match val {
                Value::Float(f) => Ok(f),
                Value::Int(i) => Ok(i as f32),
                _ => Err(ErrorStack::new(format!(
                  "Expected Float or Int from radius cb in `extrude_pipe`, found: {val:?}"
                ))),
              }
            };
            extrude_pipe(get_radius, resolution, path, close_ends)?
          }
          _ => {
            return Err(ErrorStack::new(format!(
              "Invalid radius argument for `extrude_pipe`; expected Float, Int, or Callable, \
               found: {radius:?}",
            )))
          }
        };
        Ok(Value::Mesh(Arc::new(mesh)))
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

        let out_mesh = simplify_mesh(&mesh, tolerance)
          .map_err(|err| ErrorStack::new(err).wrap("Error in `simplify` function"))?;
        Ok(Value::Mesh(Arc::new(out_mesh)))
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
        Ok(Value::Mesh(Arc::new(out_mesh)))
      }
      _ => unimplemented!(),
    },
    "verts" => match def_ix {
      0 => {
        let mesh = arg_refs[0].resolve(args, &kwargs).as_mesh().unwrap();
        Ok(Value::Sequence(Box::new(MeshVertsSeq { mesh })))
      }
      _ => unimplemented!(),
    },
    "randf" => match def_ix {
      0 => Ok(Value::Float(ctx.rng().gen())),
      1 => {
        let min = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        let max = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
        Ok(Value::Float(ctx.rng().gen_range(min..max)))
      }
      _ => unimplemented!(),
    },
    "randv" => match def_ix {
      0 => Ok(Value::Vec3(Vec3::new(
        ctx.rng().gen(),
        ctx.rng().gen(),
        ctx.rng().gen(),
      ))),
      1 => {
        let min = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
        let max = arg_refs[1].resolve(args, &kwargs).as_vec3().unwrap();
        Ok(Value::Vec3(Vec3::new(
          ctx.rng().gen_range(min.x..max.x),
          ctx.rng().gen_range(min.y..max.y),
          ctx.rng().gen_range(min.z..max.z),
        )))
      }
      _ => unimplemented!(),
    },
    "fbm" => match def_ix {
      0 => {
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
        let pos = arg_refs[4].resolve(args, &kwargs).as_vec3().unwrap();

        Ok(Value::Float(fbm(
          seed,
          octaves,
          frequency,
          persistence,
          lacunarity,
          *pos,
        )))
      }
      1 => {
        let pos = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
        Ok(Value::Float(fbm(0, 4, 1., 0.5, 2., *pos)))
      }
      _ => unimplemented!(),
    },
    _ => unimplemented!("Builtin function `{name}` not yet implemented"),
  }
}
