use std::{f32::consts::PI, sync::Arc};

use fxhash::FxHashMap;
use mesh::{
  linked_mesh::{DisplacementNormalMethod, Vec3},
  LinkedMesh,
};

use crate::{
  mesh_boolean::{eval_mesh_boolean, MeshBooleanOp},
  seq::{FilterSeq, IteratorSeq, PointDistributeSeq},
  ArgRef, ArgType, Callable, ComposedFn, EvalCtx, MapSeq, Value,
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
      ("mesh", &[ArgType::Mesh]),
      ("count", &[ArgType::Int]),
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
  // TODO
  "convex_hull" => &[
    &[
      ("points", &[ArgType::Sequence]),
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
      ("radius", &[ArgType::Numeric]),
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
) -> Result<Value, String> {
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
) -> Result<Value, String> {
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
              return Err(format!(
                "Invalid argument for box size: expected Vec3 or Float, found {:?}",
                val
              ))
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
              return Err(format!(
                "Invalid argument for scale: expected Vec3 or Float, found {:?}",
                val
              ))
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
        .ok_or("Scale function requires a mesh argument")?;
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
        // float * float
        let a = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_float().unwrap();
        Ok(Value::Float(a / b))
      }
      3 => {
        // float * int
        let a = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_int().unwrap();
        Ok(Value::Float(a / b as f32))
      }
      4 => {
        // int * int
        let a = arg_refs[0].resolve(args, &kwargs).as_int().unwrap();
        let b = arg_refs[1].resolve(args, &kwargs).as_int().unwrap();
        Ok(Value::Int(a / b))
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
          let mesh = res.map_err(|err| format!("Error evaluating mesh in render: {err}"))?;
          if let Value::Mesh(mesh) = mesh {
            ctx.rendered_meshes.push(mesh);
          } else {
            return Err("Render function expects a sequence of meshes".to_owned());
          }
        }
        Ok(Value::Nil)
      }
      _ => unimplemented!(),
    },
    "point_distribute" => {
      let mesh = arg_refs[0].resolve(args, &kwargs).as_mesh().unwrap();
      let count = arg_refs[1].resolve(args, &kwargs).as_int().unwrap();

      if count < 0 {
        return Err("negative point count is not valid for point_distribute".to_owned());
      }

      let sampler_seq = PointDistributeSeq {
        mesh: mesh.clone(),
        point_count: count as usize,
      };
      Ok(Value::Sequence(Box::new(sampler_seq)))
    }
    "compose" => {
      if !kwargs.is_empty() {
        return Err("compose function does not accept keyword arguments".to_owned());
      }

      if args.is_empty() {
        return Err("compose function requires at least one argument".to_owned());
      }

      let inner: Vec<Value> = if args.len() == 1 {
        if matches!(args[0], Value::Callable(_)) {
          return Ok(args[0].clone());
        }

        if let Some(seq) = args[0].as_sequence() {
          // have to eagerly evaluate the sequence to get the inner callables
          seq
            .clone_box()
            .consume(ctx)
            .collect::<Result<Vec<_>, _>>()?
        } else {
          return Err(format!(
            "compose function requires a sequence or callable if a single arg is provided, found: \
             {:?}",
            args[0]
          ));
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
            Err(format!(
              "Non-callable found in sequence passed to compose, found: {val:?}"
            ))
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

        let mut new_mesh = (*mesh).clone();
        for vtx in new_mesh.vertices.values_mut() {
          let warped_pos = ctx
            .invoke_callable(
              warp_fn,
              &[Value::Vec3(vtx.position)],
              Default::default(),
              &ctx.globals,
            )
            .map_err(|err| format!("error calling warp cb: {err}"))?;
          let warped_pos = warped_pos
            .as_vec3()
            .ok_or_else(|| format!("warp function must return Vec3, got: {:?}", warped_pos))?;
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
        let p0 = arg_refs[0].resolve(args, &kwargs).as_vec3().unwrap();
        let p1 = arg_refs[1].resolve(args, &kwargs).as_vec3().unwrap();
        let p2 = arg_refs[2].resolve(args, &kwargs).as_vec3().unwrap();
        let p3 = arg_refs[3].resolve(args, &kwargs).as_vec3().unwrap();
        let count = arg_refs[4].resolve(args, &kwargs).as_int().unwrap() as usize;

        fn bezier_curve_3d(
          p0: Vec3,
          p1: Vec3,
          p2: Vec3,
          p3: Vec3,
          count: usize,
        ) -> impl Iterator<Item = Result<Value, String>> + Clone + 'static {
          (0..=count).map(move |i| {
            let t = i as f32 / count as f32;
            let u = 1.0 - t;
            let tt = t * t;
            let uu = u * u;
            let uuu = uu * u;
            let ttt = tt * t;

            Ok(Value::Vec3(
              uuu * p0 + 3.0 * uu * t * p1 + 3.0 * u * tt * p2 + ttt * p3,
            ))
          })
        }

        let curve = bezier_curve_3d(*p0, *p1, *p2, *p3, count);
        Ok(Value::Sequence(Box::new(IteratorSeq { inner: curve })))
      }

      _ => unimplemented!(),
    },
    "extrude_pipe" => match def_ix {
      0 => {
        let radius = arg_refs[0].resolve(args, &kwargs).as_float().unwrap();
        let resolution = arg_refs[1].resolve(args, &kwargs).as_int().unwrap() as usize;
        let path = arg_refs[2].resolve(args, &kwargs).as_sequence().unwrap();
        let close_ends = arg_refs[3].resolve(args, &kwargs).as_bool().unwrap();

        if resolution < 3 {
          return Err("`extrude_pipe` requires a resolution of at least 3".to_owned());
        }

        let points = path
          .clone_box()
          .consume(ctx)
          .map(|res| match res {
            Ok(Value::Vec3(v)) => Ok(v),
            Ok(val) => Err(format!(
              "Expected Vec3 in path seq passed to `extrude_pipe`, found: {val:?}"
            )),
            Err(err) => Err(err),
          })
          .collect::<Result<Vec<_>, _>>()?;

        if points.len() < 2 {
          return Err(format!(
            "`extrude_pipe` requires at least two points in the path, found: {}",
            points.len()
          ));
        }

        // Tangents are the direction of the path at each point.  There's some special handling for
        // the first and last points.
        let mut tangents: Vec<Vec3> = Vec::with_capacity(points.len());
        for i in 0..points.len() {
          let dir = if i == points.len() - 1 {
            points[i] - points[i - 1]
          } else {
            points[i + 1] - points[i]
          };
          tangents.push(dir.normalize());
        }

        // Rotation-minimizing frames are used to avoid twists or kinks in the mesh as it's
        // generated along the path.
        //
        // Rather than using a fixed up vector, the normal is projected forward using the new
        // tangent to minimize its rotation from the previous ring.

        // an initial normal is picked using an arbitrary up vector.
        let t0 = tangents[0];
        let mut up = Vec3::new(0., 1., 0.);
        // if the chosen up vector is nearly parallel to the tangent, a different one is picked to
        // avoid numerical issues
        if t0.dot(&up).abs() > 0.999 {
          up = Vec3::new(1., 0., 0.);
        }
        let mut normal = t0.cross(&up).normalize();
        // the "binormal" is a vector that's perpendicular to the plane defined by the tangent and
        // normal.
        let mut binormal = t0.cross(&normal).normalize();

        let mut verts: Vec<Vec3> = Vec::with_capacity(points.len() * resolution);

        let center0 = points[0];
        for j in 0..resolution {
          let theta = 2. * PI * (j as f32) / (resolution as f32);
          let dir = normal * theta.cos() + binormal * theta.sin();
          verts.push(center0 + dir * radius);
        }

        for i in 1..points.len() {
          let ti = tangents[i];
          // Project previous normal onto plane ⟂ tangentᵢ
          let dot = ti.dot(&normal);
          let mut proj = normal - ti * dot;
          const EPSILON: f32 = 1e-6;
          if proj.norm_squared() < EPSILON {
            // the same check as before is done to avoid numerical issues if the projected normal is
            // very close to 0
            proj = ti.cross(&binormal);
            if proj.norm_squared() < EPSILON {
              // In the extremely degenerate case, pick any vector ⟂ tangentᵢ
              let arbitrary = if ti.dot(&Vec3::new(0., 1., 0.)).abs() > 0.999 {
                Vec3::new(1., 0., 0.)
              } else {
                Vec3::new(0., 1., 0.)
              };
              proj = ti.cross(&arbitrary);
            }
          }
          normal = proj.normalize();
          binormal = ti.cross(&normal).normalize();

          let center = points[i];
          for j in 0..resolution {
            let theta = 2. * PI * (j as f32) / (resolution as f32);
            let dir = normal * theta.cos() + binormal * theta.sin();
            verts.push(center + dir * radius);
          }
        }

        assert_eq!(verts.len(), points.len() * resolution);

        // stitch the rings together with quads, two triangles per quad
        let mut index_count = (points.len() - 1) * resolution * 3 * 2;
        if close_ends {
          // `n-2` triangles are needed to tessellate a convex polygon of `n` vertices/edges
          let cap_triangles = resolution - 2;
          index_count += cap_triangles * 3 * 2;
        }
        let mut indices: Vec<u32> = Vec::with_capacity(index_count);

        for i in 0..(points.len() - 1) {
          for j in 0..resolution {
            let a = (i * resolution + j) as u32;
            let b = (i * resolution + (j + 1) % resolution) as u32;
            let c = ((i + 1) * resolution + j) as u32;
            let d = ((i + 1) * resolution + (j + 1) % resolution) as u32;

            indices.push(a);
            indices.push(b);
            indices.push(c);

            indices.push(b);
            indices.push(d);
            indices.push(c);
          }
        }

        if close_ends {
          for (ix_offset, reverse_winding) in [
            (0u32, true),
            ((points.len() - 1) as u32 * resolution as u32, false),
          ] {
            // using a basic triangle fan to form the end caps
            //
            // 0,1,2
            // 0,2,3
            // ...
            // 0,2n-2,2n-1
            for vtx_ix in 1..(resolution - 1) {
              let a = 0;
              let b = vtx_ix as u32;
              let c = (vtx_ix + 1) as u32;

              if reverse_winding {
                indices.push(ix_offset + c);
                indices.push(ix_offset + b);
                indices.push(ix_offset + a);
              } else {
                indices.push(ix_offset + a);
                indices.push(ix_offset + b);
                indices.push(ix_offset + c);
              }
            }
          }
        }

        assert_eq!(indices.len(), index_count);

        let mesh = LinkedMesh::from_indexed_vertices(&verts, &indices, None, None);
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

        pub fn sample_torus_knot(
          p: usize,
          q: usize,
          radius: f32,
          tube_radius: f32,
          t: f32,
        ) -> Vec3 {
          let t = 2. * PI * t;
          let p = p as f32;
          let q = q as f32;
          let qt = q * t;
          let pt = p * t;
          let radius = radius + tube_radius * qt.cos();
          let x = radius * pt.cos();
          let y = radius * pt.sin();
          let z = tube_radius * qt.sin();
          Vec3::new(x, y, z)
        }

        Ok(Value::Sequence(Box::new(IteratorSeq {
          inner: (0..=count).map(move |i| {
            let t = i as f32 / count as f32;
            Ok(Value::Vec3(sample_torus_knot(p, q, radius, tube_radius, t)))
          }),
        })))
      }
      _ => unreachable!(),
    },
    _ => unimplemented!("Function `{name}` not yet implemented"),
  }
}
