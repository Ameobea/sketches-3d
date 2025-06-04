use fxhash::FxHashMap;
use mesh::{linked_mesh::Vec3, LinkedMesh};

use crate::{
  mesh_boolean::{eval_mesh_boolean, MeshBooleanOp},
  seq::PointDistributeSeq,
  ArgRef, ArgType, EvalCtx, MapSeq, Value,
};

// TODO: support optional arguments and default values
pub(crate) static FN_SIGNATURE_DEFS: phf::Map<&'static str, &[&[(&'static str, &[ArgType])]]> = phf::phf_map! {
  "sphere" => &[&[("radius", &[ArgType::Numeric])]],
  "box" => &[
    &[
      ("width", &[ArgType::Numeric]),
      ("height", &[ArgType::Numeric]),
      ("depth", &[ArgType::Numeric]),
    ],
    &[
      ("size", &[ArgType::Vec3, ArgType::Numeric]),
    ]
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
  "subtract" => &[
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
      ("fn", &[ArgType::PartiallyAppliedFn]),
      ("sequence", &[ArgType::Sequence]),
    ],
  ],
  "reduce" => &[
    &[
      ("fn", &[ArgType::PartiallyAppliedFn]),
      ("sequence", &[ArgType::Sequence]),
    ],
  ],
  "add" => &[
    &[
      ("a", &[ArgType::Vec3]),
      ("b", &[ArgType::Vec3]),
    ],
    &[
      ("a", &[ArgType::Float]),
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
      ("b", &[ArgType::Numeric]),
    ],
  ],
  "map" => &[
    &[
      ("fn", &[ArgType::PartiallyAppliedFn]),
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
  ],
  "cos" => &[
    &[
      ("value", &[ArgType::Numeric]),
    ],
  ],
  "tan" => &[
    &[
      ("value", &[ArgType::Numeric]),
    ],
  ],
  "point_distribute" => &[
    &[
      ("mesh", &[ArgType::Mesh]),
      ("count", &[ArgType::Int]),
    ],
  ],
};

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
    "box" => {
      let v3 = match def_ix {
        0 => {
          let x = arg_refs[0].resolve(&args, &kwargs).as_float().unwrap();
          let y = arg_refs[1].resolve(&args, &kwargs).as_float().unwrap();
          let z = arg_refs[2].resolve(&args, &kwargs).as_float().unwrap();
          Vec3::new(x, y, z)
        }
        1 => {
          let val = arg_refs[0].resolve(&args, &kwargs);
          match val {
            Value::Vec3(v3) => *v3,
            Value::Float(size) => Vec3::new(*size, *size, *size),
            Value::Int(size) => {
              let size = *size as f32;
              Vec3::new(size, size, size)
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
      Ok(Value::Mesh(LinkedMesh::new_box(v3.x, v3.y, v3.z)))
    }
    "translate" => {
      let (translation, mesh) = match def_ix {
        0 => {
          let translation = arg_refs[0].resolve(&args, &kwargs).as_vec3().unwrap();
          let mesh = arg_refs[1].resolve(&args, &kwargs);
          (*translation, mesh)
        }
        1 => {
          let x = arg_refs[0].resolve(&args, &kwargs).as_float().unwrap();
          let y = arg_refs[1].resolve(&args, &kwargs).as_float().unwrap();
          let z = arg_refs[2].resolve(&args, &kwargs).as_float().unwrap();
          let translation = Vec3::new(x, y, z);
          let mesh = arg_refs[3].resolve(&args, &kwargs);
          (translation, mesh)
        }
        _ => unimplemented!(),
      };

      let mesh = mesh.as_mesh().unwrap();
      let mut translated_mesh = mesh.clone();

      // TODO: use built-in transform instead of modifying vertices directly?
      for vtx in translated_mesh.vertices.values_mut() {
        vtx.position.x += translation.x;
        vtx.position.y += translation.y;
        vtx.position.z += translation.z;
      }

      Ok(Value::Mesh(translated_mesh))
    }
    "scale" => {
      let (scale, mesh) = match def_ix {
        0 => {
          let x = arg_refs[0].resolve(&args, &kwargs).as_float().unwrap();
          let y = arg_refs[1].resolve(&args, &kwargs).as_float().unwrap();
          let z = arg_refs[2].resolve(&args, &kwargs).as_float().unwrap();
          (Vec3::new(x, y, z), arg_refs[3].resolve(&args, &kwargs))
        }
        1 => {
          let val = arg_refs[0].resolve(&args, &kwargs);
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

          let mesh = arg_refs[1].resolve(&args, &kwargs);
          (scale, mesh)
        }
        _ => unimplemented!(),
      };

      // TODO: make use of the mesh's transform rather than modifying vertices directly?
      let mut mesh = mesh
        .as_mesh()
        .ok_or("Scale function requires a mesh argument")?
        .clone();
      for vtx in mesh.vertices.values_mut() {
        vtx.position.x *= scale.x;
        vtx.position.y *= scale.y;
        vtx.position.z *= scale.z;
      }

      Ok(Value::Mesh(mesh))
    }
    "vec3" => match def_ix {
      0 => {
        let x = arg_refs[0].resolve(&args, &kwargs).as_float().unwrap();
        let y = arg_refs[1].resolve(&args, &kwargs).as_float().unwrap();
        let z = arg_refs[2].resolve(&args, &kwargs).as_float().unwrap();
        Ok(Value::Vec3(Vec3::new(x, y, z)))
      }
      1 => {
        let x = arg_refs[0].resolve(&args, &kwargs).as_float().unwrap();
        Ok(Value::Vec3(Vec3::new(x, x, x)))
      }
      _ => unimplemented!(),
    },
    "union" => eval_mesh_boolean(def_ix, arg_refs, args, kwargs, ctx, MeshBooleanOp::Union),
    "subtract" | "difference" => eval_mesh_boolean(
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
      let initial_val = arg_refs[0].resolve(&args, &kwargs).clone();
      let fn_value = arg_refs[1].resolve(&args, &kwargs).as_fn().unwrap();
      let sequence = arg_refs[2].resolve(&args, &kwargs).as_sequence().unwrap();

      ctx.fold(
        initial_val,
        fn_value.clone(),
        sequence.clone_box().consume(ctx),
      )
    }
    "reduce" => {
      let fn_value = arg_refs[0].resolve(&args, &kwargs).as_fn().unwrap();
      let sequence = arg_refs[1].resolve(&args, &kwargs).as_sequence().unwrap();

      ctx.reduce(fn_value.clone(), sequence.clone_box())
    }
    "map" => {
      let fn_value = arg_refs[0].resolve(&args, &kwargs).as_fn().unwrap();
      let sequence = arg_refs[1].resolve(&args, &kwargs).as_sequence().unwrap();

      Ok(Value::Sequence(Box::new(MapSeq {
        f: fn_value.clone(),
        inner: sequence.clone_box(),
      })))
    }
    "add" => match def_ix {
      0 => {
        // vec3 + vec3
        let a = arg_refs[0].resolve(&args, &kwargs).as_vec3().unwrap();
        let b = arg_refs[1].resolve(&args, &kwargs).as_vec3().unwrap();
        Ok(Value::Vec3(*a + *b))
      }
      1 => {
        // float + float
        let a = arg_refs[0].resolve(&args, &kwargs).as_float().unwrap();
        let b = arg_refs[1].resolve(&args, &kwargs).as_float().unwrap();
        Ok(Value::Float(a + b))
      }
      2 => {
        // float + int
        let a = arg_refs[0].resolve(&args, &kwargs).as_float().unwrap();
        let b = arg_refs[1].resolve(&args, &kwargs).as_int().unwrap();
        Ok(Value::Float(a + b as f32))
      }
      3 => {
        // int + int
        let a = arg_refs[0].resolve(&args, &kwargs).as_int().unwrap();
        let b = arg_refs[1].resolve(&args, &kwargs).as_int().unwrap();
        Ok(Value::Int(a + b))
      }
      _ => unimplemented!(),
    },
    "mul" => match def_ix {
      0 => {
        let a = arg_refs[0].resolve(&args, &kwargs).as_vec3().unwrap();
        let b = arg_refs[1].resolve(&args, &kwargs).as_vec3().unwrap();
        Ok(Value::Vec3(Vec3::new(a.x * b.x, a.y * b.y, a.z * b.z)))
      }
      1 => {
        let a = arg_refs[0].resolve(&args, &kwargs).as_vec3().unwrap();
        let b = arg_refs[1].resolve(&args, &kwargs).as_float().unwrap();
        Ok(Value::Vec3(a * b))
      }
      2 => {
        let a = arg_refs[0].resolve(&args, &kwargs).as_float().unwrap();
        let b = arg_refs[1].resolve(&args, &kwargs).as_float().unwrap();
        Ok(Value::Float(a * b))
      }
      _ => unimplemented!(),
    },
    "sin" => match def_ix {
      0 => {
        let value = arg_refs[0].resolve(&args, &kwargs).as_float().unwrap();
        Ok(Value::Float(value.sin()))
      }
      _ => unimplemented!(),
    },
    "cos" => match def_ix {
      0 => {
        let value = arg_refs[0].resolve(&args, &kwargs).as_float().unwrap();
        Ok(Value::Float(value.cos()))
      }
      _ => unimplemented!(),
    },
    "tan" => match def_ix {
      0 => {
        let value = arg_refs[0].resolve(&args, &kwargs).as_float().unwrap();
        Ok(Value::Float(value.tan()))
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

      println!("{}, {}", formatted_pos_ags, formatted_kwargs);
      Ok(Value::Nil)
    }
    "render" => match def_ix {
      0 => {
        let mesh = arg_refs[0].resolve(&args, &kwargs).as_mesh().unwrap();
        ctx.rendered_meshes.push(mesh.clone());
        Ok(Value::Nil)
      }
      1 => {
        let sequence = arg_refs[0]
          .resolve(&args, &kwargs)
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
      let mesh = arg_refs[0].resolve(&args, &kwargs).as_mesh().unwrap();
      let count = arg_refs[1].resolve(&args, &kwargs).as_int().unwrap();

      if count < 0 {
        return Err("negative point count is not valid for point_distribute".to_owned());
      }

      let sampler_seq = PointDistributeSeq {
        mesh: mesh.clone(),
        point_count: count as usize,
      };
      Ok(Value::Sequence(Box::new(sampler_seq)))
    }
    _ => unimplemented!("Function `{name}` not yet implemented"),
  }
}
